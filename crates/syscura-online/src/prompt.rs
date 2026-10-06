//! What Syscura sends to the AI and how it reads the answer. Pure
//! functions, so the safety rules are unit-tested.
//!
//! Safety model: the AI never writes commands. It may only pick actions
//! from Syscura's fixed catalog (by id, with named parameters); anything it
//! suggests outside the catalog becomes written steps for the person. The
//! agent re-checks every pick and decides itself whether it is safe enough
//! to run without asking.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use syscura_core::findings::ActionInfo;

/// What the AI is asked about.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Question {
    /// Short title, e.g. "A Windows service crashed".
    pub title: String,
    /// Plain-language explanation Syscura already has, if any.
    pub explanation: String,
    /// Windows' message text and event fields.
    pub details: BTreeMap<String, String>,
    /// "Windows 11 Pro 24H2 (26100.2314), ASUS ..., AMD Ryzen ..."
    pub system: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProposedAction {
    pub action: String,
    pub params: BTreeMap<String, String>,
    pub why: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Source {
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Analysis {
    /// One or two sentences: what is going on.
    pub summary: String,
    pub likely_cause: String,
    /// "no", "maybe" or "yes".
    pub harmful: String,
    /// Only when the question includes a fix that was just tried: "yes" if
    /// its result shows the problem is fixed, "no" if not, "unsure" if the
    /// result does not tell. Empty otherwise.
    pub fixed: String,
    /// Steps for the person (things Syscura cannot do itself).
    pub steps: Vec<String>,
    /// Picks from the action catalog, in the order to try them.
    pub actions: Vec<ProposedAction>,
    /// Web pages the answer was based on (from Google Search grounding).
    pub sources: Vec<Source>,
    pub model: String,
}

pub fn system_instruction() -> &'static str {
    "You are Syscura's Windows troubleshooting assistant. You explain Windows problems to \
     non-technical people and choose safe fixes.\n\
     Rules:\n\
     - Use Google Search to check what the error, event ID or stop code means and how it is \
       usually fixed on this Windows version.\n\
     - Be accurate. If you are not sure, say so. Never invent event meanings.\n\
     - You can NOT run commands. You may only pick actions from the provided catalog, by id, \
       with the listed parameter names. Pick nothing if no catalog action fits.\n\
     - Never pick actions that would delete user data. Prefer the least invasive fix first.\n\
     - Anything else the person should do goes into \"steps\", written plainly, one action per step.\n\
     - If the details include \"Fix that was tried\" and \"Result of the fix\", judge from that \
       result whether the problem is now fixed: set \"fixed\" to \"yes\", \"no\" or \"unsure\", start \
       the summary with that verdict, and if it is not fixed, give the next fix to try. \
       Otherwise leave \"fixed\" empty.\n\
     - Reply with ONLY a JSON object, no other text, in exactly this shape:\n\
     {\"summary\": \"...\", \"likely_cause\": \"...\", \"harmful\": \"no|maybe|yes\", \"fixed\": \"yes|no|unsure|\", \
     \"steps\": [\"...\"], \"actions\": [{\"action\": \"<catalog id>\", \"params\": {\"name\": \"value\"}, \"why\": \"...\"}]}"
}

/// The user-turn text: the problem, the system, and the action catalog.
pub fn build_prompt(q: &Question, catalog: &[ActionInfo]) -> String {
    let mut out = String::new();
    out.push_str(&format!("Problem: {}\n", q.title));
    if !q.explanation.is_empty() {
        out.push_str(&format!("What Syscura knows: {}\n", q.explanation));
    }
    if !q.details.is_empty() {
        out.push_str("Details:\n");
        for (k, v) in &q.details {
            let v: String = v.chars().take(1500).collect();
            out.push_str(&format!("- {k}: {v}\n"));
        }
    }
    out.push_str(&format!("System: {}\n\n", q.system));
    out.push_str("Action catalog (the only fixes you may pick):\n");
    for a in catalog {
        let params = if a.params.is_empty() { String::new() } else { format!(" params: [{}]", a.params.join(", ")) };
        out.push_str(&format!("- {}: {}{params}\n", a.id, a.description));
    }
    out
}

/// A plain question for a free AI website (Gemini, ChatGPT, Claude,
/// Copilot) that the person opens themselves. No catalog: the answer goes
/// to a human, who stays in control.
pub fn web_prompt(q: &Question) -> String {
    let mut out = String::from("My Windows PC has a problem. Please help me like I'm not a computer expert.

");
    out.push_str(&format!("Problem: {}
", q.title));
    if !q.explanation.is_empty() {
        out.push_str(&format!("What I already know: {}
", q.explanation));
    }
    for (k, v) in &q.details {
        let v: String = v.chars().take(800).collect();
        out.push_str(&format!("{k}: {v}
"));
    }
    if !q.system.is_empty() {
        out.push_str(&format!("My system: {}
", q.system));
    }
    out.push_str(
        "
Please tell me:
         1. What this means, in simple words.
         2. Whether it is harmful or dangerous for my PC or my files.
         3. Step by step, how to fix it safely, starting with the least risky step.
         If I already tried a fix (shown above with its result), first tell me whether that result means it is fixed.
         Warn me clearly before any step that could lose data, and tell me to back up my files first if there is any risk.",
    );
    out
}

/// Reads the AI's reply. Tolerates code fences and text around the JSON.
/// Actions that are not in the catalog, or that use unknown parameter
/// names, are dropped here (the agent checks again before running).
pub fn parse_reply(text: &str, catalog: &[ActionInfo]) -> Result<Analysis, String> {
    let start = text.find('{').ok_or("The AI did not answer in the expected format.")?;
    let end = text.rfind('}').ok_or("The AI did not answer in the expected format.")?;
    if end < start {
        return Err("The AI did not answer in the expected format.".into());
    }
    let mut a: Analysis =
        serde_json::from_str(&text[start..=end]).map_err(|e| format!("Could not read the AI's answer: {e}"))?;
    a.actions.retain(|p| {
        catalog
            .iter()
            .find(|c| c.id == p.action)
            .is_some_and(|c| p.params.keys().all(|k| c.params.contains(k)))
    });
    a.actions.dedup_by(|x, y| x.action == y.action && x.params == y.params);
    a.harmful = match a.harmful.to_ascii_lowercase().as_str() {
        h @ ("no" | "maybe" | "yes") => h.to_string(),
        _ => "maybe".into(),
    };
    a.fixed = match a.fixed.to_ascii_lowercase().as_str() {
        f @ ("yes" | "no" | "unsure") => f.to_string(),
        _ => String::new(),
    };
    a.steps.retain(|s| !s.trim().is_empty());
    Ok(a)
}

/// Private details that must not leave the PC.
#[derive(Debug, Clone, Default)]
pub struct Secrets {
    pub user_name: String,
    pub computer_name: String,
    pub profile_dir: String,
}

impl Secrets {
    pub fn from_env() -> Self {
        let get = |k: &str| std::env::var(k).unwrap_or_default();
        Secrets { user_name: get("USERNAME"), computer_name: get("COMPUTERNAME"), profile_dir: get("USERPROFILE") }
    }
}

/// Masks the user name, PC name, profile path, e-mail and IP addresses.
pub fn redact(text: &str, s: &Secrets) -> String {
    let mut out = text.to_string();
    if !s.profile_dir.is_empty() {
        out = replace_ci(&out, &s.profile_dir, "%USERPROFILE%");
    }
    // Short names would mangle normal words.
    if s.user_name.len() >= 3 {
        out = replace_ci(&out, &s.user_name, "<user>");
    }
    if s.computer_name.len() >= 3 {
        out = replace_ci(&out, &s.computer_name, "<pc>");
    }
    out.split_inclusive(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"' | '\''))
        .map(|tok| {
            let core = tok.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '(' | ')' | '"' | '\''));
            let tail = &tok[core.len()..];
            if looks_like_email(core) {
                format!("<email>{tail}")
            } else if looks_like_ipv4(core) {
                format!("<ip>{tail}")
            } else {
                tok.to_string()
            }
        })
        .collect()
}

fn replace_ci(haystack: &str, needle: &str, with: &str) -> String {
    let lower = haystack.to_lowercase();
    let n = needle.to_lowercase();
    if n.is_empty() || lower.len() != haystack.len() {
        return haystack.replace(needle, with);
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while let Some(pos) = lower[i..].find(&n) {
        // Lowercasing can move character boundaries in rare scripts; never
        // cut inside a character.
        if !haystack.is_char_boundary(i + pos) || !haystack.is_char_boundary(i + pos + n.len()) {
            break;
        }
        out.push_str(&haystack[i..i + pos]);
        out.push_str(with);
        i += pos + n.len();
    }
    out.push_str(&haystack[i..]);
    out
}

fn looks_like_email(s: &str) -> bool {
    let Some((user, domain)) = s.split_once('@') else { return false };
    !user.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

fn looks_like_ipv4(s: &str) -> bool {
    let s = s.split(':').next().unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.parse::<u8>().is_ok())
}

/// Free models to try, best first: stable "flash" models newest first,
/// then "flash-lite" ones (lower quality but much higher free limits).
/// Brand-new models often have little or no free quota, so callers move
/// down this list when Google says the limit is reached.
pub fn model_candidates(names: &[String]) -> Vec<String> {
    let version = |n: &str| -> f64 {
        n.strip_prefix("gemini-").and_then(|r| r.split('-').next()).and_then(|v| v.parse::<f64>().ok()).unwrap_or(0.0)
    };
    let mut flash = Vec::new();
    let mut lite = Vec::new();
    for raw in names {
        let n = raw.trim_start_matches("models/").to_string();
        let l = n.to_ascii_lowercase();
        if !l.starts_with("gemini-") || !l.contains("flash") {
            continue;
        }
        if ["preview", "exp", "image", "tts", "audio", "live", "thinking", "8b"].iter().any(|b| l.contains(b)) {
            continue;
        }
        if l.contains("lite") { lite.push(n) } else { flash.push(n) }
    }
    for list in [&mut flash, &mut lite] {
        list.sort_by(|a, b| version(b).total_cmp(&version(a)).then_with(|| a.len().cmp(&b.len())));
        list.dedup();
    }
    flash.into_iter().chain(lite).collect()
}

/// Chooses the best free, general-purpose model from the API's model list:
/// the newest stable "flash" model (not lite, not a preview, not an
/// image/audio/live variant).
pub fn pick_model(names: &[String]) -> Option<String> {
    let version = |n: &str| -> f64 {
        n.trim_start_matches("models/")
            .strip_prefix("gemini-")
            .and_then(|r| r.split('-').next())
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
    };
    let usable = |n: &str| {
        let l = n.to_ascii_lowercase();
        l.contains("gemini-")
            && l.contains("flash")
            && !["lite", "preview", "exp", "image", "tts", "audio", "live", "thinking", "8b"].iter().any(|b| l.contains(b))
    };
    names
        .iter()
        .filter(|n| usable(n))
        .max_by(|a, b| version(a).total_cmp(&version(b)).then_with(|| b.len().cmp(&a.len())))
        .map(|n| n.trim_start_matches("models/").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use syscura_core::findings::Risk;

    fn catalog() -> Vec<ActionInfo> {
        vec![
            ActionInfo { id: "service.ensure_running".into(), label: "Start".into(), description: "Start a service".into(), risk: Risk::Safe, params: vec!["service".into()], undoable: false, needs_admin: true },
            ActionInfo { id: "dns.flush".into(), label: "Flush DNS".into(), description: "Clear DNS".into(), risk: Risk::Safe, params: vec![], undoable: false, needs_admin: false },
        ]
    }

    #[test]
    fn parses_fenced_json_and_drops_unknown_actions() {
        let reply = "Sure!\n```json\n{\"summary\":\"The spooler crashed.\",\"likely_cause\":\"A bad driver\",\"harmful\":\"Maybe\",\
            \"steps\":[\"Update the printer driver\", \" \"],\
            \"actions\":[{\"action\":\"service.ensure_running\",\"params\":{\"service\":\"Spooler\"},\"why\":\"restart\"},\
            {\"action\":\"cmd.run\",\"params\":{\"command\":\"format c:\"}},\
            {\"action\":\"dns.flush\",\"params\":{\"evil\":\"x\"}},\
            {\"action\":\"dns.flush\"}]}\n```";
        let a = parse_reply(reply, &catalog()).unwrap();
        assert_eq!(a.summary, "The spooler crashed.");
        assert_eq!(a.harmful, "maybe");
        assert_eq!(a.steps, vec!["Update the printer driver"]);
        let ids: Vec<&str> = a.actions.iter().map(|x| x.action.as_str()).collect();
        assert_eq!(ids, vec!["service.ensure_running", "dns.flush"], "unknown action and unknown param dropped");
    }

    #[test]
    fn reads_the_fix_verdict() {
        let a = parse_reply(r#"{"summary":"Not fixed.","fixed":"No","harmful":"no"}"#, &catalog()).unwrap();
        assert_eq!(a.fixed, "no");
        let b = parse_reply(r#"{"summary":"x","fixed":"probably"}"#, &catalog()).unwrap();
        assert_eq!(b.fixed, "", "anything else counts as no verdict");
    }

    #[test]
    fn rejects_non_json() {
        assert!(parse_reply("I can't help with that", &catalog()).is_err());
    }

    #[test]
    fn prompt_lists_the_catalog_only() {
        let q = Question { title: "Spooler crashed".into(), system: "Windows 11".into(), ..Default::default() };
        let p = build_prompt(&q, &catalog());
        assert!(p.contains("- service.ensure_running: Start a service params: [service]"));
        assert!(p.contains("- dns.flush: Clear DNS\n"));
    }

    #[test]
    fn web_prompt_is_plain_and_asks_for_safety() {
        let q = Question { title: "Disk error".into(), details: BTreeMap::from([("Event ID".into(), "7".into())]), system: "Windows 11".into(), ..Default::default() };
        let p = web_prompt(&q);
        assert!(p.contains("Problem: Disk error") && p.contains("Event ID: 7") && p.contains("My system: Windows 11"));
        assert!(p.contains("back up my files first"));
        assert!(!p.contains("catalog"), "no machine instructions in a human prompt");
    }

    #[test]
    fn redaction_handles_non_english_names() {
        let s = Secrets { user_name: "عمران".into(), computer_name: "PC-محمد".into(), profile_dir: r"C:\Users\عمران".into() };
        let t = r"Failed for C:\Users\عمران\Desktop\İstanbul ß.exe on PC-محمد by عمران";
        assert_eq!(redact(t, &s), r"Failed for %USERPROFILE%\Desktop\İstanbul ß.exe on <pc> by <user>");
    }

    #[test]
    fn redaction() {
        let s = Secrets { user_name: "hiibr".into(), computer_name: "DESKTOP-ABC1".into(), profile_dir: r"C:\Users\hiibr".into() };
        let t = r"Failed for C:\Users\HIIBR\AppData\x.exe on DESKTOP-ABC1 by hiibr (me@mail.com) from 192.168.1.20, version 10.0.26100";
        assert_eq!(
            redact(t, &s),
            r"Failed for %USERPROFILE%\AppData\x.exe on <pc> by <user> (<email>) from <ip>, version 10.0.26100"
        );
    }

    #[test]
    fn model_choice() {
        let names: Vec<String> = [
            "models/gemini-1.5-flash", "models/gemini-2.0-flash", "models/gemini-2.5-flash", "models/gemini-2.5-flash-lite",
            "models/gemini-2.5-pro", "models/gemini-3.0-flash-preview", "models/gemini-2.0-flash-exp-image-generation", "models/embedding-001",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(pick_model(&names).as_deref(), Some("gemini-2.5-flash"));
        assert_eq!(pick_model(&[]), None);
        assert_eq!(
            model_candidates(&names),
            vec!["gemini-2.5-flash", "gemini-2.0-flash", "gemini-1.5-flash", "gemini-2.5-flash-lite"]
        );
    }
}
