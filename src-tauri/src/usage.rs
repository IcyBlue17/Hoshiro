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

pub fn record(provider: &str, model: &str, response: &Value, audio: f64) {
    let Some(app) = APP.get() else { return };
    let (input, output) = tokens(response);
    if let Err(e) = app.state::<AppState>().db.record_usage(provider, model, input, output, audio) {
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
