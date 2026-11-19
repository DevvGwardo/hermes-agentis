use reqwest::Client;

#[derive(Clone, Debug)]
pub struct DecodedPcm {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

pub fn decode_wav_or_linear16_pcm(bytes: &[u8], fallback_sample_rate: u32) -> DecodedPcm {
    if bytes.len() >= 44 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WAVE" {
        let mut pos = 12usize;
        let mut sample_rate = fallback_sample_rate;
        let mut channels = 1u16;
        let mut bits_per_sample = 16u16;
        let mut data_slice: Option<&[u8]> = None;

        while pos + 8 <= bytes.len() {
            let id = &bytes[pos..pos + 4];
            let chunk_len = u32::from_le_bytes([
                bytes[pos + 4],
                bytes[pos + 5],
                bytes[pos + 6],
                bytes[pos + 7],
            ]) as usize;
            let body_start = pos + 8;
            let body_end = body_start.saturating_add(chunk_len).min(bytes.len());
            if id == b"fmt " && body_end - body_start >= 16 {
                channels = u16::from_le_bytes([bytes[body_start + 2], bytes[body_start + 3]]);
                sample_rate = u32::from_le_bytes([
                    bytes[body_start + 4],
                    bytes[body_start + 5],
                    bytes[body_start + 6],
                    bytes[body_start + 7],
                ]);
                bits_per_sample =
                    u16::from_le_bytes([bytes[body_start + 14], bytes[body_start + 15]]);
            } else if id == b"data" {
                data_slice = Some(&bytes[body_start..body_end]);
                break;
            }
            pos = body_end + (chunk_len & 1);
        }

        if let Some(raw) = data_slice {
            return DecodedPcm {
                sample_rate: sample_rate.max(8000),
                samples: pcm_i16_to_mono_f32(raw, channels.max(1) as usize, bits_per_sample),
            };
        }
    }

    DecodedPcm {
        sample_rate: fallback_sample_rate.max(8000),
        samples: pcm_i16_to_mono_f32(bytes, 1, 16),
    }
}

fn pcm_i16_to_mono_f32(raw: &[u8], channels: usize, bits: u16) -> Vec<f32> {
    if bits != 16 {
        return raw.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect();
    }
    let frame_bytes = 2 * channels;
    let mut out = Vec::with_capacity(raw.len() / frame_bytes.max(2));
    for frame in raw.chunks_exact(frame_bytes) {
        let mut sum = 0.0_f32;
        for ch in 0..channels {
            let s = i16::from_le_bytes([frame[ch * 2], frame[ch * 2 + 1]]) as f32 / 32768.0;
            sum += s;
        }
        out.push(sum / channels as f32);
    }
    out
}

pub async fn synthesize_deepgram_tts(
    http: &Client,
    api_key: &str,
    model: &str,
    text: &str,
) -> Result<DecodedPcm, String> {
    if api_key.trim().is_empty() {
        return Err("DEEPGRAM_API_KEY not configured".into());
    }
    let clean_text = text.trim();
    if clean_text.is_empty() {
        return Err("empty TTS text".into());
    }
    let model_name = if model.trim().is_empty() {
        "aura-2-thalia-en"
    } else {
        model.trim()
    };
    let url = format!(
        "https://api.deepgram.com/v1/speak?model={model_name}&encoding=linear16&sample_rate=24000"
    );
    let resp = http
        .post(&url)
        .header("Authorization", format!("Token {}", api_key.trim()))
        .header("Content-Type", "application/json")
        .json(&serde_json_lite_body(clean_text))
        .send()
        .await
        .map_err(|e| format!("deepgram TTS request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("deepgram TTS HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    Ok(decode_wav_or_linear16_pcm(&bytes, 24_000))
}

fn serde_json_lite_body(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', " ");
    format!("{{\"text\":\"{escaped}\"}}")
}

// ---------------------------------------------------------------------------
// Sentence chunking (P2-05: <600 ms first-audio)
// ---------------------------------------------------------------------------

/// Splits text into sentence chunks so TTS synthesis can start on the first
/// sentence while the rest is still being generated/spoken. The first
/// chunk is requested immediately, minimizing time-to-first-audio.
#[derive(Default)]
pub struct SentenceChunker {
    pending: String,
}

const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "vs", "etc", "e.g", "i.e", "inc", "ltd",
];

impl SentenceChunker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a text delta; returns sentences completed by this delta.
    /// A terminator counts when followed by whitespace or when it ends the
    /// current buffer; [`Self::flush`] finalizes any unterminated tail.
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.pending.push_str(delta);
        self.drain_completed()
    }

    /// Finalize: return any remaining unterminated text as the last chunk.
    pub fn flush(&mut self) -> Option<String> {
        let tail = self.pending.trim().to_string();
        self.pending.clear();
        if tail.is_empty() {
            None
        } else {
            Some(tail)
        }
    }

    /// One-shot split of complete text.
    pub fn chunk_all(text: &str) -> Vec<String> {
        let mut c = Self::new();
        let mut out = c.push(text);
        if let Some(tail) = c.flush() {
            out.push(tail);
        }
        out
    }

    fn drain_completed(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        loop {
            let Some(end) = self.find_boundary() else {
                break;
            };
            let sentence = self.pending[..end].trim().to_string();
            self.pending.drain(..end);
            // Skip leading whitespace of the remainder.
            let ws = self.pending.len() - self.pending.trim_start().len();
            self.pending.drain(..ws);
            if !sentence.is_empty() {
                out.push(sentence);
            }
        }
        out
    }

    /// Byte index just past the next sentence boundary, if any.
    fn find_boundary(&self) -> Option<usize> {
        let chars: Vec<(usize, char)> = self.pending.char_indices().collect();
        for (i, &(pos, c)) in chars.iter().enumerate() {
            let next = chars.get(i + 1).map(|&(_, nc)| nc);
            if c == '\n' {
                return Some(pos + 1);
            }
            if matches!(c, '.' | '!' | '?') {
                // Ellipsis: boundary only after the last dot.
                if c == '.' && next == Some('.') {
                    continue;
                }
                // Terminator must be followed by whitespace or end-of-text.
                let at_end = next.is_none();
                let followed_by_ws = next.map(|n| n.is_whitespace()).unwrap_or(false);
                if !at_end && !followed_by_ws {
                    continue;
                }
                // Abbreviation guard ("Dr. Smith"): look at the word before.
                if c == '.' {
                    let before = &self.pending[..pos];
                    let word: String = before
                        .chars()
                        .rev()
                        .take_while(|ch| ch.is_alphabetic() || *ch == '.')
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    let word = word.trim_end_matches('.');
                    // Single-letter initials ("J. R. R. Tolkien"). Must be
                    // checked BEFORE lowercasing, or the case is lost.
                    if word.chars().count() == 1
                        && word.chars().next().unwrap().is_uppercase()
                    {
                        continue;
                    }
                    let word = word.to_lowercase();
                    if ABBREVIATIONS.contains(&word.as_str()) {
                        continue;
                    }
                }
                return Some(pos + c.len_utf8());
            }
        }
        None
    }
}

/// Encode mono f32 PCM as a 16-bit PCM WAV (whisper.cpp input format).
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data = crate::dsp::encode_linear16(samples);
    let byte_rate = sample_rate * 2;
    let mut out = Vec::with_capacity(44 + data.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_wav_header_and_i16_samples() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&24000u32.to_le_bytes());
        wav.extend_from_slice(&48000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&16384i16.to_le_bytes());
        wav.extend_from_slice(&(-16384i16).to_le_bytes());

        let pcm = decode_wav_or_linear16_pcm(&wav, 16000);
        assert_eq!(pcm.sample_rate, 24000);
        assert_eq!(pcm.samples.len(), 2);
        assert!((pcm.samples[0] - 0.5).abs() < 0.01);
        assert!((pcm.samples[1] + 0.5).abs() < 0.01);
    }

    #[test]
    fn chunk_all_splits_sentences() {
        assert_eq!(
            SentenceChunker::chunk_all("Hello there. How are you? I am fine!"),
            vec!["Hello there.", "How are you?", "I am fine!"]
        );
        assert_eq!(
            SentenceChunker::chunk_all("line one\nline two"),
            vec!["line one", "line two"]
        );
        assert!(SentenceChunker::chunk_all("   ").is_empty());
        assert_eq!(
            SentenceChunker::chunk_all("No terminator here"),
            vec!["No terminator here"]
        );
    }

    #[test]
    fn chunker_keeps_abbreviations_decimals_and_ellipses_intact() {
        assert_eq!(
            SentenceChunker::chunk_all("Dr. Smith went home. He slept."),
            vec!["Dr. Smith went home.", "He slept."]
        );
        assert_eq!(
            SentenceChunker::chunk_all("It costs 3.5 dollars. Buy it."),
            vec!["It costs 3.5 dollars.", "Buy it."]
        );
        assert_eq!(
            SentenceChunker::chunk_all("Wait... what?"),
            vec!["Wait...", "what?"]
        );
        assert_eq!(
            SentenceChunker::chunk_all("J. R. R. Tolkien wrote books. Many books."),
            vec!["J. R. R. Tolkien wrote books.", "Many books."]
        );
    }

    #[test]
    fn streaming_push_emits_first_sentence_before_the_rest_arrives() {
        let mut c = SentenceChunker::new();
        // First sentence complete -> available immediately (first-audio path).
        assert_eq!(c.push("Hello there. How are"), vec!["Hello there."]);
        assert_eq!(c.push(" you? "), vec!["How are you?"]);
        assert_eq!(c.flush(), None);
        // Unterminated tail is finalized by flush.
        assert!(c.push("trailing words").is_empty());
        assert_eq!(c.flush().as_deref(), Some("trailing words"));
        assert_eq!(c.flush(), None);
    }

    #[test]
    fn wav_encode_roundtrips_through_decoder() {
        let pcm: Vec<f32> = (0..500).map(|i| (i as f32 * 0.02).sin() * 0.7).collect();
        let wav = encode_wav(&pcm, 16_000);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        let decoded = decode_wav_or_linear16_pcm(&wav, 48_000);
        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.samples.len(), pcm.len());
        for (a, b) in pcm.iter().zip(&decoded.samples) {
            assert!((a - b).abs() <= 1.0 / 32768.0 + 1e-6);
        }
    }
}
