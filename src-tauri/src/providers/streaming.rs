//! Live 16 kHz PCM streams. Google uses its official Rust SDK; Azure uses the
//! Speech SDK's WebSocket framing because Microsoft does not ship a Rust Speech SDK.
use crate::{pipeline::join_transcripts, types::AsrConfig};
use anyhow::{bail, Context};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, Message};

pub type PcmSender = UnboundedSender<Vec<i16>>;
const SEGMENT_SAMPLES: usize = 240 * 16_000;

pub fn start(
    cfg: AsrConfig,
    languages: Vec<String>,
    words: Vec<String>,
) -> (PcmSender, tauri::async_runtime::JoinHandle<anyhow::Result<String>>) {
    // Share MiMo/reqwest's TLS provider; avoid enabling a second crypto backend for the SDK.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let task = tauri::async_runtime::spawn(async move {
        let mut parts = Vec::new();
        // Rotate before Google's five-minute limit. Incoming PCM remains queued during finalization.
        loop {
            let (text, ended) = match cfg.provider.as_str() {
                "google" => google(&cfg, &languages, &words, &mut rx).await?,
                "azure" => azure(&cfg, &languages, &words, &mut rx).await?,
                _ => bail!("该服务不支持流式识别"),
            };
            parts.push(text);
            if ended {
                return Ok(join_transcripts(&parts));
            }
        }
    });
    (tx, task)
}

struct RecognitionTask(tauri::async_runtime::JoinHandle<anyhow::Result<String>>);
impl Drop for RecognitionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub async fn transcribe_samples(
    cfg: AsrConfig,
    languages: Vec<String>,
    words: Vec<String>,
    samples: Vec<i16>,
) -> anyhow::Result<String> {
    let (tx, task) = start(cfg, languages, words);
    let mut task = RecognitionTask(task);
    // Speech streaming APIs require approximately real-time delivery, including history retries.
    for frame in samples.chunks(3200) {
        if tx.send(frame.to_vec()).is_err() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    drop(tx);
    (&mut task.0).await.context("流式识别任务异常")?
}

fn pcm_bytes(samples: &[i16]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

pub fn service_error(provider: &str, error: impl std::fmt::Display) -> anyhow::Error {
    let text = error.to_string().to_lowercase();
    let hint = if text.contains("401")
        || text.contains("403")
        || text.contains("unauthenticated")
        || text.contains("permission")
    {
        "鉴权失败，请检查密钥、服务账户权限与所属项目/区域"
    } else if text.contains("429") || text.contains("exhausted") || text.contains("quota") {
        "额度不足或请求过多，请检查配额后重试"
    } else if text.contains("invalid") || text.contains("400") || text.contains("404") {
        "配置不正确，请检查区域、语言、模型及项目 ID"
    } else {
        "连接失败或超时，请检查网络和服务配置后重试"
    };
    anyhow::anyhow!("{provider}：{hint}")
}

fn google_final_text(response: google_cloud_speech_v2::model::StreamingRecognizeResponse) -> Vec<String> {
    response
        .results
        .into_iter()
        .filter(|r| r.is_final)
        .filter_map(|r| r.alternatives.into_iter().next().map(|a| a.transcript))
        .collect()
}

async fn google(
    cfg: &AsrConfig,
    languages: &[String],
    words: &[String],
    rx: &mut UnboundedReceiver<Vec<i16>>,
) -> anyhow::Result<(String, bool)> {
    use google_cloud_speech_v2::{client::Speech, model::*};
    let c = &cfg.google;
    let credentials = if !c.service_account_json.trim().is_empty() {
        let key: Value = serde_json::from_str(&c.service_account_json).context("Google 服务账户 JSON 格式不正确")?;
        google_cloud_auth::credentials::service_account::Builder::new(key)
            .build()
            .map_err(|e| service_error("Google", e))?
    } else {
        google_cloud_auth::credentials::api_key_credentials::Builder::new(c.api_key.trim()).build()
    };
    if !c.location.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("Google 区域格式不正确");
    }
    let endpoint = if c.location == "global" {
        "https://speech.googleapis.com".into()
    } else {
        format!("https://{}-speech.googleapis.com", c.location)
    };
    let client = Speech::builder()
        .with_credentials(credentials)
        .with_endpoint(endpoint)
        .build()
        .await
        .map_err(|e| service_error("Google", e))?;
    let decoding = ExplicitDecodingConfig::new()
        .set_encoding(explicit_decoding_config::AudioEncoding::Linear16)
        .set_sample_rate_hertz(16000)
        .set_audio_channel_count(1);
    let mut config = RecognitionConfig::new()
        .set_model(&c.model)
        .set_language_codes(languages.iter().take(3).cloned())
        .set_explicit_decoding_config(decoding);
    if !words.is_empty() {
        let phrases = PhraseSet::new()
            .set_phrases(words.iter().take(100).map(|w| phrase_set::Phrase::new().set_value(w).set_boost(10.0_f32)));
        config = config.set_adaptation(
            SpeechAdaptation::new()
                .set_phrase_sets([speech_adaptation::AdaptationPhraseSet::new().set_inline_phrase_set(phrases)]),
        );
    }
    let (sender, mut responses) = client.streaming_recognize().build();
    sender
        .send(
            StreamingRecognizeRequest::new()
                .set_recognizer(format!("projects/{}/locations/{}/recognizers/_", c.project_id, c.location))
                .set_streaming_config(StreamingRecognitionConfig::new().set_config(config)),
        )
        .await
        .map_err(|e| service_error("Google", e))?;
    let send = async {
        let mut count = 0;
        let mut ended = false;
        while count < SEGMENT_SAMPLES {
            let Some(frame) = rx.recv().await else {
                ended = true;
                break;
            };
            count += frame.len();
            // Keep each gRPC audio message below the 25 KB limit.
            for block in frame.chunks(8000) {
                sender
                    .send(StreamingRecognizeRequest::new().set_audio(pcm_bytes(block)))
                    .await
                    .map_err(|e| service_error("Google", e))?;
            }
        }
        drop(sender);
        Ok::<_, anyhow::Error>((ended, count))
    };
    let receive = async {
        let mut text = Vec::new();
        while let Some(response) = responses.next().await {
            let response = response.map_err(|e| service_error("Google", e))?;
            text.extend(google_final_text(response));
        }
        Ok::<_, anyhow::Error>(join_transcripts(&text))
    };
    let ((ended, count), text) = tokio::time::timeout(Duration::from_secs(275), async {
        tokio::pin!(send, receive);
        tokio::select! {
            sent = &mut send => {
                let sent = sent?;
                let text = tokio::time::timeout(Duration::from_secs(30), &mut receive).await.context("等待最终识别结果超时")??;
                Ok::<_, anyhow::Error>((sent, text))
            }
            result = &mut receive => {
                result?;
                bail!("流式识别在音频发送结束前中断，请重试");
            }
        }
    }).await.context("Google 流式识别超时")??;
    crate::usage::record("Google Speech", &c.model, &json!({}), count as f64 / 16000.0);
    Ok((text, ended))
}

fn azure_text(path: &str, id: &str, body: Value) -> Message {
    Message::Text(
        format!(
            "Path:{path}\r\nX-RequestId:{id}\r\nX-Timestamp:{}\r\nContent-Type:application/json\r\n\r\n{body}",
            chrono::Utc::now().to_rfc3339()
        )
        .into(),
    )
}
fn azure_audio(id: &str, bytes: &[u8]) -> Message {
    let header = format!(
        "Path:audio\r\nX-RequestId:{id}\r\nX-Timestamp:{}\r\nContent-Type:audio/x-wav\r\n",
        chrono::Utc::now().to_rfc3339()
    );
    let mut packet = (header.len() as u16).to_be_bytes().to_vec();
    packet.extend(header.as_bytes());
    packet.extend(bytes);
    Message::Binary(packet.into())
}

pub fn azure_phrase(message: &str) -> anyhow::Result<Option<String>> {
    let Some((headers, body)) = message.split_once("\r\n\r\n") else { return Ok(None) };
    if !headers.lines().any(|l| {
        l.trim().eq_ignore_ascii_case("Path:speech.phrase") || l.trim().eq_ignore_ascii_case("Path: speech.phrase")
    }) {
        return Ok(None);
    }
    let v: Value = serde_json::from_str(body).context("Azure 返回了无法解析的识别结果")?;
    match v.get("RecognitionStatus").and_then(Value::as_str) {
        Some("Success") => Ok(v
            .get("DisplayText")
            .or_else(|| v.pointer("/NBest/0/Display"))
            .and_then(Value::as_str)
            .map(str::to_string)),
        Some("NoMatch" | "InitialSilenceTimeout" | "BabbleTimeout" | "EndOfDictation") => Ok(None),
        _ => bail!("Azure 语音识别失败，请检查语言和服务配置"),
    }
}

async fn azure(
    cfg: &AsrConfig,
    languages: &[String],
    words: &[String],
    rx: &mut UnboundedReceiver<Vec<i16>>,
) -> anyhow::Result<(String, bool)> {
    let c = &cfg.azure;
    if !c.region.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        bail!("Azure 区域格式不正确");
    }
    let language = languages.first().map(String::as_str).unwrap_or("zh-CN");
    let path = if languages.len() > 1 {
        "/stt/speech/universal/v2"
    } else {
        "/speech/recognition/conversation/cognitiveservices/v1"
    };
    let mut url = reqwest::Url::parse(&format!("wss://{}.stt.speech.microsoft.com{path}", c.region))?;
    if languages.len() <= 1 {
        url.query_pairs_mut().append_pair("language", language);
    }
    url.query_pairs_mut().append_pair("format", "simple");
    azure_at(cfg, languages, words, rx, url).await
}

async fn azure_at(
    cfg: &AsrConfig,
    languages: &[String],
    words: &[String],
    rx: &mut UnboundedReceiver<Vec<i16>>,
    url: reqwest::Url,
) -> anyhow::Result<(String, bool)> {
    let c = &cfg.azure;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let mut request = url.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Ocp-Apim-Subscription-Key", c.api_key.trim().parse().context("Azure Key 格式不正确")?);
    request.headers_mut().insert("X-ConnectionId", id.parse()?);
    let (mut socket, _) = tokio::time::timeout(Duration::from_secs(15), tokio_tungstenite::connect_async(request))
        .await
        .context("Azure 连接超时")?
        .map_err(|e| service_error("Azure", e))?;
    socket.send(azure_text("speech.config", &id, json!({"context":{"system":{"name":"Hoshiro","version":env!("CARGO_PKG_VERSION")},"os":{"platform":std::env::consts::OS}}}))).await?;
    let mut context = json!({"phraseDetection":{"mode":"Conversation"},"dgi":{"groups":[{"type":"Generic","items":words.iter().take(100).map(|w| json!({"text":w})).collect::<Vec<_>>()}]}});
    if languages.len() > 1 {
        context["languageId"] = json!({"languages":languages.iter().take(4).collect::<Vec<_>>(),"mode":"DetectAtAudioStart","onSuccess":{"action":"Recognize"},"onUnknown":{"action":"None"},"priority":"PrioritizeLatency"});
    }
    socket.send(azure_text("speech.context", &id, context.clone())).await?;
    socket.send(azure_audio(&id, &crate::audio::wav::encode_wav(&[], 16000))).await?;
    let mut id = id;
    let mut audio = Vec::new();
    let mut parts = Vec::new();
    let mut ended = false;
    let mut sending = true;
    let mut turn_base = 0usize;
    let mut confirmed = 0usize;
    let mut deadline = tokio::time::Instant::now() + Duration::from_secs(275);
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => bail!("Azure 流式识别超时"),
            frame = rx.recv(), if sending => {
                if let Some(frame) = frame {
                    socket.send(azure_audio(&id, &pcm_bytes(&frame))).await.map_err(|e| service_error("Azure", e))?;
                    audio.extend(frame);
                } else { ended = true; }
                if ended || audio.len() >= SEGMENT_SAMPLES {
                    sending = false;
                    socket.send(azure_audio(&id, &[])).await.map_err(|e| service_error("Azure", e))?;
                    deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                }
            }
            message = socket.next() => {
                let message = message.context("Azure 流式识别被中断")?.map_err(|e| service_error("Azure", e))?;
                match message {
                    Message::Text(text) => {
                        if let Some(phrase) = azure_phrase(&text)? { parts.push(phrase); }
                        let (headers, body) = text.split_once("\r\n\r\n").unwrap_or((&text, ""));
                        let path = headers.lines().find_map(|line| line.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("path")).map(|(_,v)| v.trim())).unwrap_or("");
                        if path.eq_ignore_ascii_case("speech.phrase") {
                            let v: Value = serde_json::from_str(body)?;
                            if let (Some(offset), Some(duration)) = (v["Offset"].as_u64(), v["Duration"].as_u64()) {
                                // Azure offsets are 100 ns ticks, relative to this request's audio.
                                confirmed = confirmed.max((turn_base + ((offset + duration) * 16000 / 10_000_000) as usize).min(audio.len()));
                            }
                        }
                        if path.eq_ignore_ascii_case("turn.end") {
                            if !sending {
                                crate::usage::record("Azure Speech", "streaming", &json!({}), audio.len() as f64 / 16000.0);
                                return Ok((join_transcripts(&parts), ended));
                            }
                            // Continuous recognition may end a service turn while recording continues.
                            // Replay only unacknowledged audio, so words spoken during the turn transition survive.
                            if confirmed == turn_base { bail!("Azure 未确认当前音频，请重试"); }
                            turn_base = confirmed;
                            id = uuid::Uuid::new_v4().simple().to_string();
                            socket.send(azure_text("speech.context", &id, context.clone())).await?;
                            socket.send(azure_audio(&id, &crate::audio::wav::encode_wav(&[], 16000))).await?;
                            for frame in audio[confirmed..].chunks(3200) {
                                socket.send(azure_audio(&id, &pcm_bytes(frame))).await?;
                            }
                        }
                    }
                    Message::Close(_) => bail!("Azure 连接在返回完整结果前关闭"),
                    Message::Ping(_) => { socket.flush().await?; }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn google_mock_responses_keep_only_final_best_alternatives() {
        use google_cloud_speech_v2::model::*;
        let response = StreamingRecognizeResponse::new().set_results([
            StreamingRecognitionResult::new()
                .set_is_final(false)
                .set_alternatives([SpeechRecognitionAlternative::new().set_transcript("discard partial")]),
            StreamingRecognitionResult::new().set_is_final(true).set_alternatives([
                SpeechRecognitionAlternative::new().set_transcript("hello"),
                SpeechRecognitionAlternative::new().set_transcript("alternative"),
            ]),
            StreamingRecognitionResult::new()
                .set_is_final(true)
                .set_alternatives([SpeechRecognitionAlternative::new().set_transcript("world")]),
        ]);
        assert_eq!(join_transcripts(&google_final_text(response)), "hello world");
    }

    #[tokio::test]
    async fn azure_mock_stream_sends_pcm_and_half_closes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = reqwest::Url::parse(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut audio_packets = 0;
            loop {
                let message = ws.next().await.unwrap().unwrap();
                if let Message::Binary(bytes) = message {
                    let len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
                    if bytes.len() == len + 2 {
                        break;
                    }
                    audio_packets += 1;
                }
            }
            assert_eq!(audio_packets, 2); // WAV header, then PCM
            ws.send(Message::Text(
                "Path:speech.phrase\r\n\r\n{\"RecognitionStatus\":\"Success\",\"DisplayText\":\"Hello\"}".into(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text("Path:turn.end\r\n\r\n{}".into())).await.unwrap();
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(vec![1, 2, 3]).unwrap();
        drop(tx);
        let mut cfg = AsrConfig::default();
        cfg.azure.api_key = "test-key".into();
        let result = azure_at(&cfg, &["en-US".into()], &[], &mut rx, url).await.unwrap();
        assert_eq!(result, ("Hello".into(), true));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn azure_continues_after_a_service_turn() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = reqwest::Url::parse(&format!("ws://{}", listener.local_addr().unwrap())).unwrap();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();
            let mut packets = 0;
            while packets < 2 {
                if matches!(ws.next().await.unwrap().unwrap(), Message::Binary(_)) {
                    packets += 1;
                }
            }
            ws.send(Message::Text("Path:speech.phrase\r\n\r\n{\"RecognitionStatus\":\"Success\",\"DisplayText\":\"Hello\",\"Offset\":0,\"Duration\":1875}".into())).await.unwrap();
            ws.send(Message::Text("Path:turn.end\r\n\r\n{}".into())).await.unwrap();
            // Wait for the next turn's WAV header, then let the recording continue.
            loop {
                if matches!(ws.next().await.unwrap().unwrap(), Message::Binary(_)) {
                    break;
                }
            }
            ready_tx.send(()).unwrap();
            let mut samples = Vec::new();
            loop {
                if let Message::Binary(bytes) = ws.next().await.unwrap().unwrap() {
                    let n = u16::from_be_bytes([bytes[0], bytes[1]]) as usize + 2;
                    if n == bytes.len() {
                        break;
                    }
                    samples.extend_from_slice(&bytes[n..]);
                }
            }
            assert_eq!(samples, pcm_bytes(&[4, 5, 6]));
            ws.send(Message::Text(
                "Path:speech.phrase\r\n\r\n{\"RecognitionStatus\":\"Success\",\"DisplayText\":\"world\"}".into(),
            ))
            .await
            .unwrap();
            ws.send(Message::Text("Path:turn.end\r\n\r\n{}".into())).await.unwrap();
        });
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        tx.send(vec![1, 2, 3]).unwrap();
        let producer = tokio::spawn(async move {
            ready_rx.await.unwrap();
            tx.send(vec![4, 5, 6]).unwrap();
        });
        let mut cfg = AsrConfig::default();
        cfg.azure.api_key = "test-key".into();
        let result = azure_at(&cfg, &["en-US".into()], &[], &mut rx, url).await.unwrap();
        assert_eq!(result, ("Hello world".into(), true));
        server.await.unwrap();
        producer.await.unwrap();
    }

    #[test]
    fn azure_frames_and_final_only() {
        let Message::Binary(packet) = azure_audio("abc", &[1, 2]) else { panic!() };
        let size = u16::from_be_bytes([packet[0], packet[1]]) as usize;
        assert_eq!(&packet[size + 2..], &[1, 2]);
        assert_eq!(
            azure_phrase("Path:speech.phrase\r\n\r\n{\"RecognitionStatus\":\"Success\",\"DisplayText\":\"你好\"}")
                .unwrap(),
            Some("你好".into())
        );
        assert!(azure_phrase("Path:speech.hypothesis\r\n\r\n{\"Text\":\"partial\"}").unwrap().is_none());
        assert!(azure_phrase("Path:speech.phrase\r\n\r\n{}").is_err());
    }
    #[test]
    fn errors_are_localized_without_credentials() {
        assert!(service_error("Google", "RESOURCE_EXHAUSTED").to_string().contains("额度"));
        assert!(service_error("Azure", "401 key=secret").to_string().contains("鉴权"));
        assert!(!service_error("Azure", "401 key=secret").to_string().contains("secret"));
    }
}
