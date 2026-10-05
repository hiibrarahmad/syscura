//! The knowledge base: rules that turn raw events into findings with a
//! plain-language explanation, a "is it harmful?" verdict and safe fixes.
//!
//! Rules are data (`kb/rules.toml`), not code, so they can be reviewed and
//! updated without touching the engine.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::Deserialize;
use syscura_core::findings::{Harm, Risk};
use syscura_core::{Event, Level};

/// The rules shipped with Syscura.
pub const BUILTIN: &str = include_str!("../../../kb/rules.toml");

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleBook {
    #[serde(rename = "rule")]
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub title: String,
    pub category: String,
    pub severity: Level,
    pub harmful: Harm,
    /// Template; `{Field}` is replaced with event data, `{count}` with the
    /// number of occurrences.
    pub explain: String,
    #[serde(default)]
    pub advice: String,
    /// Web search for this problem (template, like `explain`).
    #[serde(default)]
    pub search: String,
    #[serde(rename = "match")]
    pub matcher: Matcher,
    #[serde(default)]
    pub threshold: Option<Threshold>,
    /// Event data field that separates findings (e.g. one per service).
    #[serde(default)]
    pub group_by: Option<String>,
    /// Extra checks the agent runs on the evidence (e.g. "service_image").
    #[serde(default)]
    pub enrich: Option<String>,
    #[serde(default, rename = "fix")]
    pub fixes: Vec<FixSpec>,
    #[serde(default)]
    pub verify: Vec<VerifySpec>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matcher {
    pub channel: String,
    /// Case-insensitive substring of the provider name.
    #[serde(default)]
    pub provider: Option<String>,
    pub event_ids: Vec<u32>,
    /// Only these levels (1 = critical ... 5 = verbose).
    #[serde(default)]
    pub levels: Vec<u8>,
    /// Field -> case-insensitive substring that must be present.
    #[serde(default)]
    pub data: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Threshold {
    pub count: u32,
    pub within_mins: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FixSpec {
    pub action: String,
    pub label: String,
    pub risk: Risk,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
    /// Safe fixes run automatically unless this is false (e.g. for long,
    /// heavy scans that are harmless but should not start by surprise).
    #[serde(default = "yes")]
    pub auto: bool,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifySpec {
    /// "service_running" or "quiet".
    pub probe: String,
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

/// A rule matched an event (and its threshold, if any, was reached).
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub rule: String,
    pub group: String,
    /// Event data plus `provider` and `event_id`.
    pub evidence: BTreeMap<String, String>,
}

pub struct Engine {
    book: RuleBook,
    /// Recent match times per (rule, group), for thresholds.
    recent: HashMap<(usize, String), VecDeque<i64>>,
}

const MAX_GROUP: usize = 120;

impl Engine {
    pub fn new(book: RuleBook) -> Self {
        Engine { book, recent: HashMap::new() }
    }

    pub fn builtin() -> Result<Self, String> {
        let book: RuleBook = toml::from_str(BUILTIN).map_err(|e| format!("bad built-in rules: {e}"))?;
        Ok(Engine::new(book))
    }

    pub fn rules(&self) -> &[Rule] {
        &self.book.rules
    }

    pub fn rule(&self, id: &str) -> Option<&Rule> {
        self.book.rules.iter().find(|r| r.id == id)
    }

    /// Event Log query (XPath) per channel covering every rule, plus all
    /// critical/error/warning events for the two main logs.
    pub fn subscriptions(&self) -> BTreeMap<String, String> {
        let mut per: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for ch in ["System", "Application"] {
            per.entry(ch.into()).or_default().push("(Level=1 or Level=2 or Level=3)".into());
        }
        for r in &self.book.rules {
            let m = &r.matcher;
            let ids = m.event_ids.iter().map(|id| format!("EventID={id}")).collect::<Vec<_>>().join(" or ");
            let mut clause = format!("({ids})");
            if !m.levels.is_empty() {
                let lv = m.levels.iter().map(|l| format!("Level={l}")).collect::<Vec<_>>().join(" or ");
                clause = format!("({clause} and ({lv}))");
            }
            let list = per.entry(m.channel.clone()).or_default();
            if !list.contains(&clause) {
                list.push(clause);
            }
        }
        per.into_iter().map(|(ch, clauses)| (ch, format!("*[System[{}]]", clauses.join(" or ")))).collect()
    }

    /// Checks one event against every rule.
    pub fn evaluate(&mut self, e: &Event) -> Vec<Hit> {
        let mut hits = Vec::new();
        for (i, r) in self.book.rules.iter().enumerate() {
            let m = &r.matcher;
            if !m.channel.eq_ignore_ascii_case(&e.channel)
                || !m.event_ids.contains(&e.event_id)
                || (!m.levels.is_empty() && !m.levels.contains(&(e.level as u8)))
                || m.provider.as_ref().is_some_and(|p| !contains_ci(&e.provider, p))
                || m.data.iter().any(|(k, v)| !e.data.get(k).is_some_and(|d| contains_ci(d, v)))
            {
                continue;
            }
            let group: String = r
                .group_by
                .as_ref()
                .and_then(|g| e.data.get(g))
                .map(|g| g.chars().take(MAX_GROUP).collect())
                .unwrap_or_default();
            if let Some(t) = &r.threshold {
                let window = t.within_mins as i64 * 60_000;
                let q = self.recent.entry((i, group.clone())).or_default();
                q.push_back(e.ts);
                while q.front().is_some_and(|&ts| ts < e.ts - window) {
                    q.pop_front();
                }
                if (q.len() as u32) < t.count {
                    continue;
                }
            }
            let mut evidence = e.data.clone();
            evidence.insert("provider".into(), e.provider.clone());
            evidence.insert("event_id".into(), e.event_id.to_string());
            hits.push(Hit { rule: r.id.clone(), group, evidence });
        }
        hits
    }
}

/// Fills `{Field}` and `{count}` in a template. Unknown fields become "?".
pub fn render(template: &str, evidence: &BTreeMap<String, String>, count: u64) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = &rest[start + 1..start + end];
        let value = if key == "count" {
            count.to_string()
        } else {
            evidence.get(key).map(|v| clean(v)).unwrap_or_else(|| "?".into())
        };
        out.push_str(&value);
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Event text can be long or multi-line; keep explanations readable.
fn clean(v: &str) -> String {
    let one_line: String = v.split_whitespace().collect::<Vec<_>>().join(" ");
    match one_line.char_indices().nth(160) {
        Some((i, _)) => format!("{}…", &one_line[..i]),
        None => one_line,
    }
}

fn contains_ci(haystack: &str, needle: &str) -> bool {
    haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(channel: &str, provider: &str, id: u32, level: Level, ts: i64, data: &[(&str, &str)]) -> Event {
        Event {
            ts,
            source: "eventlog".into(),
            channel: channel.into(),
            provider: provider.into(),
            event_id: id,
            level,
            record_id: ts as u64,
            data: data.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        }
    }

    #[test]
    fn builtin_rules_parse_and_are_sane() {
        let e = Engine::builtin().unwrap();
        assert!(e.rules().len() >= 15, "expected a real rule set");
        let mut ids = std::collections::HashSet::new();
        for r in e.rules() {
            assert!(ids.insert(r.id.clone()), "duplicate rule id {}", r.id);
            assert!(!r.matcher.event_ids.is_empty(), "{} matches nothing", r.id);
            for f in &r.fixes {
                assert!(KNOWN_ACTIONS.contains(&f.action.as_str()), "{}: unknown action {}", r.id, f.action);
            }
        }
    }

    /// Every action a rule may use. The agent implements exactly these.
    const KNOWN_ACTIONS: &[&str] = &[
        "service.ensure_running", "service.disable", "dns.flush", "time.resync", "defender.quick_scan",
        "defender.full_scan", "defender.update", "defender.enable_realtime", "sfc.scan", "dism.restore_health",
        "chkdsk.scan", "winsock.reset", "wu.reset_cache", "restore_point",
    ];

    #[test]
    fn service_crash_groups_by_service() {
        let mut e = Engine::builtin().unwrap();
        let hits = e.evaluate(&ev("System", "Service Control Manager", 7031, Level::Error, 1000, &[("param1", "Print Spooler")]));
        let h = hits.iter().find(|h| h.rule == "svc.crash").expect("svc.crash should match");
        assert_eq!(h.group, "Print Spooler");
        assert_eq!(h.evidence["event_id"], "7031");
    }

    #[test]
    fn thresholds_wait_for_repeats() {
        let mut e = Engine::builtin().unwrap();
        let dns = |ts| ev("System", "Microsoft-Windows-DNS-Client", 1014, Level::Warning, ts, &[("QueryName", "example.com")]);
        let mut fired = 0;
        for i in 0..10 {
            if e.evaluate(&dns(i * 1000)).iter().any(|h| h.rule == "net.dns") {
                fired += 1;
            }
        }
        let t = e.rule("net.dns").unwrap().threshold.as_ref().unwrap().count;
        assert_eq!(fired, 10 - (t as usize - 1), "fires from the threshold on");
    }

    #[test]
    fn levels_filter() {
        let mut e = Engine::builtin().unwrap();
        let verbose = ev("Microsoft-Windows-PowerShell/Operational", "Microsoft-Windows-PowerShell", 4104, Level::Verbose, 1, &[]);
        assert!(e.evaluate(&verbose).is_empty(), "normal script logging is not suspicious");
        let warn = ev("Microsoft-Windows-PowerShell/Operational", "Microsoft-Windows-PowerShell", 4104, Level::Warning, 1, &[]);
        assert!(e.evaluate(&warn).iter().any(|h| h.rule == "sec.powershell"));
    }

    #[test]
    fn templates() {
        let mut ev = BTreeMap::new();
        ev.insert("param1".to_string(), "Print   Spooler\n".to_string());
        assert_eq!(render("{param1} crashed {count} times ({missing})", &ev, 3), "Print Spooler crashed 3 times (?)");
        assert_eq!(render("no braces", &ev, 1), "no braces");
        assert_eq!(render("open {param1", &ev, 1), "open {param1");
    }

    #[test]
    fn subscription_queries() {
        let e = Engine::builtin().unwrap();
        let subs = e.subscriptions();
        assert!(subs["System"].contains("Level=1"));
        assert!(subs["System"].contains("EventID=7045"), "info-level new-service events are included");
        let ps = &subs["Microsoft-Windows-PowerShell/Operational"];
        assert!(ps.contains("EventID=4104") && ps.contains("Level=3"));
        assert!(!ps.contains("Level=1 or Level=2 or Level=3)"), "no blanket level filter outside System/Application");
    }
}
