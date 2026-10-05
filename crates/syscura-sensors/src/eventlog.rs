//! Push-based Windows Event Log sensor built on `EvtSubscribe`.
//!
//! Windows calls our callback on its own thread pool only when a matching
//! record is written, so an idle system costs nothing. The XPath filter
//! (built from the rules) runs inside the Event Log service, so Syscura
//! only ever sees events it cares about.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use syscura_core::{Event, Level, now_ms};
use windows::Win32::System::EventLog::{
    EVT_HANDLE, EVT_SUBSCRIBE_NOTIFY_ACTION, EvtClose, EvtFormatMessage, EvtFormatMessageEvent, EvtNext,
    EvtOpenPublisherMetadata, EvtQuery, EvtQueryChannelPath,
    EvtQueryForwardDirection, EvtRender, EvtRenderEventXml, EvtSubscribe, EvtSubscribeActionDeliver,
    EvtSubscribeToFutureEvents,
};
use windows::core::HSTRING;

use crate::xml::parse_event_xml;

/// Receives every parsed event. Called on a Windows thread-pool thread.
pub type Sink = Box<dyn Fn(Event) + Send + Sync>;

struct Context {
    channel: String,
    sink: Sink,
}

/// A live subscription. Dropping it unsubscribes.
pub struct Subscription {
    handle: EVT_HANDLE,
    // Boxed so its address stays fixed while Windows holds a pointer to it.
    // Freed only after EvtClose, which waits for in-flight callbacks.
    ctx: *mut Context,
}

// The raw pointer is only touched by Windows callbacks and by Drop.
unsafe impl Send for Subscription {}

impl Subscription {
    /// Subscribes to new events on `channel` matching the XPath `query`.
    pub fn new(channel: &str, query: &str, sink: Sink) -> windows::core::Result<Self> {
        let ctx = Box::into_raw(Box::new(Context { channel: channel.to_string(), sink }));
        let result = unsafe {
            EvtSubscribe(
                None,
                None,
                &HSTRING::from(channel),
                &HSTRING::from(query),
                None,
                Some(ctx as *const c_void),
                Some(callback),
                EvtSubscribeToFutureEvents.0,
            )
        };
        match result {
            Ok(handle) => Ok(Subscription { handle, ctx }),
            Err(e) => {
                drop(unsafe { Box::from_raw(ctx) });
                Err(e)
            }
        }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        unsafe {
            let _ = EvtClose(self.handle);
            drop(Box::from_raw(self.ctx));
        }
    }
}

unsafe extern "system" fn callback(
    action: EVT_SUBSCRIBE_NOTIFY_ACTION,
    user_context: *const c_void,
    event: EVT_HANDLE,
) -> u32 {
    if action != EvtSubscribeActionDeliver || user_context.is_null() {
        return 0;
    }
    let ctx = unsafe { &*(user_context as *const Context) };
    if let Some(e) = to_event(event, &ctx.channel) {
        (ctx.sink)(e);
    }
    0
}

fn to_event(event: EVT_HANDLE, channel: &str) -> Option<Event> {
    let mut p = render_xml(event).and_then(|xml| parse_event_xml(&xml))?;
    // Windows' own readable text for the event, kept with the data.
    if let Some(msg) = format_message(&p.provider, event) {
        p.data.insert("_message".into(), msg);
    }
    Some(Event {
        ts: p.time_ms.unwrap_or_else(now_ms),
        source: "eventlog".into(),
        channel: if p.channel.is_empty() { channel.to_string() } else { p.channel },
        provider: p.provider,
        event_id: p.event_id,
        level: Level::from_u8(p.level),
        record_id: p.record_id,
        data: p.data,
    })
}

/// Past events on `channel` matching `query` from the last `days` days,
/// oldest first (so repeat counting works as if they had been seen live).
pub fn history(channel: &str, query: &str, days: u32, max: usize) -> windows::core::Result<Vec<Event>> {
    // Add a time limit inside the existing System[...] filter.
    let ms = days as u64 * 24 * 60 * 60 * 1000;
    let timed = match query.strip_prefix("*[System[").and_then(|q| q.strip_suffix("]]")) {
        Some(inner) => format!("*[System[({inner}) and TimeCreated[timediff(@SystemTime) <= {ms}]]]"),
        None => query.to_string(),
    };
    let handle = unsafe {
        EvtQuery(None, &HSTRING::from(channel), &HSTRING::from(timed), EvtQueryChannelPath.0 | EvtQueryForwardDirection.0)
    }?;
    let mut out = Vec::new();
    let mut batch = [0isize; 64];
    'outer: loop {
        let mut returned = 0u32;
        let ok = unsafe { EvtNext(handle, &mut batch, 1000, 0, &mut returned) };
        if ok.is_err() || returned == 0 {
            break;
        }
        let n = returned as usize;
        for (i, &raw) in batch[..n].iter().enumerate() {
            let h = EVT_HANDLE(raw);
            if let Some(e) = to_event(h, channel) {
                out.push(e);
            }
            let _ = unsafe { EvtClose(h) };
            if out.len() >= max {
                // Close the rest of this batch before stopping.
                for &rest in &batch[i + 1..n] {
                    let _ = unsafe { EvtClose(EVT_HANDLE(rest)) };
                }
                break 'outer;
            }
        }
    }
    let _ = unsafe { EvtClose(handle) };
    Ok(out)
}

/// Publisher metadata handles, opened once per provider (0 = not available).
fn publishers() -> &'static Mutex<HashMap<String, isize>> {
    static P: OnceLock<Mutex<HashMap<String, isize>>> = OnceLock::new();
    P.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The event's message as Event Viewer shows it, from the provider's own
/// message table. Long messages are cut to 2000 characters.
fn format_message(provider: &str, event: EVT_HANDLE) -> Option<String> {
    if provider.is_empty() {
        return None;
    }
    let handle = {
        let mut map = publishers().lock().unwrap_or_else(|e| e.into_inner());
        *map.entry(provider.to_string()).or_insert_with(|| {
            unsafe { EvtOpenPublisherMetadata(None, &HSTRING::from(provider), None, 0, 0) }
                .map(|h| h.0)
                .unwrap_or(0)
        })
    };
    if handle == 0 {
        return None;
    }
    let publisher = EVT_HANDLE(handle);
    let mut used = 0u32;
    let _ = unsafe { EvtFormatMessage(Some(publisher), Some(event), 0, None, EvtFormatMessageEvent.0, None, &mut used) };
    if used == 0 || used > 64 * 1024 {
        return None;
    }
    let mut buf = vec![0u16; used as usize];
    unsafe { EvtFormatMessage(Some(publisher), Some(event), 0, None, EvtFormatMessageEvent.0, Some(&mut buf), &mut used) }
        .ok()?;
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    let text = String::from_utf16_lossy(&buf[..len]).trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(match text.char_indices().nth(2000) {
        Some((i, _)) => format!("{}…", &text[..i]),
        None => text,
    })
}

fn render_xml(event: EVT_HANDLE) -> Option<String> {
    let mut used = 0u32;
    let mut props = 0u32;
    // First call sizes the buffer (it fails with ERROR_INSUFFICIENT_BUFFER).
    let _ = unsafe { EvtRender(None, event, EvtRenderEventXml.0, 0, None, &mut used, &mut props) };
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u16; (used as usize).div_ceil(2)];
    unsafe {
        EvtRender(
            None,
            event,
            EvtRenderEventXml.0,
            (buf.len() * 2) as u32,
            Some(buf.as_mut_ptr() as *mut c_void),
            &mut used,
            &mut props,
        )
        .ok()?;
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}
