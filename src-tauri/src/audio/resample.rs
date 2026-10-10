//! Streaming windowed-sinc resampler (mono f32). Anti-aliased for downsampling.

use std::f64::consts::PI;

const ZEROS: f64 = 16.0;
const TABLE_RES: usize = 256;
const CUTOFF: f64 = 0.92;

pub struct Resampler {
    passthrough: bool,
    step: f64,
    half: usize,
    table: Vec<f32>,
    buf: Vec<f32>,
    pos: f64,
}

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        let passthrough = in_rate == out_rate || in_rate == 0 || out_rate == 0;
        let step = if passthrough { 1.0 } else { in_rate as f64 / out_rate as f64 };
        let scale = (out_rate as f64 / in_rate.max(1) as f64).min(1.0) * CUTOFF;
        let half = (ZEROS / scale).ceil() as usize;
        let n = half * TABLE_RES + 2;
        let mut table = Vec::with_capacity(n);
        for i in 0..n {
            let x = i as f64 / TABLE_RES as f64;
            let u = (x / half as f64).min(1.0);
            let window = 0.42 + 0.5 * (PI * u).cos() + 0.08 * (2.0 * PI * u).cos();
            table.push((scale * sinc(scale * x) * window) as f32);
        }
        let mut buf = Vec::new();
        buf.resize(half, 0.0);
        Self { passthrough, step, half, table, buf, pos: half as f64 }
    }

    fn weight(&self, x: f64) -> f32 {
        let a = x.abs() * TABLE_RES as f64;
        let i = a as usize;
        let frac = (a - i as f64) as f32;
        match (self.table.get(i), self.table.get(i + 1)) {
            (Some(&w0), Some(&w1)) => w0 + (w1 - w0) * frac,
            _ => 0.0,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.passthrough {
            out.extend_from_slice(input);
            return;
        }
        self.buf.extend_from_slice(input);
        let half = self.half as isize;
        loop {
            let i0 = self.pos.floor() as isize;
            if (i0 + half) as usize >= self.buf.len() {
                break;
            }
            let mut acc = 0.0f32;
            let mut wsum = 0.0f32;
            for j in (i0 - half + 1)..=(i0 + half) {
                let w = self.weight(j as f64 - self.pos);
                acc += w * self.buf[j as usize];
                wsum += w;
            }
            out.push(if wsum.abs() > 1e-6 { acc / wsum } else { 0.0 });
            self.pos += self.step;
        }
        let keep_from = (self.pos.floor() as isize - half).max(0) as usize;
        if keep_from > 0 {
            self.buf.drain(..keep_from);
            self.pos -= keep_from as f64;
        }
    }

    /// Emit the samples still held back by the filter delay.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        if self.passthrough {
            return;
        }
        let pad = vec![0.0f32; self.half + 2];
        self.process(&pad, out);
    }
}

/// One-shot convenience.
pub fn resample(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    let mut r = Resampler::new(in_rate, out_rate);
    let mut out = Vec::with_capacity(input.len() * out_rate as usize / in_rate.max(1) as usize + 8);
    r.process(input, &mut out);
    r.flush(&mut out);
    let expected = (input.len() as u64 * out_rate as u64 / in_rate.max(1) as u64) as usize;
    out.truncate(expected.max(1).min(out.len()));
    out
}

pub fn f32_to_i16(x: f32) -> i16 {
    (x * 32768.0).round().clamp(-32768.0, 32767.0) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, secs: f64, amp: f32) -> Vec<f32> {
        (0..(rate as f64 * secs) as usize)
            .map(|n| amp * (2.0 * PI * freq * n as f64 / rate as f64).sin() as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn sine_1khz_48k_to_16k_keeps_frequency() {
        let input = sine(1000.0, 48_000, 1.0, 0.5);
        let out = resample(&input, 48_000, 16_000);
        assert!((out.len() as i64 - 16_000).abs() <= 2, "len {}", out.len());
        assert!(out.iter().all(|v| v.is_finite()));
        let mid = &out[1000..out.len() - 1000];
        let hz = crossings(mid) as f64 / (mid.len() as f64 / 16_000.0);
        assert!((hz - 1000.0).abs() < 10.0, "freq {hz}");
        let r = rms(mid);
        assert!((r - 0.5 / 2f32.sqrt()).abs() < 0.01, "rms {r}");
    }

    #[test]
    fn aliasing_is_suppressed() {
        // 12 kHz would alias to 4 kHz when naively decimated by 3.
        let input = sine(12_000.0, 48_000, 1.0, 0.5);
        let out = resample(&input, 48_000, 16_000);
        let r = rms(&out[1000..out.len() - 1000]);
        assert!(r < 0.003, "alias rms {r}");
    }

    #[test]
    fn handles_44k1_and_streaming_chunks() {
        let input = sine(500.0, 44_100, 1.0, 0.4);
        let whole = resample(&input, 44_100, 16_000);
        let mut r = Resampler::new(44_100, 16_000);
        let mut out = Vec::new();
        for c in input.chunks(441) {
            r.process(c, &mut out);
        }
        r.flush(&mut out);
        assert!((out.len() as i64 - whole.len() as i64).abs() <= 3);
        let n = whole.len().min(out.len()) - 50;
        for i in 0..n {
            assert!((whole[i] - out[i]).abs() < 1e-3);
        }
    }

    #[test]
    fn upsample_and_passthrough() {
        let input = sine(300.0, 8_000, 0.5, 0.3);
        let out = resample(&input, 8_000, 16_000);
        assert!((out.len() as i64 - 8_000).abs() <= 2);
        assert!(out.iter().all(|v| v.is_finite()));
        let same = resample(&input, 16_000, 16_000);
        assert_eq!(same.len(), input.len());
    }
}
