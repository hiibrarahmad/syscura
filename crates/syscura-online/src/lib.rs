//! Free online help: an optional AI assistant (Google Gemini's free tier,
//! with the user's own key) that searches the web for a problem, explains
//! it, and picks fixes from Syscura's fixed action catalog.

pub mod prompt;
#[cfg(windows)]
pub mod secrets;

use std::time::Duration;

use serde_json::{Value, json};
use syscura_core::findings::ActionInfo;

pub use prompt::{Analysis, ProposedAction, Question, Secrets, Source};

const API: &str = "https://generativelanguage.googleapis.com/v1beta";
const MAX_REPLY: u64 = 2 * 1024 * 1024;

pub struct Gemini {
    agent: ureq::Agent,
    key: String,
}

impl Gemini {
    pub fn new(key: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(90)))
            .http_status_as_error(false)
            .build()
            .into();
        Gemini { agent, key: key.trim().to_string() }
    }

    fn get_json(&self, url: &str) -> Result<Value, String> {
        let mut resp = self
            .agent
            .get(url)
            .header("x-goog-api-key", &self.key)
            .call()
            .map_err(|e| format!("Could not reach Google: {e}"))?;
        let status = resp.status().as_u16();
        let body = resp.body_mut().with_config().limit(MAX_REPLY).read_to_string().map_err(|e| e.to_string())?;
        check(status, &body)
    }

    /// Names of the models this key can use ("models/gemini-2.5-flash", ...).
    pub fn models(&self) -> Result<Vec<String>, String> {
        let v = self.get_json(&format!("{API}/models?pageSize=200"))?;
        Ok(v["models"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter(|m| {
                        m["supportedGenerationMethods"]
                            .as_array()
                            .is_some_and(|s| s.iter().any(|x| x == "generateContent"))
                    })
                    .filter_map(|m| m["name"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Checks the key and returns the model Syscura will use.
    pub fn verify(&self) -> Result<String, String> {
        prompt::pick_model(&self.models()?).ok_or_else(|| "This key has no Gemini Flash model available.".to_string())
    }

    /// Asks with `preferred` first; when Google says a model's free limit is
    /// reached, tries the other free models, and finally answers without
    /// web search (which has its own, smaller limit).
    pub fn analyze_with_fallback(&self, preferred: Option<&str>, q: &Question, catalog: &[ActionInfo]) -> Result<Analysis, String> {
        let mut models = prompt::model_candidates(&self.models()?);
        if let Some(p) = preferred
            && let Some(i) = models.iter().position(|m| m == p)
        {
            let m = models.remove(i);
            models.insert(0, m);
        }
        if models.is_empty() {
            return Err("This key has no free Gemini Flash model available.".into());
        }
        let mut last = String::new();
        for search in [true, false] {
            for m in models.iter().take(5) {
                match self.ask(m, q, catalog, search) {
                    Ok(a) => return Ok(a),
                    Err(e) if e.contains("free AI limit") || e.contains("(404)") || e.contains("(503)") => last = e,
                    Err(e) => return Err(e),
                }
            }
        }
        Err(if last.is_empty() { "The AI did not answer.".into() } else { last })
    }

    /// Asks the AI about a problem. Uses Google Search grounding, so the
    /// answer is based on current web results, with sources.
    pub fn analyze(&self, model: &str, q: &Question, catalog: &[ActionInfo]) -> Result<Analysis, String> {
        self.ask(model, q, catalog, true)
    }

    fn ask(&self, model: &str, q: &Question, catalog: &[ActionInfo], search: bool) -> Result<Analysis, String> {
        let secrets = Secrets::from_env();
        let mut q = q.clone();
        q.title = prompt::redact(&q.title, &secrets);
        q.explanation = prompt::redact(&q.explanation, &secrets);
        q.details = q.details.iter().map(|(k, v)| (k.clone(), prompt::redact(v, &secrets))).collect();
        q.system = prompt::redact(&q.system, &secrets);

        let mut body = json!({
            "systemInstruction": { "parts": [{ "text": prompt::system_instruction() }] },
            "contents": [{ "role": "user", "parts": [{ "text": prompt::build_prompt(&q, catalog) }] }],
            "generationConfig": { "temperature": 0.2 }
        });
        if search {
            body["tools"] = json!([{ "google_search": {} }]);
        }
        let url = format!("{API}/models/{model}:generateContent");
        let mut resp = self
            .agent
            .post(&url)
            .header("x-goog-api-key", &self.key)
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| format!("Could not reach Google: {e}"))?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().with_config().limit(MAX_REPLY).read_to_string().map_err(|e| e.to_string())?;
        let v = check(status, &text)?;

        let cand = &v["candidates"][0];
        let reply: String = cand["content"]["parts"]
            .as_array()
            .map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        if reply.is_empty() {
            let reason = cand["finishReason"].as_str().unwrap_or("no answer");
            return Err(format!("The AI returned no answer ({reason})."));
        }
        let mut a = prompt::parse_reply(&reply, catalog)?;
        a.model = model.to_string();
        a.sources = cand["groundingMetadata"]["groundingChunks"]
            .as_array()
            .map(|chunks| {
                chunks
                    .iter()
                    .filter_map(|c| {
                        Some(Source {
                            title: c["web"]["title"].as_str()?.to_string(),
                            url: c["web"]["uri"].as_str()?.to_string(),
                        })
                    })
                    .take(6)
                    .collect()
            })
            .unwrap_or_default();
        Ok(a)
    }
}

/// Turns API errors into messages a person can act on.
fn check(status: u16, body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if status == 200 {
        return Ok(v);
    }
    let msg = v["error"]["message"].as_str().unwrap_or("").to_string();
    Err(match status {
        400 if msg.to_ascii_lowercase().contains("api key") => "That API key is not valid. Copy it again from Google AI Studio.".into(),
        401 | 403 => "Google refused the key. Check it in Google AI Studio.".into(),
        429 => "The free AI limit is reached for now. Syscura will try again later.".into(),
        _ => format!("Google answered with an error ({status}). {msg}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_are_readable() {
        assert!(check(429, "{}").unwrap_err().contains("free AI limit"));
        assert!(check(400, r#"{"error":{"message":"API key not valid. Please pass a valid API key."}}"#).unwrap_err().contains("not valid"));
        assert!(check(200, r#"{"ok":true}"#).is_ok());
    }
}
