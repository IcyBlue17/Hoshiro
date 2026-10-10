//! Provider-reported usage, stored without prompts, keys or transcript content.
use crate::{db::Db, state::AppState, types::events};
use once_cell::sync::OnceCell;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceCell<AppHandle> = OnceCell::new();
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
}

pub fn tokens(v: &Value) -> (Option<u64>, Option<u64>) {
    let number = |paths: &[&str]| paths.iter().find_map(|p| v.pointer(p).and_then(Value::as_u64));
    let mut input = number(&["/usage/prompt_tokens", "/usage/input_tokens", "/usageMetadata/promptTokenCount"]);
    // Anthropic reports cache reads/writes separately from ordinary input tokens.
    if v.pointer("/usage/input_tokens").is_some() {
        input = input.map(|n| {
            n + number(&["/usage/cache_read_input_tokens"]).unwrap_or(0)
                + number(&["/usage/cache_creation_input_tokens"]).unwrap_or(0)
        });
    }
    let mut output =
        number(&["/usage/completion_tokens", "/usage/output_tokens", "/usageMetadata/candidatesTokenCount"]);
    if let Some(thoughts) = number(&["/usageMetadata/thoughtsTokenCount"]) {
        output = Some(output.unwrap_or(0) + thoughts);
    }
    (input, output)
}

/// Cache-hit input tokens, when the provider reports them.
pub fn cached_tokens(v: &Value) -> Option<u64> {
    ["/usage/cache_read_input_tokens", "/usage/prompt_tokens_details/cached_tokens", "/usageMetadata/cachedContentTokenCount"]
        .iter()
        .find_map(|p| v.pointer(p).and_then(Value::as_u64))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Asr,
    Llm,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Asr => "asr",
            Kind::Llm => "llm",
        }
    }
}

/// Speech-recognition services are billed by audio duration; everything else is an LLM billed by tokens.
pub fn classify(provider: &str) -> Kind {
    let p = provider.trim().to_ascii_lowercase();
    // Cloudflare speech is recorded as exactly "Cloudflare"; its rewrite models as "Cloudflare Workers AI".
    if p == "cloudflare" || ["mimo", "azure", "google"].iter().any(|n| p.starts_with(n)) {
        Kind::Asr
    } else {
        Kind::Llm
    }
}

/// Linear-interpolated percentile (`p` in 0..=1) of an ascending-sorted slice; 0 when empty.
pub fn percentile(sorted: &[u64], p: f64) -> u64 {
    match sorted.len() {
        0 => 0,
        1 => sorted[0],
        n => {
            let pos = p.clamp(0.0, 1.0) * (n - 1) as f64;
            let (lo, hi) = (pos.floor() as usize, pos.ceil() as usize);
            let frac = pos - lo as f64;
            (sorted[lo] as f64 + (sorted[hi] as f64 - sorted[lo] as f64) * frac).round() as u64
        }
    }
}

pub fn latency(values: &mut Vec<u64>) -> crate::types::LatencyStat {
    values.sort_unstable();
    let n = values.len() as u64;
    crate::types::LatencyStat {
        samples: n,
        median_ms: percentile(values, 0.5),
        p90_ms: percentile(values, 0.9),
        mean_ms: if n == 0 { 0 } else { values.iter().sum::<u64>() / n },
    }
}

pub fn record(provider: &str, model: &str, response: &Value, audio: f64) {
    let Some(app) = APP.get() else { return };
    let (input, output) = tokens(response);
    if let Err(e) = app.state::<AppState>().db.record_usage_cached(provider, model, input, output, cached_tokens(response), audio) {
        log::warn!("cannot record usage: {e:#}");
    }
    let _ = app.emit(events::STATS_UPDATED, ());
}

pub fn summary(db: &Db, days: u32) -> anyhow::Result<crate::types::UsageStats> {
    db.usage_stats(days)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn classifies_asr_vs_llm() {
        for p in ["MiMo", "Cloudflare", "Azure Speech", "Google Speech"] {
            assert_eq!(classify(p), Kind::Asr, "{p}");
        }
        for p in ["Gemini", "Anthropic", "OpenAI 兼容服务", "Cloudflare Workers AI"] {
            assert_eq!(classify(p), Kind::Llm, "{p}");
        }
    }

    #[test]
    fn percentiles() {
        assert_eq!(percentile(&[], 0.5), 0);
        assert_eq!(percentile(&[7], 0.9), 7);
        assert_eq!(percentile(&[1, 2, 3, 4], 0.5), 3); // 2.5 rounds up
        assert_eq!(percentile(&[10, 20, 30, 40, 50], 0.5), 30);
        assert_eq!(percentile(&[0, 100], 0.9), 90);
        let mut v = vec![300, 100, 200];
        let l = latency(&mut v);
        assert_eq!((l.samples, l.median_ms, l.mean_ms), (3, 200, 200));
    }

    #[test]
    fn cached_tokens_variants() {
        assert_eq!(cached_tokens(&json!({"usage":{"cache_read_input_tokens":5}})), Some(5));
        assert_eq!(cached_tokens(&json!({"usage":{"prompt_tokens_details":{"cached_tokens":8}}})), Some(8));
        assert_eq!(cached_tokens(&json!({"usageMetadata":{"cachedContentTokenCount":3}})), Some(3));
        assert_eq!(cached_tokens(&json!({})), None);
    }

    #[test]
    fn missing_is_not_zero_and_cached_input_is_counted() {
        assert_eq!(tokens(&json!({})), (None, None));
        assert_eq!(
            tokens(&json!({"usage":{"input_tokens":10,"cache_read_input_tokens":90,"output_tokens":0}})),
            (Some(100), Some(0))
        );
        assert_eq!(
            tokens(&json!({"usageMetadata":{"promptTokenCount":20,"candidatesTokenCount":30,"thoughtsTokenCount":5}})),
            (Some(20), Some(35))
        );
    }
}
