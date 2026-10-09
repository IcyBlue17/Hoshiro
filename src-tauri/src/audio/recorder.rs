//! Microphone recorder (cpal). OWNER: audio/providers agent.
//!
//! - Captures from the selected device (None = system default), downmixes to mono, resamples to 16 kHz i16.
//! - Reports a smoothed loudness level (0..1, perceptual, suitable for waveform bars) ~30x/second.
//! - Long recordings are split into chunks at natural pauses so they can be transcribed while the user
//!   is still talking (MiMo has a 10 MB base64 limit per request): once the current chunk is >= 25 s,
//!   cut at the next pause (>= 350 ms below the silence threshold); force a cut at 90 s.
//! - All methods are callable from any thread. `start` must return quickly (< 100 ms) — the stream
//!   should already be delivering audio when it returns so the first syllable is not lost.
//!
//! Threading: `start`/`stop` block briefly (stream build / join, typically < 100 ms). The cpal stream lives on
//! its own thread; all DSP and every `RecorderSink` callback run on a separate worker thread.

use super::resample::{f32_to_i16, Resampler};
use super::TARGET_SAMPLE_RATE;
use crate::types::MicDevice;
use anyhow::{anyhow, Context};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct AudioChunk {
    /// 0-based, in order.
    pub index: usize,
    /// 16 kHz mono PCM.
    pub samples: Vec<i16>,
}

pub struct StopResult {
    /// The final (not yet emitted) chunk. May be empty.
    pub tail: AudioChunk,
    /// The complete recording (all chunks concatenated), 16 kHz mono — used for history audio.
    pub all_samples: Vec<i16>,
    pub duration_secs: f64,
    /// True if the whole recording is effectively silent (no speech detected).
    pub is_silent: bool,
}

pub trait RecorderSink: Send + Sync {
    fn on_level(&self, level: f32);
    fn on_chunk(&self, chunk: AudioChunk);
    /// Live PCM before pause-based chunking; default keeps batch recognizers unchanged.
    fn on_pcm(&self, _samples: &[i16]) {}
    /// The input device failed mid-recording (unplugged, etc.). Audio captured so far is still
    /// returned by `stop()`. Called from the worker thread.
    fn on_error(&self, _message: &str) {}
}

// ───────────────────────── Device listing ─────────────────────────

pub fn classify_device(name: &str) -> &'static str {
    let n = name.to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|k| n.contains(k));
    if has(&["blackhole", "loopback", "virtual", "teams", "zoom", "soundflower", "aggregate", "multi-output", "obs"]) {
        "virtual"
    } else if has(&["airpods", "bluetooth", "buds", "beats", "wh-1000", "wf-1000", "powerbeats"]) {
        "bluetooth"
    } else if has(&["macbook", "built-in", "builtin", "内建", "内置", "imac", "mac mini", "mac studio", "studio display"]) {
        "builtin"
    } else if has(&["usb"]) {
        "usb"
    } else {
        "unknown"
    }
}

fn find_input_device(host: &cpal::Host, id: &str) -> Option<cpal::Device> {
    let wanted = id.trim();
    let devices = host.devices().ok()?;
    let mut fallback = None;
    for d in devices {
        if let Ok(name) = d.name() {
            if name == wanted {
                return Some(d);
            }
            if fallback.is_none() && name.trim().eq_ignore_ascii_case(wanted) {
                fallback = Some(d);
            }
        }
    }
    fallback
}

// ───────────────────────── Signal processing ─────────────────────────

const FRAME: usize = 512; // 32 ms @ 16 kHz
const FRAME_MS: usize = 32;
const LEVEL_DB_MIN: f32 = -55.0;
const LEVEL_DB_MAX: f32 = -12.0;
const MIN_SPEECH_MS: usize = 250;
const NOISE_WINDOW_FRAMES: usize = 96;
const VAD_MARGIN_DB: f32 = 9.0;
const VAD_MIN_DB: f32 = -54.0;
const VAD_MAX_DB: f32 = -38.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ChunkConfig {
    pub min_chunk_samples: usize,
    pub pause_frames: usize,
    pub max_chunk_samples: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        let sr = TARGET_SAMPLE_RATE as usize;
        Self { min_chunk_samples: 25 * sr, pause_frames: 350usize.div_ceil(FRAME_MS), max_chunk_samples: 90 * sr }
    }
}

pub(crate) struct Processor {
    sink: Arc<dyn RecorderSink>,
    resampler: Resampler,
    cfg: ChunkConfig,
    scratch: Vec<f32>,
    frame_buf: Vec<i16>,
    cur: Vec<i16>,
    all: Vec<i16>,
    next_index: usize,
    cur_speech_frames: usize,
    total_speech_frames: usize,
    silence_run: usize,
    recent_db: VecDeque<f32>,
    level: f32,
}

fn frame_db(frame: &[i16]) -> f32 {
    if frame.is_empty() {
        return -100.0;
    }
    let sum: f64 = frame.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
    let rms = (sum / frame.len() as f64).sqrt();
    (20.0 * (rms + 1e-7).log10()) as f32
}

impl Processor {
    pub fn new(in_rate: u32, sink: Arc<dyn RecorderSink>, cfg: ChunkConfig) -> Self {
        let mut recent_db = VecDeque::with_capacity(NOISE_WINDOW_FRAMES + 1);
        recent_db.extend(std::iter::repeat(-60.0).take(NOISE_WINDOW_FRAMES));
        Self {
            sink,
            resampler: Resampler::new(in_rate, TARGET_SAMPLE_RATE),
            cfg,
            scratch: Vec::new(),
            frame_buf: Vec::with_capacity(FRAME * 2),
            cur: Vec::new(),
            all: Vec::new(),
            next_index: 0,
            cur_speech_frames: 0,
            total_speech_frames: 0,
            silence_run: 0,
            recent_db,
            level: 0.0,
        }
    }

    /// Feed mono f32 samples at the device rate.
    pub fn push(&mut self, input: &[f32]) {
        self.scratch.clear();
        let mut scratch = std::mem::take(&mut self.scratch);
        self.resampler.process(input, &mut scratch);
        self.push_16k(&scratch);
        self.scratch = scratch;
    }

    fn push_16k(&mut self, samples: &[f32]) {
        let pcm: Vec<i16> = samples.iter().map(|&s| f32_to_i16(s)).collect();
        self.sink.on_pcm(&pcm);
        self.frame_buf.extend(pcm);
        while self.frame_buf.len() >= FRAME {
            let frame: Vec<i16> = self.frame_buf.drain(..FRAME).collect();
            self.analyze_frame(frame);
        }
    }

    fn analyze_frame(&mut self, frame: Vec<i16>) {
        let db = frame_db(&frame);

        self.recent_db.push_back(db);
        if self.recent_db.len() > NOISE_WINDOW_FRAMES {
            self.recent_db.pop_front();
        }
        let floor = self.recent_db.iter().copied().fold(f32::MAX, f32::min);
        let threshold = (floor + VAD_MARGIN_DB).clamp(VAD_MIN_DB, VAD_MAX_DB);
        let is_speech = db > threshold;

        if is_speech {
            self.cur_speech_frames += 1;
            self.total_speech_frames += 1;
            self.silence_run = 0;
        } else {
            self.silence_run += 1;
        }

        let norm = ((db - LEVEL_DB_MIN) / (LEVEL_DB_MAX - LEVEL_DB_MIN)).clamp(0.0, 1.0);
        let coeff = if norm > self.level { 0.65 } else { 0.18 };
        self.level += (norm - self.level) * coeff;
        self.sink.on_level(self.level);

        self.all.extend_from_slice(&frame);
        self.cur.extend_from_slice(&frame);

        let at_pause = self.cur.len() >= self.cfg.min_chunk_samples && self.silence_run >= self.cfg.pause_frames;
        if at_pause || self.cur.len() >= self.cfg.max_chunk_samples {
            self.emit_current();
        }
    }

    fn emit_current(&mut self) {
        let samples = std::mem::take(&mut self.cur);
        let speech_ms = self.cur_speech_frames * FRAME_MS;
        self.cur_speech_frames = 0;
        if speech_ms < 150 {
            log::debug!("dropping silent chunk of {} samples", samples.len());
            return;
        }
        let chunk = AudioChunk { index: self.next_index, samples };
        self.next_index += 1;
        self.sink.on_chunk(chunk);
    }

    pub fn finish(mut self) -> StopResult {
        let mut rest = Vec::new();
        self.resampler.flush(&mut rest);
        self.push_16k(&rest);
        if !self.frame_buf.is_empty() {
            let tail = std::mem::take(&mut self.frame_buf);
            self.all.extend_from_slice(&tail);
            self.cur.extend_from_slice(&tail);
        }
        let is_silent = self.total_speech_frames * FRAME_MS < MIN_SPEECH_MS;
        let tail_samples = if self.next_index > 0 && self.cur_speech_frames * FRAME_MS < 150 {
            Vec::new()
        } else {
            std::mem::take(&mut self.cur)
        };
        let duration_secs = self.all.len() as f64 / TARGET_SAMPLE_RATE as f64;
        StopResult {
            tail: AudioChunk { index: self.next_index, samples: tail_samples },
            all_samples: self.all,
            duration_secs,
            is_silent,
        }
    }
}

// ───────────────────────── Recorder ─────────────────────────

enum Msg {
    Audio(Vec<f32>),
    Error(String),
    Finish(Sender<StopResult>),
    Cancel,
}

struct Session {
    stop_stream: Sender<()>,
    stream_thread: JoinHandle<()>,
    msg_tx: Sender<Msg>,
    worker: JoinHandle<()>,
    failed: Arc<AtomicBool>,
}

pub struct Recorder {
    inner: Mutex<Option<Session>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    pub fn list_devices() -> Vec<MicDevice> {
        let host = cpal::default_host();
        let default_name = host.default_input_device().and_then(|d| d.name().ok());
        let mut out: Vec<MicDevice> = Vec::new();
        let devices = match host.input_devices() {
            Ok(d) => d,
            Err(e) => {
                log::warn!("input_devices failed: {e}");
                return out;
            }
        };
        for d in devices {
            let Ok(name) = d.name() else { continue };
            if d.default_input_config().is_err() || out.iter().any(|m| m.id == name) {
                continue;
            }
            out.push(MicDevice {
                id: name.clone(),
                is_default: default_name.as_deref() == Some(name.as_str()),
                kind: classify_device(&name).to_string(),
                name,
            });
        }
        out
    }

    pub fn start(&self, device_id: Option<String>, sink: Arc<dyn RecorderSink>) -> anyhow::Result<()> {
        let mut guard = self.inner.lock();
        if let Some(old) = guard.take() {
            log::warn!("recorder.start while already recording; discarding previous session");
            shutdown(old, true);
        }

        let (msg_tx, msg_rx) = mpsc::channel::<Msg>();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let (stop_tx, stop_rx) = mpsc::channel::<()>();

        let stream_msg_tx = msg_tx.clone();
        let stream_thread = std::thread::Builder::new()
            .name("hoshiro-mic".into())
            .spawn(move || stream_thread_main(device_id, stream_msg_tx, ready_tx, stop_rx))
            .context("spawn mic thread")?;

        let rate = match ready_rx.recv_timeout(Duration::from_secs(6)) {
            Ok(Ok(rate)) => rate,
            Ok(Err(e)) => {
                let _ = stream_thread.join();
                return Err(anyhow!(e));
            }
            Err(_) => {
                let _ = stop_tx.send(());
                return Err(anyhow!("麦克风启动超时"));
            }
        };

        let failed = Arc::new(AtomicBool::new(false));
        let worker_failed = failed.clone();
        let worker = std::thread::Builder::new()
            .name("hoshiro-audio-dsp".into())
            .spawn(move || worker_main(rate, sink, msg_rx, worker_failed))
            .context("spawn audio worker")?;

        *guard = Some(Session { stop_stream: stop_tx, stream_thread, msg_tx, worker, failed });
        Ok(())
    }

    pub fn stop(&self) -> anyhow::Result<StopResult> {
        let session = self.inner.lock().take().ok_or_else(|| anyhow!("not recording"))?;
        let _ = session.stop_stream.send(());
        let _ = session.stream_thread.join();
        let (tx, rx) = mpsc::channel();
        session.msg_tx.send(Msg::Finish(tx)).map_err(|_| anyhow!("audio worker gone"))?;
        let result = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| anyhow!("audio worker did not finish"))?;
        let _ = session.worker.join();
        Ok(result)
    }

    /// Stop and discard.
    pub fn cancel(&self) {
        if let Some(session) = self.inner.lock().take() {
            shutdown(session, true);
        }
    }

    pub fn is_recording(&self) -> bool {
        self.inner.lock().as_ref().map(|s| !s.failed.load(Ordering::Relaxed)).unwrap_or(false)
    }
}

impl Default for Recorder {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn shutdown(session: Session, discard: bool) {
    let _ = session.stop_stream.send(());
    let _ = session.stream_thread.join();
    if discard {
        let _ = session.msg_tx.send(Msg::Cancel);
    }
    let _ = session.worker.join();
}

fn worker_main(rate: u32, sink: Arc<dyn RecorderSink>, rx: Receiver<Msg>, failed: Arc<AtomicBool>) {
    let mut proc = Processor::new(rate, sink.clone(), ChunkConfig::default());
    while let Ok(msg) = rx.recv() {
        match msg {
            Msg::Audio(buf) => proc.push(&buf),
            Msg::Error(e) => {
                log::error!("audio input error: {e}");
                if !failed.swap(true, Ordering::Relaxed) {
                    sink.on_error(&e);
                }
            }
            Msg::Finish(reply) => {
                let _ = reply.send(proc.finish());
                return;
            }
            Msg::Cancel => return,
        }
    }
}

fn stream_thread_main(
    device_id: Option<String>,
    msg_tx: Sender<Msg>,
    ready: Sender<Result<u32, String>>,
    stop_rx: Receiver<()>,
) {
    let host = cpal::default_host();
    let mut candidates: Vec<(cpal::Device, bool)> = Vec::new();
    if let Some(id) = device_id.as_deref().filter(|s| !s.trim().is_empty()) {
        match find_input_device(&host, id) {
            Some(d) => candidates.push((d, false)),
            None => log::warn!("microphone '{id}' not found, falling back to default input"),
        }
    }
    if let Some(d) = host.default_input_device() {
        candidates.push((d, true));
    }
    if candidates.is_empty() {
        let _ = ready.send(Err("没有可用的麦克风".into()));
        return;
    }

    let mut last_err = String::new();
    for (device, _is_default) in candidates {
        let name = device.name().unwrap_or_else(|_| "?".into());
        match open_stream(&device, msg_tx.clone()) {
            Ok((stream, rate, channels, format)) => {
                if let Err(e) = stream.play() {
                    last_err = format!("无法启动麦克风: {e}");
                    log::warn!("stream.play failed on '{name}': {e}");
                    continue;
                }
                log::info!("recording from '{name}' ({rate} Hz, {channels} ch, {format:?})");
                let _ = ready.send(Ok(rate));
                let _ = stop_rx.recv();
                drop(stream);
                return;
            }
            Err(e) => {
                log::warn!("cannot open '{name}': {e:#}");
                last_err = format!("无法打开麦克风: {e}");
            }
        }
    }
    let _ = ready.send(Err(last_err));
}

fn open_stream(device: &cpal::Device, tx: Sender<Msg>) -> anyhow::Result<(cpal::Stream, u32, usize, SampleFormat)> {
    let supported = device.default_input_config().context("default_input_config")?;
    let format = supported.sample_format();
    let config: cpal::StreamConfig = supported.config();
    let channels = config.channels.max(1) as usize;
    let rate = config.sample_rate.0;
    let stream = match format {
        SampleFormat::F32 => build::<f32>(device, &config, channels, tx)?,
        SampleFormat::I16 => build::<i16>(device, &config, channels, tx)?,
        SampleFormat::U16 => build::<u16>(device, &config, channels, tx)?,
        SampleFormat::I32 => build::<i32>(device, &config, channels, tx)?,
        SampleFormat::I8 => build::<i8>(device, &config, channels, tx)?,
        SampleFormat::U8 => build::<u8>(device, &config, channels, tx)?,
        SampleFormat::F64 => build::<f64>(device, &config, channels, tx)?,
        other => anyhow::bail!("unsupported sample format {other:?}"),
    };
    Ok((stream, rate, channels, format))
}

fn build<T>(device: &cpal::Device, config: &cpal::StreamConfig, channels: usize, tx: Sender<Msg>) -> anyhow::Result<cpal::Stream>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let err_tx = tx.clone();
    let stream = device.build_input_stream(
        config,
        move |data: &[T], _| {
            let mono: Vec<f32> = if channels == 1 {
                data.iter().map(|s| s.to_sample::<f32>()).collect()
            } else {
                data.chunks_exact(channels)
                    .map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / channels as f32)
                    .collect()
            };
            let _ = tx.send(Msg::Audio(mono));
        },
        move |e| {
            let _ = err_tx.send(Msg::Error(e.to_string()));
        },
        None,
    )?;
    Ok(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    #[derive(Default)]
    struct TestSink {
        chunks: Mutex<Vec<AudioChunk>>,
        levels: Mutex<Vec<f32>>,
    }
    impl RecorderSink for TestSink {
        fn on_level(&self, level: f32) {
            self.levels.lock().push(level);
        }
        fn on_chunk(&self, chunk: AudioChunk) {
            self.chunks.lock().push(chunk);
        }
    }

    const SR: usize = 16_000;

    fn speech(secs: f32) -> Vec<f32> {
        (0..(SR as f32 * secs) as usize)
            .map(|n| {
                let t = n as f32 / SR as f32;
                let env = 0.6 + 0.4 * (2.0 * PI * 4.0 * t).sin();
                0.12 * env * ((2.0 * PI * 200.0 * t).sin() + 0.5 * (2.0 * PI * 1300.0 * t).sin())
            })
            .collect()
    }

    fn silence(secs: f32) -> Vec<f32> {
        // faint deterministic noise around -70 dBFS
        (0..(SR as f32 * secs) as usize).map(|n| 0.0003 * ((n * 7919 % 101) as f32 / 50.0 - 1.0)).collect()
    }

    fn run(parts: &[Vec<f32>], cfg: ChunkConfig) -> (Arc<TestSink>, StopResult) {
        let sink = Arc::new(TestSink::default());
        let mut p = Processor::new(SR as u32, sink.clone(), cfg);
        for part in parts {
            for c in part.chunks(480) {
                p.push(c);
            }
        }
        let res = p.finish();
        (sink, res)
    }

    fn small_cfg() -> ChunkConfig {
        ChunkConfig { min_chunk_samples: 2 * SR, pause_frames: 11, max_chunk_samples: 6 * SR }
    }

    #[test]
    fn pure_silence_is_silent() {
        let (sink, res) = run(&[silence(2.0)], ChunkConfig::default());
        assert!(res.is_silent);
        assert!(sink.chunks.lock().is_empty());
        assert!((res.duration_secs - 2.0).abs() < 0.05);
        assert!(sink.levels.lock().iter().all(|&l| l < 0.05));
    }

    #[test]
    fn short_blip_is_silent_but_real_speech_is_not() {
        let (_, res) = run(&[silence(0.5), speech(0.1), silence(0.5)], ChunkConfig::default());
        assert!(res.is_silent);
        let (_, res) = run(&[silence(0.3), speech(1.0), silence(0.3)], ChunkConfig::default());
        assert!(!res.is_silent);
        assert_eq!(res.tail.index, 0);
        assert_eq!(res.tail.samples.len(), res.all_samples.len());
    }

    #[test]
    fn speech_immediately_at_start_is_detected() {
        let (_, res) = run(&[speech(0.6)], ChunkConfig::default());
        assert!(!res.is_silent);
    }

    #[test]
    fn levels_rise_and_fall() {
        let (sink, _) = run(&[silence(0.5), speech(1.0), silence(1.0)], ChunkConfig::default());
        let l = sink.levels.lock();
        let peak = l.iter().cloned().fold(0.0, f32::max);
        assert!(peak > 0.6 && peak <= 1.0, "peak {peak}");
        assert!(*l.last().unwrap() < 0.1);
    }

    #[test]
    fn cuts_at_pause_after_min_length() {
        // 1.5 s speech + 0.2 s gap (too short) + 1.0 s speech + 0.6 s pause -> cut inside that pause.
        let parts = [speech(1.5), silence(0.2), speech(1.0), silence(0.6), speech(1.0)];
        let (sink, res) = run(&parts, small_cfg());
        let chunks = sink.chunks.lock();
        assert_eq!(chunks.len(), 1, "chunks {}", chunks.len());
        assert_eq!(chunks[0].index, 0);
        let secs = chunks[0].samples.len() as f32 / SR as f32;
        assert!(secs > 3.0 && secs < 3.2, "chunk0 {secs}");
        assert_eq!(res.tail.index, 1);
        assert_eq!(chunks[0].samples.len() + res.tail.samples.len(), res.all_samples.len());
        assert!(!res.is_silent);
    }

    #[test]
    fn short_pause_before_min_length_does_not_cut() {
        let parts = [speech(0.8), silence(0.6), speech(0.8)];
        let (sink, res) = run(&parts, small_cfg());
        assert!(sink.chunks.lock().is_empty());
        assert_eq!(res.tail.index, 0);
    }

    #[test]
    fn forced_cut_without_pauses() {
        let parts = [speech(13.5)];
        let (sink, res) = run(&parts, small_cfg());
        let chunks = sink.chunks.lock();
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].samples.len() >= 6 * SR && chunks[0].samples.len() < 6 * SR + FRAME);
        assert_eq!(chunks[1].index, 1);
        assert_eq!(res.tail.index, 2);
        let total: usize = chunks.iter().map(|c| c.samples.len()).sum::<usize>() + res.tail.samples.len();
        assert_eq!(total, res.all_samples.len());
    }

    #[test]
    fn silent_tail_after_chunks_is_dropped_but_kept_in_all() {
        let parts = [speech(2.5), silence(0.6), silence(1.0)];
        let (sink, res) = run(&parts, small_cfg());
        assert_eq!(sink.chunks.lock().len(), 1);
        assert!(res.tail.samples.is_empty());
        assert!(res.all_samples.len() > 4 * SR);
    }

    #[test]
    fn downsampling_input_works_end_to_end() {
        let sink = Arc::new(TestSink::default());
        let mut p = Processor::new(48_000, sink, ChunkConfig::default());
        let input: Vec<f32> = (0..48_000).map(|n| 0.3 * (2.0 * PI * 1000.0 * n as f32 / 48_000.0).sin()).collect();
        for c in input.chunks(1024) {
            p.push(c);
        }
        let res = p.finish();
        assert!((res.all_samples.len() as i64 - 16_000).abs() < 600);
        assert!(!res.is_silent);
    }

    #[test]
    fn device_kind_heuristics() {
        assert_eq!(classify_device("Michiko's AirPods Pro"), "bluetooth");
        assert_eq!(classify_device("MacBook Pro Microphone"), "builtin");
        assert_eq!(classify_device("MacBook Pro麦克风"), "builtin");
        assert_eq!(classify_device("USB Audio Device"), "usb");
        assert_eq!(classify_device("BlackHole 2ch"), "virtual");
        assert_eq!(classify_device("Microsoft Teams Audio"), "virtual");
        assert_eq!(classify_device("Scarlett 2i2"), "unknown");
    }
}
