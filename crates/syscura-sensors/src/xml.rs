//! Parser for Windows Event Log XML (the `EvtRenderEventXml` format).
//! Platform-independent so it can be tested anywhere.

use std::collections::BTreeMap;

use quick_xml::events::{BytesStart, Event as Xml};
use quick_xml::escape::resolve_predefined_entity;
use quick_xml::{Reader, XmlVersion};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedEvent {
    pub provider: String,
    pub event_id: u32,
    pub level: u8,
    pub record_id: u64,
    pub channel: String,
    /// When Windows recorded the event (Unix ms), from `<TimeCreated>`.
    pub time_ms: Option<i64>,
    /// `<EventData><Data Name=..>` fields, or leaf elements of `<UserData>`.
    /// Unnamed `<Data>` items are keyed by position ("0", "1", ...).
    pub data: BTreeMap<String, String>,
}

enum Capture {
    None,
    EventId,
    Level,
    RecordId,
    Channel,
    Data(String),
}

/// Parses one rendered event. Returns `None` if the XML is malformed or has
/// no `<EventID>`.
pub fn parse_event_xml(xml: &str) -> Option<ParsedEvent> {
    let mut reader = Reader::from_str(xml);
    let mut out = ParsedEvent::default();
    let mut capture = Capture::None;
    let mut text = String::new();
    let mut have_id = false;
    let mut unnamed = 0usize;
    // Depth inside <UserData>; 0 means "not inside".
    let mut user_depth = 0usize;

    loop {
        match reader.read_event().ok()? {
            Xml::Start(e) => {
                let name = e.local_name();
                let name = name.as_ref();
                if user_depth > 0 {
                    user_depth += 1;
                    // <UserData><Wrapper><Field>value</Field>: fields sit at depth 3.
                    if user_depth >= 3 {
                        capture = Capture::Data(name.to_string());
                    }
                } else {
                    capture = match name {
                        "EventID" => Capture::EventId,
                        "Level" => Capture::Level,
                        "EventRecordID" => Capture::RecordId,
                        "Channel" => Capture::Channel,
                        "Data" => Capture::Data(data_name(&e, &mut unnamed)),
                        "UserData" => {
                            user_depth = 1;
                            Capture::None
                        }
                        _ => Capture::None,
                    };
                }
                text.clear();
            }
            Xml::Empty(e) => match e.local_name().as_ref() {
                "Provider" => out.provider = attr(&e, "Name").unwrap_or_default(),
                "TimeCreated" => out.time_ms = attr(&e, "SystemTime").and_then(|t| parse_system_time(&t)),
                "Data" if user_depth == 0 => {
                    out.data.insert(data_name(&e, &mut unnamed), String::new());
                }
                _ => {}
            },
            Xml::Text(t) => text.push_str(&t.xml10_content()),
            Xml::CData(t) => text.push_str(&t),
            Xml::GeneralRef(r) => {
                if let Ok(Some(ch)) = r.resolve_char_ref() {
                    text.push(ch);
                } else if let Some(s) = resolve_predefined_entity(&r.xml10_content()) {
                    text.push_str(s);
                }
            }
            Xml::End(_) => {
                let value = text.trim();
                match std::mem::replace(&mut capture, Capture::None) {
                    Capture::EventId => {
                        out.event_id = value.parse().ok()?;
                        have_id = true;
                    }
                    Capture::Level => out.level = value.parse().unwrap_or(4),
                    Capture::RecordId => out.record_id = value.parse().unwrap_or(0),
                    Capture::Channel => out.channel = value.to_string(),
                    Capture::Data(name) => {
                        out.data.insert(name, value.to_string());
                    }
                    Capture::None => {}
                }
                text.clear();
                user_depth = user_depth.saturating_sub(1);
            }
            Xml::Eof => break,
            _ => {}
        }
    }
    have_id.then_some(out)
}

/// "2026-10-05T10:11:12.1234567Z" -> Unix milliseconds (UTC).
pub fn parse_system_time(s: &str) -> Option<i64> {
    let num = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (h, mi, sec) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    let frac = s.get(19..).unwrap_or("");
    let ms = frac
        .strip_prefix('.')
        .map(|f| {
            let digits: String = f.chars().take_while(char::is_ascii_digit).take(3).collect();
            format!("{digits:0<3}").parse::<i64>().unwrap_or(0)
        })
        .unwrap_or(0);
    // Days since 1970-01-01 (Howard Hinnant's civil-from-days, inverted).
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms)
}

fn attr(e: &BytesStart, key: &str) -> Option<String> {
    let a = e.try_get_attribute(key).ok()??;
    a.normalized_value(XmlVersion::Implicit1_0).ok().map(|v| v.into_owned())
}

fn data_name(e: &BytesStart, unnamed: &mut usize) -> String {
    attr(e, "Name").unwrap_or_else(|| {
        let n = unnamed.to_string();
        *unnamed += 1;
        n
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCM_7031: &str = r#"<Event xmlns='http://schemas.microsoft.com/win/2004/08/events/event'><System><Provider Name='Service Control Manager' Guid='{555908d1-a6d7-4695-8e1e-26931d2012f4}' EventSourceName='Service Control Manager'/><EventID Qualifiers='49152'>7031</EventID><Version>0</Version><Level>2</Level><Task>0</Task><Opcode>0</Opcode><Keywords>0x8080000000000000</Keywords><TimeCreated SystemTime='2026-10-05T10:11:12.1234567Z'/><EventRecordID>48213</EventRecordID><Correlation/><Execution ProcessID='812' ThreadID='9020'/><Channel>System</Channel><Computer>DESKTOP</Computer><Security/></System><EventData><Data Name='param1'>Print Spooler</Data><Data Name='param2'>1</Data><Data Name='param3'>60000</Data><Data Name='param5'/><Binary>530070006F006F006C00</Binary></EventData></Event>"#;

    #[test]
    fn parses_service_crash() {
        let p = parse_event_xml(SCM_7031).unwrap();
        assert_eq!(p.provider, "Service Control Manager");
        assert_eq!(p.event_id, 7031);
        assert_eq!(p.level, 2);
        assert_eq!(p.record_id, 48213);
        assert_eq!(p.channel, "System");
        assert_eq!(p.time_ms, Some(1_791_195_072_123));
        assert_eq!(p.data["param1"], "Print Spooler");
        assert_eq!(p.data["param3"], "60000");
        assert_eq!(p.data["param5"], "");
        assert!(!p.data.contains_key("Binary"));
    }

    #[test]
    fn unnamed_data_and_entities() {
        let xml = "<Event><System><Provider Name='Application Error'/><EventID>1000</EventID>\
                   <Level>2</Level><EventRecordID>7</EventRecordID><Channel>Application</Channel></System>\
                   <EventData><Data>app &amp; co.exe</Data><Data>&#49;.0</Data></EventData></Event>";
        let p = parse_event_xml(xml).unwrap();
        assert_eq!(p.data["0"], "app & co.exe");
        assert_eq!(p.data["1"], "1.0");
    }

    #[test]
    fn user_data_leaves() {
        let xml = "<Event><System><Provider Name='Microsoft-Windows-Eventlog'/><EventID>1102</EventID>\
                   <Level>4</Level><EventRecordID>9</EventRecordID><Channel>Security</Channel></System>\
                   <UserData><LogFileCleared xmlns='x'><SubjectUserName>bob</SubjectUserName>\
                   <SubjectDomainName>PC</SubjectDomainName></LogFileCleared></UserData></Event>";
        let p = parse_event_xml(xml).unwrap();
        assert_eq!(p.data["SubjectUserName"], "bob");
        assert_eq!(p.data["SubjectDomainName"], "PC");
    }

    #[test]
    fn system_times() {
        assert_eq!(parse_system_time("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(parse_system_time("2000-03-01T00:00:01Z"), Some(951_868_801_000));
        assert_eq!(parse_system_time("2024-02-29T12:00:00.5Z"), Some(1_709_208_000_500));
        assert_eq!(parse_system_time("garbage"), None);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_event_xml("<Event><System></System></Event>").is_none());
        assert!(parse_event_xml("not xml <<<").is_none());
    }
}

/// Property tests: event XML comes from other programs' event logs, so the
/// parser must survive anything (`cargo test` runs a few hundred random cases).
#[cfg(test)]
mod fuzz {
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn never_panics_on_any_text(s in ".{0,400}") {
            let _ = super::parse_event_xml(&s);
            let _ = super::parse_system_time(&s);
        }

        #[test]
        fn never_panics_on_xml_like_text(
            tags in proptest::collection::vec("(<|</|/>|>|=|\"|&amp;|&#x0;|Data|Name|EventID|Level|System|TimeCreated|SystemTime|[a-z0-9 ]{0,6})", 0..80)
        ) {
            let s: String = tags.concat();
            let _ = super::parse_event_xml(&format!("<Event>{s}</Event>"));
        }
    }
}
