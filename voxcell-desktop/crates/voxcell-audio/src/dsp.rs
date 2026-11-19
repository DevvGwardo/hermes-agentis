//! Small allocation-free-on-the-callback DSP helpers shared by capture,
//! VAD, STT and TTS paths.

/// Root-mean-square energy of a sample block (0 for empty input).
pub fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let acc: f32 = samples.iter().map(|s| s * s).sum();
    (acc / samples.len() as f32).sqrt()
}

/// Linear-interpolation resampler (speech-grade; used for 48 kHz → 16 kHz
/// mic downsample and TTS rate matching). Same-rate input is cloned.
pub fn resample_linear(samples: &[f32], from_sr: u32, to_sr: u32) -> Vec<f32> {
    if samples.is_empty() || from_sr == 0 || to_sr == 0 {
        return Vec::new();
    }
    if from_sr == to_sr {
        return samples.to_vec();
    }
    let ratio = from_sr as f64 / to_sr as f64;
    let out_len = ((samples.len() as f64) / ratio).ceil() as usize;
    let mut out = Vec::with_capacity(out_len);
    // Output grid spans the input's inter-sample range [0, len-1]: the first
    // output sample lands exactly on the first input sample, and the step is
    // (len-1)/out_len (≈ from/to for long blocks). This keeps endpoints
    // stable for short blocks instead of overshooting the last input sample.
    let step = (samples.len() - 1) as f64 / out_len as f64;
    for i in 0..out_len {
        let pos = i as f64 * step;
        let idx0 = (pos.floor() as usize).min(samples.len() - 1);
        let idx1 = (idx0 + 1).min(samples.len() - 1);
        let frac = (pos - idx0 as f64) as f32;
        out.push(samples[idx0] * (1.0 - frac) + samples[idx1] * frac);
    }
    out
}

/// Downmix interleaved multi-channel audio to mono.
pub fn mix_to_mono(interleaved: &[f32], channels: usize) -> Vec<f32> {
    let ch = channels.max(1);
    if ch == 1 {
        return interleaved.to_vec();
    }
    interleaved
        .chunks_exact(ch)
        .map(|frame| frame.iter().sum::<f32>() / ch as f32)
        .collect()
}

/// Encode f32 [-1, 1] samples as little-endian linear16 PCM bytes
/// (Deepgram Flux / Aura-2 wire format).
pub fn encode_linear16(samples: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(samples.len() * 2);
    for &s in samples {
        // Full-scale asymmetric mapping: -1.0 -> -32768, +1.0 -> +32767
        // (i16 has no +32768). Scaling by 32768 on both sides keeps the
        // decode (/32768) roundtrip error within half a quantization step.
        let v = (s.clamp(-1.0, 1.0) * 32768.0)
            .round()
            .clamp(-32768.0, 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// Decode little-endian linear16 PCM bytes to f32 [-1, 1].
pub fn decode_linear16(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_of_known_signals() {
        assert_eq!(rms(&[]), 0.0);
        assert!((rms(&[0.5; 100]) - 0.5).abs() < 1e-6);
        // Sine RMS = amplitude / sqrt(2)
        let sine: Vec<f32> = (0..4096)
            .map(|i| (i as f32 * 0.1).sin() * 0.8)
            .collect();
        let expected = 0.8 / 2.0_f32.sqrt();
        assert!((rms(&sine) - expected).abs() < 0.01);
    }

    #[test]
    fn resample_48k_to_16k_length_and_dc() {
        let dc = vec![0.25; 4800];
        let out = resample_linear(&dc, 48_000, 16_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|&s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input = vec![0.1, -0.2, 0.3];
        assert_eq!(resample_linear(&input, 16_000, 16_000), input);
        assert!(resample_linear(&[], 48_000, 16_000).is_empty());
    }

    #[test]
    fn resample_upsample_preserves_endpoints() {
        let input = vec![0.0, 1.0];
        let out = resample_linear(&input, 16_000, 48_000);
        assert_eq!(out.len(), 6);
        assert_eq!(out[0], 0.0);
        assert!((out[3] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn mono_mix_averages_channels() {
        let stereo = vec![0.2, 0.6, -0.4, 0.0];
        assert_eq!(mix_to_mono(&stereo, 2), vec![0.4, -0.2]);
        assert_eq!(mix_to_mono(&stereo, 1), stereo);
    }

    #[test]
    fn linear16_roundtrip_within_quantization_error() {
        let input: Vec<f32> = (0..1000)
            .map(|i| ((i as f32) * 0.01).sin() * 0.9)
            .collect();
        let bytes = encode_linear16(&input);
        assert_eq!(bytes.len(), 2000);
        let back = decode_linear16(&bytes);
        assert_eq!(back.len(), input.len());
        for (a, b) in input.iter().zip(&back) {
            assert!((a - b).abs() <= 1.0 / 32768.0 + 1e-6);
        }
    }

    #[test]
    fn linear16_encode_clamps_and_ignores_trailing_byte() {
        let bytes = encode_linear16(&[2.0, -2.0]);
        assert_eq!(decode_linear16(&bytes), vec![32767.0 / 32768.0, -1.0]);
        assert_eq!(decode_linear16(&[0x00, 0x40, 0xFF]), vec![0.5]);
    }
}
