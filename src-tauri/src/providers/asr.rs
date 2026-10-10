//! Batch recognizers share the existing pause-based chunk pipeline.
use super::{
    http::{network_failure, status_failure},
    mimo, streaming,
};
use crate::{
    audio::{wav, TARGET_SAMPLE_RATE},
    types::{AsrConfig, TestResult},
};
use anyhow::bail;
use base64::Engine;
use serde_json::{json, Value};
use std::{
    future::Future,
    time::Instant,
};

pub trait BatchRecognizer {
    fn recognize(
        &self,
        wav: Vec<u8>,
        language: &str,
        words: &[String],
    ) -> impl Future<Output = anyhow::Result<String>> + Send;
}
impl BatchRecognizer for AsrConfig {
    async fn recognize(&self, wav: Vec<u8>, language: &str, words: &[String]) -> anyhow::Result<String> {
        match self.provider.as_str() {
            "mimo" => mimo::transcribe(&self.mimo, wav, language).await,
            "cloudflare" => cloudflare(self, wav, language, words).await,
            _ => bail!("该服务需要流式音频"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CfFormat {
    Turbo,
    Whisper,
    Nova3,
}
impl CfFormat {
    pub fn of(model: &str) -> Self {
        if model.starts_with("@cf/deepgram/") {
            Self::Nova3
        } else if model == "@cf/openai/whisper" || model.starts_with("@cf/openai/whisper-tiny") {
            Self::Whisper
        } else {
            Self::Turbo
        }
    }
}

/// One Workers AI request: JSON (Turbo) or raw audio with query parameters (Whisper, Nova-3).
#[derive(Debug)]
pub struct CfRequest {
    pub content_type: &'static str,
    pub query: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

pub fn cloudflare_body(wav: &[u8], language: &str, words: &[String]) -> Value {
    let mut body = json!({"audio": base64::engine::general_purpose::STANDARD.encode(wav),
        "initial_prompt": words.iter().take(100).cloned().collect::<Vec<_>>().join(", ")});
    if language != "auto" {
        body["language"] = json!(language);
    }
    body
}

pub fn cloudflare_request(model: &str, wav: &[u8], language: &str, words: &[String]) -> CfRequest {
    match CfFormat::of(model) {
        CfFormat::Turbo => CfRequest {
            content_type: "application/json",
            query: vec![],
            body: cloudflare_body(wav, language, words).to_string().into_bytes(),
        },
        CfFormat::Whisper => CfRequest { content_type: "application/octet-stream", query: vec![], body: wav.to_vec() },
        CfFormat::Nova3 => {
            let mut query = vec![("punctuate", "true".to_string()), ("smart_format", "true".to_string())];
            if language == "auto" {
                query.push(("detect_language", "true".into()));
            } else {
                query.push(("language", language.to_string()));
            }
            for w in words.iter().filter(|w| !w.trim().is_empty()).take(50) {
                query.push(("keyterm", w.trim().to_string()));
            }
            CfRequest { content_type: "audio/wav", query, body: wav.to_vec() }
        }
    }
}

/// Extracts the transcript from the (already unwrapped) `result` object of any supported model.
pub fn cloudflare_text(result: &Value) -> Option<String> {
    let text = result
        .get("text")
        .or_else(|| result.pointer("/transcription_info/text"))
        .or_else(|| result.pointer("/results/channels/0/alternatives/0/transcript"))
        .and_then(Value::as_str)?;
    Some(text.trim().to_string())
}

async fn cloudflare(cfg: &AsrConfig, wav: Vec<u8>, language: &str, words: &[String]) -> anyhow::Result<String> {
    if !cfg.ready() {
        bail!("请填写 Cloudflare 账户 ID 和 API Token");
    }
    let c = &cfg.cloudflare;
    if !c.account_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        bail!("Cloudflare 账户 ID 格式不正确");
    }
    if !c.model.starts_with("@cf/") || c.model.contains("..") || c.model.contains(['?', '#']) {
        bail!("Cloudflare 模型名称不正确");
    }
    if CfFormat::of(&c.model) == CfFormat::Nova3 && language.to_lowercase().starts_with("zh") {
        bail!("Nova-3 不支持中文，请改用 Whisper 模型");
    }
    cloudflare_at("https://api.cloudflare.com/client/v4", cfg, wav, language, words).await
}

async fn cloudflare_at(
    base: &str,
    cfg: &AsrConfig,
    wav: Vec<u8>,
    language: &str,
    words: &[String],
) -> anyhow::Result<String> {
    let c = &cfg.cloudflare;
    let req = cloudflare_request(&c.model, &wav, language, words);
    let url = format!("{}/accounts/{}/ai/run/{}", base.trim_end_matches('/'), c.account_id, c.model);
    let response = super::http::CLIENT
        .post(url)
        .bearer_auth(c.api_key.trim())
        .header(reqwest::header::CONTENT_TYPE, req.content_type)
        .query(&req.query)
        .body(req.body)
        .send()
        .await
        .map_err(|e| network_failure("Cloudflare", &e))?;
    let status = response.status();
    let text = response.text().await.map_err(|e| network_failure("Cloudflare", &e))?;
    if !status.is_success() {
        return Err(status_failure("Cloudflare", status.as_u16(), "请检查账户、模型与权限").into());
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("Cloudflare 响应格式不正确"))?;
    if value.get("success").and_then(Value::as_bool) == Some(false)
        || value.get("errors").and_then(Value::as_array).is_some_and(|e| !e.is_empty())
    {
        bail!("Cloudflare 识别失败，请检查账户权限与额度");
    }
    let out = cloudflare_text(value.get("result").unwrap_or(&value))
        .ok_or_else(|| anyhow::anyhow!("Cloudflare 响应格式不正确"))?;
    crate::usage::record("Cloudflare", &c.model, &json!({}), wav.len().saturating_sub(44) as f64 / 32000.0);
    Ok(out)
}

pub async fn test(cfg: &AsrConfig, languages: &[String], words: &[String]) -> TestResult {
    let start = Instant::now();
    let r = if cfg.streaming() {
        streaming::transcribe_samples(cfg.clone(), languages.to_vec(), words.to_vec(), vec![0; 8000]).await
    } else {
        cfg.recognize(
            wav::encode_wav(&vec![0; 8000], TARGET_SAMPLE_RATE),
            mimo::language_param(languages, false),
            words,
        )
        .await
    };
    TestResult {
        ok: r.is_ok(),
        message: r.map(|_| "连接成功".into()).unwrap_or_else(|e| format!("{e:#}")),
        latency_ms: start.elapsed().as_millis() as u64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn cloudflare_http_response_and_auth_errors() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        for (status, body, success) in [
            ("200 OK", r#"{"success":true,"result":{"text":"Hoshiro"}}"#, true),
            ("401 Unauthorized", r#"{"success":false}"#, false),
        ] {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/run", server.local_addr().unwrap());
            let task = tokio::spawn(async move {
                let (mut socket, _) = server.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 8192];
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    let request = String::from_utf8_lossy(&bytes);
                    if let Some((headers, body)) = request.split_once("\r\n\r\n") {
                        let length: usize = headers
                            .lines()
                            .find_map(|line| {
                                line.to_lowercase().strip_prefix("content-length:").map(|n| n.trim().parse().unwrap())
                            })
                            .unwrap();
                        if body.len() >= length {
                            break;
                        }
                    }
                }
                let request = String::from_utf8_lossy(&bytes);
                assert!(request.contains("Bearer test-token"));
                let payload: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(payload["initial_prompt"], "Hoshiro");
                assert_eq!(payload["language"], "zh");
                assert!(payload["audio"].is_string());
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            });
            let mut cfg = AsrConfig::default();
            cfg.provider = "cloudflare".into();
            cfg.cloudflare.api_key = "test-token".into();
            let result = cloudflare_at(&url, &cfg, wav::encode_wav(&[0; 160], 16000), "zh", &["Hoshiro".into()]).await;
            assert_eq!(result.is_ok(), success);
            if success {
                assert_eq!(result.unwrap(), "Hoshiro");
            }
            task.await.unwrap();
        }
    }

    #[test]
    fn cloudflare_contract() {
        let body = cloudflare_body(&[1, 2, 3], "auto", &["Hoshiro".into()]);
        assert_eq!(body["audio"], "AQID");
        assert!(body.get("language").is_none());
        assert_eq!(body["initial_prompt"], "Hoshiro");
    }
}

#[cfg(test)]
mod cf_tests {
    use super::*;
    #[test]
    fn model_formats() {
        assert_eq!(CfFormat::of("@cf/openai/whisper-large-v3-turbo"), CfFormat::Turbo);
        assert_eq!(CfFormat::of("@cf/openai/whisper"), CfFormat::Whisper);
        assert_eq!(CfFormat::of("@cf/deepgram/nova-3"), CfFormat::Nova3);
        assert_eq!(CfFormat::of("@cf/foo/bar"), CfFormat::Turbo);
    }
    #[test]
    fn request_bodies() {
        let r = cloudflare_request("@cf/openai/whisper", &[1, 2], "zh", &[]);
        assert_eq!((r.content_type, r.body.as_slice(), r.query.is_empty()), ("application/octet-stream", &[1u8, 2][..], true));
        let r = cloudflare_request("@cf/deepgram/nova-3", &[9], "zh", &["Hoshiro".into(), " ".into()]);
        assert_eq!(r.content_type, "audio/wav");
        assert!(r.query.contains(&("language", "zh".into())) && r.query.contains(&("keyterm", "Hoshiro".into())));
        assert_eq!(r.query.iter().filter(|(k, _)| *k == "keyterm").count(), 1);
        let r = cloudflare_request("@cf/deepgram/nova-3", &[9], "auto", &[]);
        assert!(r.query.contains(&("detect_language", "true".into())));
        let r = cloudflare_request("@cf/openai/whisper-large-v3-turbo", &[1, 2, 3], "zh", &[]);
        assert_eq!(r.content_type, "application/json");
        assert!(String::from_utf8(r.body).unwrap().contains("AQID"));
    }
    #[test]
    fn response_parsing() {
        assert_eq!(cloudflare_text(&json!({"text":" 你好 "})).unwrap(), "你好");
        assert_eq!(cloudflare_text(&json!({"transcription_info":{"text":"a"}})).unwrap(), "a");
        let nova = json!({"results":{"channels":[{"alternatives":[{"transcript":"hi there"}]}]}});
        assert_eq!(cloudflare_text(&nova).unwrap(), "hi there");
        assert!(cloudflare_text(&json!({})).is_none());
    }
}
