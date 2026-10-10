//! Connection pre-warming: open pooled HTTPS connections to the hosts a dictation will use.

use super::http::{client, trim_base};
use crate::types::{LlmProvider, Settings};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const DEDUPE: Duration = Duration::from_secs(20);
static LAST: Lazy<Mutex<HashMap<String, Instant>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// True (and records `now`) when `key` was not warmed within the dedupe window.
pub(crate) fn should_warm(map: &mut HashMap<String, Instant>, key: &str, now: Instant, window: Duration) -> bool {
    if map.get(key).is_some_and(|t| now.duration_since(*t) < window) {
        return false;
    }
    map.insert(key.to_string(), now);
    true
}

/// Scheme + host[:port] of a URL.
pub(crate) fn origin(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(trim_base(url)).ok()?;
    let host = u.host_str()?;
    Some(match u.port() {
        Some(p) => format!("{}://{host}:{p}", u.scheme()),
        None => format!("{}://{host}", u.scheme()),
    })
}

/// Origins to warm for the active ASR + rewrite providers.
pub(crate) fn targets(settings: &Settings) -> Vec<String> {
    let mut out = Vec::new();
    match settings.asr.provider.as_str() {
        "mimo" => out.extend(origin(&settings.asr.mimo.base_url)),
        "cloudflare" => out.push("https://api.cloudflare.com".to_string()),
        _ => {}
    }
    let llm = &settings.llm;
    if llm.enabled {
        let ep = llm.active_endpoint();
        let default = match llm.provider {
            LlmProvider::Gemini => "https://generativelanguage.googleapis.com",
            LlmProvider::Anthropic => "https://api.anthropic.com",
            LlmProvider::Openai => "https://api.openai.com/v1",
        };
        let base = if ep.base_url.trim().is_empty() { default } else { ep.base_url.as_str() };
        out.extend(origin(base));
    }
    out.dedup();
    out
}

/// Non-blocking; safe to call at every session start.
pub fn warm_up(settings: &Settings) {
    let urls = targets(settings);
    if urls.is_empty() {
        return;
    }
    let now = Instant::now();
    for url in urls {
        if !should_warm(&mut LAST.lock(), &url, now, DEDUPE) {
            continue;
        }
        tauri::async_runtime::spawn(async move {
            let t = Instant::now();
            let r = client().head(&url).timeout(Duration::from_secs(3)).send().await;
            log::debug!("warm {url}: {} in {} ms", r.map(|r| r.status().as_u16().to_string()).unwrap_or_else(|e| e.to_string()), t.elapsed().as_millis());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedupes_within_window() {
        let mut m = HashMap::new();
        let t0 = Instant::now();
        let w = Duration::from_secs(20);
        assert!(should_warm(&mut m, "a", t0, w));
        assert!(!should_warm(&mut m, "a", t0 + Duration::from_secs(5), w));
        assert!(should_warm(&mut m, "b", t0 + Duration::from_secs(5), w));
        assert!(should_warm(&mut m, "a", t0 + Duration::from_secs(21), w));
    }

    #[test]
    fn origins_and_targets() {
        assert_eq!(origin("https://api.xiaomimimo.com/v1/").unwrap(), "https://api.xiaomimimo.com");
        assert_eq!(origin("http://127.0.0.1:8080/v1").unwrap(), "http://127.0.0.1:8080");
        let mut s = Settings::default();
        let t = targets(&s);
        assert!(t.contains(&"https://api.xiaomimimo.com".to_string()));
        assert!(t.contains(&"https://generativelanguage.googleapis.com".to_string()));
        s.asr.provider = "azure".into();
        s.llm.provider = LlmProvider::Anthropic;
        assert_eq!(targets(&s), vec!["https://api.anthropic.com".to_string()]);
    }
}
