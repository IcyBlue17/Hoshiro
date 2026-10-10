//! Records 5 s from the default microphone, prints levels, writes /tmp/hoshiro_probe.wav and, when API keys are
//! present in the environment, runs MiMo ASR and the LLM rewrite on it.
//!
//!   MIMO_API_KEY=... GEMINI_API_KEY=... cargo run --example record_probe
//! Optional: MIMO_BASE_URL, MIMO_LANG (zh|en|auto), MIC_DEVICE (device name), GEMINI_MODEL / ANTHROPIC_MODEL / OPENAI_MODEL,
//! OPENAI_BASE_URL.

use hoshiro_lib::audio::recorder::{AudioChunk, Recorder, RecorderSink};
use hoshiro_lib::audio::wav;
use hoshiro_lib::platform::AppContext;
use hoshiro_lib::providers::{self, RewriteRequest};
use hoshiro_lib::types::{LlmConfig, LlmProvider, MimoAsrConfig, Mode};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::{Duration, Instant};

struct Sink {
    last_print: Mutex<Instant>,
}

impl RecorderSink for Sink {
    fn on_level(&self, level: f32) {
        let mut last = self.last_print.lock();
        if last.elapsed() >= Duration::from_millis(100) {
            *last = Instant::now();
            let n = (level * 40.0).round() as usize;
            println!("{:5.2} |{}{}|", level, "#".repeat(n), " ".repeat(40 - n.min(40)));
        }
    }
    fn on_chunk(&self, chunk: AudioChunk) {
        println!("chunk #{} ({} samples)", chunk.index, chunk.samples.len());
    }
    fn on_error(&self, message: &str) {
        eprintln!("input error: {message}");
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|s| !s.trim().is_empty())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    println!("Devices:");
    for d in Recorder::list_devices() {
        println!("  {} {} [{}] id={:?}", if d.is_default { "*" } else { " " }, d.name, d.kind, d.id);
    }

    let rec = Recorder::new();
    let t0 = Instant::now();
    rec.start(env("MIC_DEVICE"), Arc::new(Sink { last_print: Mutex::new(Instant::now()) }))?;
    println!("recording started in {} ms; speak for 5 s...", t0.elapsed().as_millis());
    tokio::time::sleep(Duration::from_secs(5)).await;
    let res = rec.stop()?;
    println!(
        "stopped: {:.2}s, {} samples, silent={}, tail chunk #{} ({} samples)",
        res.duration_secs,
        res.all_samples.len(),
        res.is_silent,
        res.tail.index,
        res.tail.samples.len()
    );
    let bytes = wav::encode_wav(&res.all_samples, 16_000);
    std::fs::write("/tmp/hoshiro_probe.wav", &bytes)?;
    println!("wrote /tmp/hoshiro_probe.wav ({} bytes)", bytes.len());

    let Some(key) = env("MIMO_API_KEY") else {
        println!("MIMO_API_KEY not set; skipping ASR.");
        return Ok(());
    };
    let mut cfg = MimoAsrConfig { api_key: key, ..Default::default() };
    if let Some(b) = env("MIMO_BASE_URL") {
        cfg.base_url = b;
    }
    let lang = env("MIMO_LANG").unwrap_or_else(|| "auto".into());
    let t = Instant::now();
    let transcript = providers::transcribe(&cfg, bytes, &lang).await?;
    println!("ASR ({} ms): {transcript:?}", t.elapsed().as_millis());

    let mut llm = LlmConfig::default();
    if let Some(k) = env("GEMINI_API_KEY") {
        llm.provider = LlmProvider::Gemini;
        llm.gemini.api_key = k;
        if let Some(m) = env("GEMINI_MODEL") {
            llm.gemini.model = m;
        }
    } else if let Some(k) = env("ANTHROPIC_API_KEY") {
        llm.provider = LlmProvider::Anthropic;
        llm.anthropic.api_key = k;
        if let Some(m) = env("ANTHROPIC_MODEL") {
            llm.anthropic.model = m;
        }
    } else if let Some(k) = env("OPENAI_API_KEY") {
        llm.provider = LlmProvider::Openai;
        llm.openai.api_key = k;
        if let Some(m) = env("OPENAI_MODEL") {
            llm.openai.model = m;
        }
        if let Some(b) = env("OPENAI_BASE_URL") {
            llm.openai.base_url = b;
        }
    } else {
        println!("No LLM key set (GEMINI_API_KEY / ANTHROPIC_API_KEY / OPENAI_API_KEY); skipping rewrite.");
        return Ok(());
    }
    let req = RewriteRequest {
        mode: Mode::Dictation,
        transcript,
        selected_text: None,
        context: AppContext { app_name: Some("Notes".into()), ..Default::default() },
        dictionary: vec![],
        languages: vec!["zh-CN".into(), "en-US".into()],
        translation_target: None,
        custom_instructions: String::new(),
        smart_formatting: true,
        long_lists: false,
        profile: None,
        duration_secs: 0.0,
    };
    let t = Instant::now();
    let out = providers::rewrite(&llm, &req).await?;
    println!("Rewrite via {:?} ({} ms): {out:?}", llm.provider, t.elapsed().as_millis());
    Ok(())
}
