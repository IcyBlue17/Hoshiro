//! WAV helpers. OWNER: audio/providers agent.

use super::resample::{f32_to_i16, resample};
use super::TARGET_SAMPLE_RATE;
use std::io::Cursor;

/// Encode 16-bit mono PCM to a WAV byte buffer.
pub fn encode_wav(samples: &[i16], sample_rate: u32) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::with_capacity(44 + samples.len() * 2));
    let result = (|| -> Result<(), hound::Error> {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)?;
        for &s in samples {
            writer.write_sample(s)?;
        }
        writer.finalize()
    })();
    match result {
        Ok(()) => cursor.into_inner(),
        Err(e) => {
            log::error!("wav encode failed: {e}");
            Vec::new()
        }
    }
}

/// Decode any WAV to mono f32 at its native rate.
pub fn decode_wav_mono_f32(bytes: &[u8]) -> anyhow::Result<(Vec<f32>, u32)> {
    let mut reader = hound::WavReader::new(Cursor::new(bytes))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>()?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f32;
            reader.samples::<i32>().map(|s| s.map(|v| v as f32 * scale)).collect::<Result<_, _>>()?
        }
    };
    let mono = if channels == 1 {
        interleaved
    } else {
        interleaved.chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32).collect()
    };
    Ok((mono, spec.sample_rate))
}

/// Decode a WAV file (any rate/channels) to 16 kHz mono i16 (used for retry from saved audio).
pub fn decode_wav_16k_mono(bytes: &[u8]) -> anyhow::Result<Vec<i16>> {
    let (mono, rate) = decode_wav_mono_f32(bytes)?;
    let resampled = resample(&mono, rate, TARGET_SAMPLE_RATE);
    Ok(resampled.into_iter().map(f32_to_i16).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_16k() {
        let samples: Vec<i16> = (0..16_000).map(|n| ((n as f32 * 0.05).sin() * 12_000.0) as i16).collect();
        let bytes = encode_wav(&samples, 16_000);
        assert_eq!(&bytes[..4], b"RIFF");
        let back = decode_wav_16k_mono(&bytes).unwrap();
        assert_eq!(back, samples);
    }

    #[test]
    fn decode_stereo_48k_to_16k_mono() {
        let spec = hound::WavSpec { channels: 2, sample_rate: 48_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
        let mut cur = Cursor::new(Vec::new());
        {
            let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
            for n in 0..48_000 {
                let v = ((2.0 * std::f32::consts::PI * 1000.0 * n as f32 / 48_000.0).sin() * 10_000.0) as i16;
                w.write_sample(v).unwrap();
                w.write_sample(v).unwrap();
            }
            w.finalize().unwrap();
        }
        let out = decode_wav_16k_mono(&cur.into_inner()).unwrap();
        assert!((out.len() as i64 - 16_000).abs() <= 2);
        assert!(out.iter().map(|v| v.unsigned_abs()).max().unwrap() > 9_000);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(decode_wav_16k_mono(b"not a wav").is_err());
    }
}
