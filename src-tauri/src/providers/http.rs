//! Shared HTTP client and error mapping (user-facing messages are Chinese).

use once_cell::sync::Lazy;
use std::time::Duration;

pub static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .pool_idle_timeout(Duration::from_secs(60))
        .tcp_nodelay(true)
        .user_agent(concat!("Hoshiro/", env!("CARGO_PKG_VERSION")))
        .build()
        .unwrap_or_else(|e| {
            log::error!("falling back to default reqwest client: {e}");
            reqwest::Client::new()
        })
});

#[derive(Debug)]
pub struct Failure {
    pub message: String,
    pub retryable: bool,
    pub status: Option<u16>,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for Failure {}

pub fn network_failure(provider: &str, e: &reqwest::Error) -> Failure {
    let message = if e.is_timeout() {
        format!("{provider} 请求超时")
    } else if e.is_connect() {
        "网络连接失败".to_string()
    } else if e.is_decode() || e.is_body() {
        format!("{provider} 响应读取失败")
    } else {
        "网络连接失败".to_string()
    };
    log::warn!("{provider} network error: {e}");
    Failure { message, retryable: true, status: None }
}

/// Pull `error.message` (OpenAI/Anthropic/Gemini style) or `message` out of an error body.
pub fn api_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let msg = v
        .pointer("/error/message")
        .or_else(|| v.get("message"))
        .or_else(|| v.pointer("/error"))
        .and_then(|m| m.as_str())?;
    let msg = msg.trim();
    (!msg.is_empty()).then(|| msg.chars().take(200).collect())
}

pub fn status_failure(provider: &str, status: u16, body: &str) -> Failure {
    let detail = api_message(body);
    let lower = body.to_lowercase();
    let (message, retryable) = match status {
        401 => (format!("{provider} API Key 无效"), false),
        403 => (format!("{provider} 拒绝访问（API Key 无权限或地区受限）"), false),
        404 => (format!("{provider} 接口或模型不存在，请检查 Base URL 和模型名称"), false),
        402 => (format!("{provider} 账户余额不足"), false),
        408 => (format!("{provider} 请求超时"), true),
        413 => ("音频过长，请缩短录音".to_string(), false),
        429 => {
            if lower.contains("quota") || lower.contains("balance") || lower.contains("insufficient") {
                (format!("{provider} 额度已用尽"), false)
            } else {
                (format!("{provider} 请求过于频繁，请稍后再试"), true)
            }
        }
        500..=599 => (format!("{provider} 服务暂时不可用（HTTP {status}）"), true),
        _ => match detail {
            Some(d) => (format!("{provider} 请求被拒绝：{d}"), false),
            None => (format!("{provider} 请求失败（HTTP {status}）"), false),
        },
    };
    log::warn!("{provider} HTTP {status}: {}", body.chars().take(300).collect::<String>());
    Failure { message, retryable, status: Some(status) }
}

pub fn trim_base(url: &str) -> &str {
    url.trim().trim_end_matches('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_status_codes() {
        assert_eq!(status_failure("MiMo", 401, "").message, "MiMo API Key 无效");
        assert!(status_failure("MiMo", 503, "").retryable);
        assert!(!status_failure("MiMo", 400, r#"{"error":{"message":"bad audio"}}"#).retryable);
        assert!(status_failure("MiMo", 400, r#"{"error":{"message":"bad audio"}}"#).message.contains("bad audio"));
        assert!(status_failure("X", 429, "rate limited").retryable);
        assert!(!status_failure("X", 429, "insufficient_quota").retryable);
    }
}

#[cfg(test)]
pub(crate) mod testserver {
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Serves the given (status, body) responses in order on a random local port; returns (base_url, recorded requests).
    pub async fn serve(responses: Vec<(u16, &'static str)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        tokio::spawn(async move {
            for (status, body) in responses {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 8192];
                loop {
                    let n = sock.read(&mut tmp).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    let text = String::from_utf8_lossy(&buf).to_string();
                    if let Some(idx) = text.find("\r\n\r\n") {
                        let len = text[..idx]
                            .lines()
                            .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                            .unwrap_or(0);
                        if buf.len() >= idx + 4 + len {
                            break;
                        }
                    }
                }
                seen2.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (format!("http://{addr}"), seen)
    }
}
