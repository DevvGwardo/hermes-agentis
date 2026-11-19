//! Port of `src/utils/sidebarCharacterState.ts` (P1-06 parity check) plus the
//! original drafted presentation API (kept for voxcell-ui compatibility).
//!
//! TS-parity layer: `derive_sidebar_character_snapshot` synchronizes mood, eye
//! state, viseme, energy, tone, pulse, and compact running-task telemetry from
//! live app state (speaking > listening > working > attention > offline > idle),
//! with cursor-look derivation via `compute_sidebar_pointer_look_target`.

use crate::cli::{ApprovalCard, ApprovalStatus};
use crate::sessions::GatewaySessionInfo;
use crate::tasks::{resolve_task_display_title, resolve_task_progress, resolve_task_step_breakdown};
use crate::types::{BackgroundTask, ExpressionMood, EyeState, MouthViseme, SessionStatus, TaskLifecycleStatus};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarWorkTone {
    Emerald,
    Amber,
    Rose,
    Slate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarCharacterInput {
    pub connected: bool,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub active_channel_thinking: bool,
    pub any_thinking: bool,
    pub running_task_count: usize,
    pub pending_approval_count: usize,
    pub voice_amplitude: f32,
    pub active_task_progress: Option<f32>,
    pub active_task_title: Option<String>,
    pub look_x: f32,
    pub look_y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarCharacterPresentation {
    pub mood: ExpressionMood,
    pub eye_state: EyeState,
    pub viseme: MouthViseme,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub energy: f32,
    pub tone: SidebarWorkTone,
    pub status_label: String,
    pub status_detail: String,
    pub look_x: f32,
    pub look_y: f32,
}

pub fn resolve_sidebar_look_target(norm_x: f32, norm_y: f32, inside: bool) -> (f32, f32) {
    if !inside {
        return (0.0, 0.0);
    }
    (norm_x.clamp(-1.0, 1.0), norm_y.clamp(-1.0, 1.0))
}

pub fn amplitude_to_viseme(amp: f32) -> MouthViseme {
    if amp < 0.06 {
        MouthViseme::Closed
    } else if amp < 0.16 {
        MouthViseme::Small
    } else if amp < 0.32 {
        MouthViseme::Mid
    } else if amp < 0.54 {
        MouthViseme::Round
    } else if amp < 0.76 {
        MouthViseme::Ee
    } else {
        MouthViseme::Wide
    }
}

pub fn derive_sidebar_character_state(input: &SidebarCharacterInput) -> SidebarCharacterPresentation {
    let lx = input.look_x.clamp(-1.0, 1.0);
    let ly = input.look_y.clamp(-1.0, 1.0);

    if !input.connected {
        return SidebarCharacterPresentation {
            mood: ExpressionMood::Calm,
            eye_state: EyeState::Half,
            viseme: MouthViseme::Closed,
            is_speaking: false,
            is_listening: false,
            energy: 0.08,
            tone: SidebarWorkTone::Rose,
            status_label: "Offline".to_string(),
            status_detail: "gateway unreachable".to_string(),
            look_x: lx,
            look_y: ly,
        };
    }

    if input.pending_approval_count > 0 {
        return SidebarCharacterPresentation {
            mood: ExpressionMood::Alert,
            eye_state: EyeState::Wide,
            viseme: MouthViseme::Closed,
            is_speaking: false,
            is_listening: false,
            energy: 0.55,
            tone: SidebarWorkTone::Amber,
            status_label: "Approval needed".to_string(),
            status_detail: format!("{} waiting", input.pending_approval_count),
            look_x: lx,
            look_y: ly,
        };
    }

    if input.is_speaking {
        let amp = input.voice_amplitude.clamp(0.18, 1.0);
        return SidebarCharacterPresentation {
            mood: ExpressionMood::Speaking,
            eye_state: EyeState::Open,
            viseme: amplitude_to_viseme(amp),
            is_speaking: true,
            is_listening: false,
            energy: amp,
            tone: SidebarWorkTone::Emerald,
            status_label: "Speaking".to_string(),
            status_detail: "voice stream active".to_string(),
            look_x: lx,
            look_y: ly,
        };
    }

    if input.is_listening {
        let amp = input.voice_amplitude.clamp(0.0, 1.0);
        return SidebarCharacterPresentation {
            mood: ExpressionMood::Listening,
            eye_state: EyeState::Open,
            viseme: if amp > 0.12 {
                MouthViseme::Small
            } else {
                MouthViseme::Rest
            },
            is_speaking: false,
            is_listening: true,
            energy: (0.28 + amp * 0.55).clamp(0.0, 1.0),
            tone: SidebarWorkTone::Emerald,
            status_label: "Listening".to_string(),
            status_detail: "mic open".to_string(),
            look_x: lx,
            look_y: ly,
        };
    }

    if input.running_task_count > 0 || input.active_channel_thinking || input.any_thinking {
        let detail = if let Some(title) = input.active_task_title.as_deref() {
            if let Some(pct) = input.active_task_progress {
                format!("{title} · {}%", pct.round().clamp(0.0, 100.0) as u8)
            } else {
                title.to_string()
            }
        } else if input.running_task_count > 1 {
            format!("{} tasks running", input.running_task_count)
        } else {
            "working on turn".to_string()
        };

        return SidebarCharacterPresentation {
            mood: ExpressionMood::Thinking,
            eye_state: EyeState::FocusedOpen(),
            viseme: MouthViseme::Rest,
            is_speaking: false,
            is_listening: false,
            energy: 0.62,
            tone: SidebarWorkTone::Amber,
            status_label: "Working".to_string(),
            status_detail: detail,
            look_x: lx,
            look_y: ly,
        };
    }

    SidebarCharacterPresentation {
        mood: ExpressionMood::Calm,
        eye_state: EyeState::Open,
        viseme: MouthViseme::Closed,
        is_speaking: false,
        is_listening: false,
        energy: 0.16,
        tone: SidebarWorkTone::Emerald,
        status_label: "Online".to_string(),
        status_detail: "hermes ready".to_string(),
        look_x: lx,
        look_y: ly,
    }
}

trait EyeStateExt {
    #[allow(non_snake_case)]
    fn FocusedOpen() -> Self;
}

impl EyeStateExt for EyeState {
    fn FocusedOpen() -> Self {
        EyeState::Open
    }
}

pub fn lead_running_task(tasks: &[BackgroundTask]) -> Option<&BackgroundTask> {
    tasks.iter().find(|t| t.status == TaskLifecycleStatus::Running)
}

// ---------------------------------------------------------------------------
// TS-parity snapshot layer (sidebarCharacterState.ts)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarCharacterMode {
    Speaking,
    Listening,
    Working,
    Attention,
    Offline,
    Idle,
}

/// TS `EyeState` includes `side`/`happy` which the shared core enum lacks;
/// the sidebar snapshot uses this local superset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarEyeState {
    Open,
    Blink,
    Half,
    Wide,
    Happy,
    Side,
}

/// Mirrors `StatusPulseState` from components/ui/StatusPulse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarPulseState {
    Idle,
    Live,
    Thinking,
    Speaking,
    Listening,
    Waiting,
    Done,
    Error,
    Muted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarGatewayStatus {
    pub connected: bool,
    pub active_model: Option<String>,
    pub profile: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarActiveTaskSummary {
    pub id: String,
    pub channel_id: String,
    pub title: String,
    pub step_text: String,
    pub progress_pct: u8,
    pub ratio_label: String,
    pub is_indeterminate: bool,
    pub created_at_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarPendingApprovalSummary {
    pub id: String,
    pub channel_id: String,
    pub title: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarStateInput<'a> {
    pub tasks: &'a [BackgroundTask],
    pub approvals: &'a [ApprovalCard],
    pub thinking_by_channel: &'a HashMap<String, bool>,
    pub hermes_sessions: &'a [GatewaySessionInfo],
    pub active_channel: &'a str,
    pub home_session_draft: bool,
    pub base_mood: ExpressionMood,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub input_level: f32,
    pub output_level: f32,
    pub frequency_bands: &'a [f32],
    pub tick: u64,
    pub gateway_status: Option<&'a SidebarGatewayStatus>,
    pub last_spoken_line: &'a str,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarCharacterSnapshot {
    pub mode: SidebarCharacterMode,
    pub mood: ExpressionMood,
    pub eye_state: SidebarEyeState,
    pub viseme: MouthViseme,
    pub energy: f32,
    pub tone: SidebarWorkTone,
    pub pulse_state: SidebarPulseState,
    pub status_label: String,
    pub gateway_line: String,
    pub running_task_count: usize,
    pub pending_approval_count: usize,
    pub active_task: Option<SidebarActiveTaskSummary>,
    pub top_approval: Option<SidebarPendingApprovalSummary>,
}

fn resolve_mood_eye_state(mood: ExpressionMood) -> SidebarEyeState {
    match mood {
        ExpressionMood::Alert => SidebarEyeState::Wide,
        ExpressionMood::Thinking => SidebarEyeState::Side,
        ExpressionMood::Focused => SidebarEyeState::Half,
        ExpressionMood::Happy => SidebarEyeState::Happy,
        _ => SidebarEyeState::Open,
    }
}

fn peak_band_energy(bands: &[f32]) -> f32 {
    if bands.is_empty() {
        return 0.0;
    }
    let count = 8usize.min(bands.len());
    let mut sum = 0.0_f32;
    let mut max = 0.0_f32;
    for &band in bands.iter().take(count) {
        let val = if band.is_finite() { band.max(0.0) } else { 0.0 };
        sum += val;
        if val > max {
            max = val;
        }
    }
    (max * 0.6 + (sum / count.max(1) as f32) * 0.4).clamp(0.0, 1.0)
}

/// Derives the sidebar character's synchronized expression, tone, viseme,
/// energy, and compact running-task telemetry from live app state (TS parity).
pub fn derive_sidebar_character_snapshot(input: &SidebarStateInput<'_>) -> SidebarCharacterSnapshot {
    let mut running_tasks: Vec<&BackgroundTask> = input
        .tasks
        .iter()
        .filter(|t| t.status == TaskLifecycleStatus::Running)
        .collect();
    let prioritize_active = !input.home_session_draft && !input.active_channel.is_empty();
    running_tasks.sort_by(|a, b| {
        if prioritize_active {
            let a_active = usize::from(a.channel_id != input.active_channel);
            let b_active = usize::from(b.channel_id != input.active_channel);
            let ord = a_active.cmp(&b_active);
            if ord != std::cmp::Ordering::Equal {
                return ord;
            }
        }
        let recency = |t: &BackgroundTask| t.updated_at_ms.or(t.created_at_ms).unwrap_or(0);
        recency(b).cmp(&recency(a))
    });

    let running_channel_set: HashSet<&str> = running_tasks.iter().map(|t| t.channel_id.as_str()).collect();
    let mut extra_thinking_channels = 0usize;
    let mut seen_extra: Vec<String> = Vec::new();
    let mut seen_set: HashSet<String> = HashSet::new();

    let mut thinking_keys: Vec<&String> = input
        .thinking_by_channel
        .iter()
        .filter(|(_, v)| **v)
        .map(|(k, _)| k)
        .collect();
    thinking_keys.sort();
    for ch_id in thinking_keys {
        if !running_channel_set.contains(ch_id.as_str()) && seen_set.insert(ch_id.clone()) {
            seen_extra.push(ch_id.clone());
            extra_thinking_channels += 1;
        }
    }
    for session in input.hermes_sessions {
        if matches!(
            session.status,
            SessionStatus::Thinking | SessionStatus::Speaking | SessionStatus::Running
        ) && !running_channel_set.contains(session.channel_id.as_str())
            && seen_set.insert(session.channel_id.clone())
        {
            seen_extra.push(session.channel_id.clone());
            extra_thinking_channels += 1;
        }
    }

    let running_task_count = running_tasks.len() + extra_thinking_channels;
    let pending_approvals: Vec<&ApprovalCard> = input
        .approvals
        .iter()
        .filter(|a| a.status == ApprovalStatus::Pending)
        .collect();
    let pending_approval_count = pending_approvals.len();

    let lead_task = running_tasks.first().copied();
    let active_task = if let Some(lead) = lead_task {
        let progress = resolve_task_progress(lead);
        let breakdown = resolve_task_step_breakdown(lead);
        let step_text = if !breakdown.active_step_text.is_empty() {
            breakdown.active_step_text
        } else {
            lead.last_step.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "Working…".to_string())
        };
        Some(SidebarActiveTaskSummary {
            id: lead.id.clone(),
            channel_id: lead.channel_id.clone(),
            title: resolve_task_display_title(lead),
            step_text,
            progress_pct: progress.pct,
            ratio_label: progress.ratio_label,
            is_indeterminate: progress.is_indeterminate,
            created_at_ms: lead.created_at_ms,
        })
    } else if running_task_count > 0 {
        let first_active_channel = seen_extra
            .first()
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                if input.active_channel.is_empty() {
                    "session".to_string()
                } else {
                    input.active_channel.to_string()
                }
            });
        Some(SidebarActiveTaskSummary {
            id: format!("live-channel-{first_active_channel}"),
            channel_id: first_active_channel.clone(),
            title: format!("Routing turn in #{first_active_channel}"),
            step_text: "Thinking…".to_string(),
            progress_pct: 0,
            ratio_label: "live".to_string(),
            is_indeterminate: true,
            created_at_ms: None,
        })
    } else {
        None
    };

    let top_approval = pending_approvals.first().map(|a| SidebarPendingApprovalSummary {
        id: a.id.clone(),
        channel_id: a.channel_id.clone(),
        title: a.title.clone(),
    });

    let is_gateway_connected = input.gateway_status.is_some_and(|g| g.connected);
    let gateway_line = match input.gateway_status {
        Some(g) => {
            if is_gateway_connected {
                format!(
                    "{} · {}",
                    g.active_model.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "hermes-agent".to_string()),
                    g.profile.clone().filter(|s| !s.is_empty()).unwrap_or_else(|| "default".to_string())
                )
            } else {
                "Gateway offline · Open Ops".to_string()
            }
        }
        None => input.last_spoken_line.to_string(),
    };

    let band_peak = peak_band_energy(input.frequency_bands);

    if input.is_speaking {
        let energy = 0.46_f32.max(input.output_level).max(band_peak).clamp(0.46, 1.0);
        let eye_mood = if input.base_mood == ExpressionMood::Happy {
            ExpressionMood::Happy
        } else {
            ExpressionMood::Calm
        };
        return SidebarCharacterSnapshot {
            mode: SidebarCharacterMode::Speaking,
            mood: ExpressionMood::Speaking,
            eye_state: resolve_mood_eye_state(eye_mood),
            viseme: amplitude_to_viseme_animated(
                energy,
                ExpressionMood::Speaking,
                input.tick,
                true,
                input.frequency_bands,
                input.last_spoken_line,
            ),
            energy,
            tone: SidebarWorkTone::Emerald,
            pulse_state: SidebarPulseState::Speaking,
            status_label: "speaking".to_string(),
            gateway_line,
            running_task_count,
            pending_approval_count,
            active_task,
            top_approval,
        };
    }

    if input.is_listening {
        let energy = 0.32_f32.max(input.input_level).max(band_peak * 0.85).clamp(0.32, 0.95);
        return SidebarCharacterSnapshot {
            mode: SidebarCharacterMode::Listening,
            mood: ExpressionMood::Listening,
            eye_state: SidebarEyeState::Open,
            viseme: MouthViseme::Rest,
            energy,
            tone: SidebarWorkTone::Emerald,
            pulse_state: SidebarPulseState::Listening,
            status_label: "listening".to_string(),
            gateway_line,
            running_task_count,
            pending_approval_count,
            active_task,
            top_approval,
        };
    }

    if running_task_count > 0 {
        let energy = (0.44 + (4usize.min(running_task_count - 1)) as f32 * 0.09).clamp(0.44, 0.78);
        return SidebarCharacterSnapshot {
            mode: SidebarCharacterMode::Working,
            mood: ExpressionMood::Thinking,
            eye_state: SidebarEyeState::Side,
            viseme: MouthViseme::Rest,
            energy,
            tone: SidebarWorkTone::Amber,
            pulse_state: SidebarPulseState::Thinking,
            status_label: if running_task_count > 1 {
                format!("{running_task_count} working")
            } else {
                "working".to_string()
            },
            gateway_line,
            running_task_count,
            pending_approval_count,
            active_task,
            top_approval,
        };
    }

    if pending_approval_count > 0 {
        return SidebarCharacterSnapshot {
            mode: SidebarCharacterMode::Attention,
            mood: ExpressionMood::Alert,
            eye_state: SidebarEyeState::Wide,
            viseme: MouthViseme::Rest,
            energy: 0.42,
            tone: SidebarWorkTone::Amber,
            pulse_state: SidebarPulseState::Waiting,
            status_label: if pending_approval_count > 1 {
                format!("{pending_approval_count} approvals")
            } else {
                "1 approval".to_string()
            },
            gateway_line,
            running_task_count,
            pending_approval_count,
            active_task,
            top_approval,
        };
    }

    if input.gateway_status.is_some() && !is_gateway_connected {
        let mood = if input.base_mood == ExpressionMood::Thinking {
            ExpressionMood::Calm
        } else {
            input.base_mood
        };
        return SidebarCharacterSnapshot {
            mode: SidebarCharacterMode::Offline,
            mood,
            eye_state: resolve_mood_eye_state(mood),
            viseme: MouthViseme::Closed,
            energy: 0.08,
            tone: SidebarWorkTone::Rose,
            pulse_state: SidebarPulseState::Error,
            status_label: "offline".to_string(),
            gateway_line,
            running_task_count: 0,
            pending_approval_count: 0,
            active_task: None,
            top_approval: None,
        };
    }

    let mood = input.base_mood;
    SidebarCharacterSnapshot {
        mode: SidebarCharacterMode::Idle,
        mood,
        eye_state: resolve_mood_eye_state(mood),
        viseme: if mood == ExpressionMood::Happy { MouthViseme::Smile } else { MouthViseme::Closed },
        energy: 0.16,
        tone: SidebarWorkTone::Emerald,
        pulse_state: if input.gateway_status.is_some() {
            SidebarPulseState::Live
        } else {
            SidebarPulseState::Idle
        },
        status_label: if input.gateway_status.is_some() {
            "online".to_string()
        } else if input.home_session_draft || input.active_channel.is_empty() {
            "new session".to_string()
        } else {
            format!("#{}", input.active_channel)
        },
        gateway_line,
        running_task_count: 0,
        pending_approval_count: 0,
        active_task: None,
        top_approval: None,
    }
}

/// Maps pointer coordinates over the sidebar character card to a normalized
/// gaze vector centered on the portrait avatar.
pub fn compute_sidebar_pointer_look_target(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    client_x: f32,
    client_y: f32,
    portrait_center_offset_px: f32,
) -> (f32, f32) {
    let cx = left + portrait_center_offset_px.min(width * 0.25);
    let cy = top + portrait_center_offset_px.min(height * 0.5);
    let norm_w = 96.0_f32.max(width * 0.72);
    let norm_h = 64.0_f32.max(height * 0.85);
    (
        ((client_x - cx) / norm_w).clamp(-1.0, 1.0),
        ((client_y - cy) / norm_h).clamp(-1.0, 1.0),
    )
}

// ---------------------------------------------------------------------------
// Animated viseme engine (asciiCharacter.ts parity for the sidebar snapshot)
// ---------------------------------------------------------------------------

fn band_at(bands: &[f32], idx: usize) -> f32 {
    bands.get(idx).copied().unwrap_or(0.0)
}

/// Extracts a compact phoneme-viseme sequence from spoken text (TS
/// `textToVisemeTimeline`).
pub fn text_to_viseme_timeline(text: &str) -> Vec<MouthViseme> {
    let default = vec![MouthViseme::Small, MouthViseme::Mid, MouthViseme::Wide, MouthViseme::Round];
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return default;
    }
    let chars: Vec<char> = cleaned.chars().collect();
    let mut out: Vec<MouthViseme> = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() || matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '—' | '-') {
            if !out.is_empty() && out.last() != Some(&MouthViseme::Rest) {
                out.push(MouthViseme::Rest);
            }
            i += 1;
            continue;
        }
        let two: String = chars[i..(i + 2).min(chars.len())]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        let matched: Option<(MouthViseme, usize)> = if matches!(two.as_bytes().first(), Some(b'm' | b'b' | b'p')) {
            Some((MouthViseme::Mbp, 1))
        } else if matches!(two.as_bytes().first(), Some(b'f' | b'v')) {
            Some((MouthViseme::Fv, 1))
        } else if two.starts_with("th") {
            Some((MouthViseme::Lth, 2))
        } else if matches!(two.as_bytes().first(), Some(b'l' | b'd' | b't' | b'n')) {
            Some((MouthViseme::Lth, 1))
        } else if two.starts_with("ee") || two.starts_with("ea") || two.starts_with("ie") || two.starts_with("ch") || two.starts_with("sh") {
            Some((MouthViseme::Ee, 2))
        } else if matches!(two.as_bytes().first(), Some(b'i' | b'y' | b'j')) {
            Some((MouthViseme::Ee, 1))
        } else if two.starts_with("oo") || two.starts_with("ou") || two.starts_with("oh") || two.starts_with("ow") {
            Some((MouthViseme::Round, 2))
        } else if matches!(two.as_bytes().first(), Some(b'o' | b'w' | b'q')) {
            Some((MouthViseme::Round, 1))
        } else if two.starts_with("ah") || two.starts_with("ha") || two.starts_with("aa") || two.starts_with("ar") {
            Some((MouthViseme::Wide, 2))
        } else if two.as_bytes().first() == Some(&b'a') {
            Some((MouthViseme::Wide, 1))
        } else if two.starts_with("uh") || two.starts_with("eh") || two.starts_with("er") {
            Some((MouthViseme::Small, 2))
        } else if matches!(two.as_bytes().first(), Some(b'e' | b'u' | b'r' | b's' | b'z' | b'k' | b'g' | b'c')) {
            Some((MouthViseme::Small, 1))
        } else {
            None
        };
        match matched {
            Some((viseme, len)) => {
                i += len;
                if out.last() != Some(&viseme) {
                    out.push(viseme);
                }
            }
            None => i += 1,
        }
    }
    if out.is_empty() {
        default
    } else {
        out
    }
}

/// Maps instantaneous RMS amplitude, mood, frame tick, FFT bands, and spoken
/// text into an animated `MouthViseme` (TS `amplitudeToViseme` parity).
pub fn amplitude_to_viseme_animated(
    amplitude: f32,
    mood: ExpressionMood,
    tick: u64,
    is_speaking: bool,
    bands: &[f32],
    spoken_text: &str,
) -> MouthViseme {
    if !is_speaking {
        return match mood {
            ExpressionMood::Happy => MouthViseme::Smile,
            ExpressionMood::Alert => MouthViseme::Small,
            ExpressionMood::Listening => MouthViseme::Rest,
            _ => MouthViseme::Closed,
        };
    }

    let clamped = amplitude.clamp(0.0, 1.0);
    let syllable_wave = (tick as f64 * 0.85).sin();
    let syllable_mod = ((syllable_wave + 1.0) * 0.11) as f32;
    let level = 1.0_f32.min(clamped + syllable_mod);

    if level < 0.07 {
        return MouthViseme::Closed;
    }
    if level < 0.22 {
        return MouthViseme::Small;
    }

    if !spoken_text.trim().is_empty() {
        let timeline = text_to_viseme_timeline(spoken_text);
        let syllable_step = tick / 3;
        let phoneme = timeline[(syllable_step as usize) % timeline.len()];

        if phoneme == MouthViseme::Mbp && (tick % 3 == 0 || clamped < 0.45) {
            return MouthViseme::Mbp;
        }
        if phoneme == MouthViseme::Mbp {
            return if clamped > 0.68 { MouthViseme::Mid } else { MouthViseme::Small };
        }
        if clamped < 0.14 && phoneme != MouthViseme::Fv {
            return if syllable_step % 3 == 0 { MouthViseme::Rest } else { MouthViseme::Small };
        }
        if bands.len() >= 10 {
            let low = (band_at(bands, 1) + band_at(bands, 2) + band_at(bands, 3)) / 3.0;
            let mid = (band_at(bands, 4) + band_at(bands, 5) + band_at(bands, 6)) / 3.0;
            let high = (band_at(bands, 7) + band_at(bands, 8) + band_at(bands, 9)) / 3.0;
            if high > mid * 1.18 && high > 0.28 {
                if matches!(phoneme, MouthViseme::Fv | MouthViseme::Lth | MouthViseme::Ee) {
                    return phoneme;
                }
                return if syllable_step % 2 == 0 { MouthViseme::Fv } else { MouthViseme::Ee };
            }
            if low > high * 1.25 && clamped > 0.52 {
                return if phoneme == MouthViseme::Round { MouthViseme::Round } else { MouthViseme::Wide };
            }
        }
        if clamped > 0.78 && matches!(phoneme, MouthViseme::Small | MouthViseme::Rest) {
            return MouthViseme::Wide;
        }
        return phoneme;
    }

    if bands.len() >= 10 {
        let low = (band_at(bands, 1) + band_at(bands, 2)) * 0.5;
        let high = (band_at(bands, 7) + band_at(bands, 8)) * 0.5;
        if clamped < 0.28 && tick % 5 == 0 {
            return MouthViseme::Mbp;
        }
        if high > low * 1.2 && high > 0.3 {
            return match tick % 3 {
                0 => MouthViseme::Fv,
                1 => MouthViseme::Lth,
                _ => MouthViseme::Ee,
            };
        }
    }

    if level < 0.44 {
        if tick % 4 == 0 {
            return MouthViseme::Round;
        }
        if tick % 3 == 0 {
            return MouthViseme::Small;
        }
        return MouthViseme::Mid;
    }
    if level < 0.72 {
        if tick % 5 == 0 {
            return MouthViseme::Round;
        }
        if tick % 3 == 1 {
            return MouthViseme::Wide;
        }
        return MouthViseme::Mid;
    }
    if tick % 6 == 0 {
        MouthViseme::Round
    } else {
        MouthViseme::Wide
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TaskStepItem;

    #[test]
    fn prioritizes_offline_then_approval_then_voice_then_working() {
        let mut inp = SidebarCharacterInput {
            connected: false,
            is_speaking: true,
            is_listening: false,
            active_channel_thinking: true,
            any_thinking: true,
            running_task_count: 2,
            pending_approval_count: 1,
            voice_amplitude: 0.6,
            active_task_progress: Some(50.0),
            active_task_title: Some("Build".into()),
            look_x: 0.2,
            look_y: -0.3,
        };
        assert_eq!(derive_sidebar_character_state(&inp).tone, SidebarWorkTone::Rose);

        inp.connected = true;
        let approval_state = derive_sidebar_character_state(&inp);
        assert_eq!(approval_state.mood, ExpressionMood::Alert);
        assert_eq!(approval_state.eye_state, EyeState::Wide);

        inp.pending_approval_count = 0;
        let speaking_state = derive_sidebar_character_state(&inp);
        assert_eq!(speaking_state.mood, ExpressionMood::Speaking);
        assert_eq!(speaking_state.tone, SidebarWorkTone::Emerald);

        inp.is_speaking = false;
        let working_state = derive_sidebar_character_state(&inp);
        assert_eq!(working_state.mood, ExpressionMood::Thinking);
        assert_eq!(working_state.tone, SidebarWorkTone::Amber);
    }

    // ---- Mirrored sidebarCharacterState.test.ts ----

    fn make_task() -> BackgroundTask {
        BackgroundTask {
            id: "task-1".into(),
            channel_id: "alpha".into(),
            session_id: String::new(),
            kind: None,
            background: None,
            title: "Run security audit across gateway routes".into(),
            status: TaskLifecycleStatus::Running,
            raw_status: None,
            progress: 45.0,
            progress_label: None,
            last_step: Some("Scanning /api/hermes handlers".into()),
            steps: vec![
                TaskStepItem { text: "Inspect routes".into(), done: true, active: false },
                TaskStepItem { text: "Scanning /api/hermes handlers".into(), done: false, active: true },
                TaskStepItem { text: "Write summary".into(), done: false, active: false },
            ],
            result: None,
            error_code: None,
            created_at_ms: Some(1_700_000_000_000),
            updated_at_ms: Some(1_700_000_015_000),
            agent_done_at_ms: None,
            confirmed_at_ms: None,
            updated_at: "live".into(),
        }
    }

    fn make_approval() -> ApprovalCard {
        ApprovalCard {
            id: "app-1".into(),
            channel_id: "alpha".into(),
            title: "Approve shell command rm -rf .cache".into(),
            action_type: "tool".into(),
            mcp_tool: None,
            run_id: None,
            request_id: None,
            execution_generation: None,
            kind: None,
            choices: vec![],
            command: None,
            destination: "local-shell".into(),
            summary: "Clear stale build cache".into(),
            draft: "rm -rf .cache".into(),
            status: ApprovalStatus::Pending,
            created_at: "just now".into(),
        }
    }

    fn online_gateway() -> SidebarGatewayStatus {
        SidebarGatewayStatus {
            connected: true,
            active_model: Some("hermes-agent".into()),
            profile: Some("agentis".into()),
        }
    }

    struct BaseInput {
        tasks: Vec<BackgroundTask>,
        approvals: Vec<ApprovalCard>,
        thinking: HashMap<String, bool>,
        sessions: Vec<GatewaySessionInfo>,
        gateway: SidebarGatewayStatus,
        bands: Vec<f32>,
    }

    impl BaseInput {
        fn new() -> Self {
            Self {
                tasks: vec![],
                approvals: vec![],
                thinking: HashMap::new(),
                sessions: vec![],
                gateway: online_gateway(),
                bands: vec![0.0, 0.0, 0.0, 0.0],
            }
        }

        fn snapshot(&self, active_channel: &str, home_draft: bool, speaking: bool, listening: bool, input_level: f32, output_level: f32, tick: u64, spoken: &str) -> SidebarCharacterSnapshot {
            derive_sidebar_character_snapshot(&SidebarStateInput {
                tasks: &self.tasks,
                approvals: &self.approvals,
                thinking_by_channel: &self.thinking,
                hermes_sessions: &self.sessions,
                active_channel,
                home_session_draft: home_draft,
                base_mood: ExpressionMood::Happy,
                is_speaking: speaking,
                is_listening: listening,
                input_level,
                output_level,
                frequency_bands: &self.bands,
                tick,
                gateway_status: Some(&self.gateway),
                last_spoken_line: spoken,
            })
        }
    }

    #[test]
    fn ts_happy_idle_online_state() {
        let base = BaseInput::new();
        let snap = base.snapshot("alpha", false, false, false, 0.0, 0.0, 0, "Standby");
        assert_eq!(snap.mode, SidebarCharacterMode::Idle);
        assert_eq!(snap.mood, ExpressionMood::Happy);
        assert_eq!(snap.viseme, MouthViseme::Smile);
        assert_eq!(snap.tone, SidebarWorkTone::Emerald);
        assert_eq!(snap.status_label, "online");
        assert_eq!(snap.gateway_line, "hermes-agent · agentis");
        assert!(snap.energy > 0.1);
        assert_eq!(snap.active_task, None);
    }

    #[test]
    fn ts_working_counts_extra_thinking_channels_without_double_count() {
        let mut base = BaseInput::new();
        let mut t_bg = make_task();
        t_bg.id = "t-bg".into();
        t_bg.channel_id = "deploy-ops".into();
        t_bg.title = "Deploy staging preview".into();
        t_bg.last_step = Some("Building docker bundle".into());
        let mut t_bg2 = make_task();
        t_bg2.id = "t-bg-2".into();
        t_bg2.channel_id = "qa-lane".into();
        t_bg2.title = "Verify regression suite".into();
        t_bg2.updated_at_ms = Some(1_700_000_020_000);
        base.tasks = vec![t_bg, t_bg2];
        base.thinking.insert("deploy-ops".into(), true); // already counted via t-bg
        base.thinking.insert("research".into(), true); // extra thinking channel

        let snap = base.snapshot("", true, false, false, 0.0, 0.0, 0, "Standby");
        assert_eq!(snap.mode, SidebarCharacterMode::Working);
        assert_eq!(snap.mood, ExpressionMood::Thinking);
        assert_eq!(snap.eye_state, SidebarEyeState::Side);
        assert_eq!(snap.tone, SidebarWorkTone::Amber);
        assert_eq!(snap.pulse_state, SidebarPulseState::Thinking);
        assert_eq!(snap.running_task_count, 3);
        assert_eq!(snap.status_label, "3 working");
        assert!(snap.energy >= 0.6);
        assert_eq!(snap.active_task.as_ref().map(|t| t.id.as_str()), Some("t-bg-2"));
    }

    #[test]
    fn ts_prefers_active_channel_task_and_surfaces_progress() {
        let mut base = BaseInput::new();
        let mut t_other = make_task();
        t_other.id = "t-other".into();
        t_other.channel_id = "other".into();
        t_other.updated_at_ms = Some(1_700_000_099_000);
        let mut t_alpha = make_task();
        t_alpha.id = "t-alpha".into();
        t_alpha.channel_id = "alpha".into();
        t_alpha.updated_at_ms = Some(1_700_000_010_000);
        base.tasks = vec![t_other, t_alpha];

        let snap = base.snapshot("alpha", false, false, false, 0.0, 0.0, 0, "Standby");
        assert_eq!(snap.mode, SidebarCharacterMode::Working);
        let active = snap.active_task.expect("active task");
        assert_eq!(active.id, "t-alpha");
        assert_eq!(active.step_text, "Scanning /api/hermes handlers");
        assert_eq!(active.ratio_label, "1/3");
        assert_eq!(active.progress_pct, 33);
    }

    #[test]
    fn ts_speaking_and_listening_priority_with_live_viseme_energy() {
        let mut base = BaseInput::new();
        base.tasks = vec![make_task()];
        base.bands = vec![0.6, 0.85, 0.72, 0.4];
        let speaking = base.snapshot("alpha", false, true, false, 0.0, 0.78, 3, "All tests are passing now.");
        assert_eq!(speaking.mode, SidebarCharacterMode::Speaking);
        assert_eq!(speaking.mood, ExpressionMood::Speaking);
        assert_eq!(speaking.tone, SidebarWorkTone::Emerald);
        assert_eq!(speaking.status_label, "speaking");
        assert_ne!(speaking.viseme, MouthViseme::Closed);
        assert!(speaking.energy >= 0.7);
        assert!(speaking.active_task.is_some());

        let listening = base.snapshot("alpha", false, false, true, 0.42, 0.0, 0, "Standby");
        assert_eq!(listening.mode, SidebarCharacterMode::Listening);
        assert_eq!(listening.mood, ExpressionMood::Listening);
        assert_eq!(listening.status_label, "listening");
        assert_eq!(listening.viseme, MouthViseme::Rest);
    }

    #[test]
    fn ts_attention_and_offline_states() {
        let mut base = BaseInput::new();
        base.approvals = vec![make_approval()];
        let attention = base.snapshot("alpha", false, false, false, 0.0, 0.0, 0, "Standby");
        assert_eq!(attention.mode, SidebarCharacterMode::Attention);
        assert_eq!(attention.mood, ExpressionMood::Alert);
        assert_eq!(attention.eye_state, SidebarEyeState::Wide);
        assert_eq!(attention.pulse_state, SidebarPulseState::Waiting);
        assert_eq!(attention.status_label, "1 approval");
        assert_eq!(
            attention.top_approval.as_ref().map(|a| a.title.as_str()),
            Some("Approve shell command rm -rf .cache")
        );

        base.gateway.connected = false;
        let offline = base.snapshot("alpha", false, false, false, 0.0, 0.0, 0, "Standby");
        // Approvals still outrank offline in the TS priority chain.
        assert_eq!(offline.mode, SidebarCharacterMode::Attention);

        base.approvals = vec![];
        let offline = base.snapshot("alpha", false, false, false, 0.0, 0.0, 0, "Standby");
        assert_eq!(offline.mode, SidebarCharacterMode::Offline);
        assert_eq!(offline.tone, SidebarWorkTone::Rose);
        assert_eq!(offline.pulse_state, SidebarPulseState::Error);
        assert_eq!(offline.status_label, "offline");
    }

    #[test]
    fn ts_pointer_look_target_normalizes_gaze() {
        let (x, y) = compute_sidebar_pointer_look_target(12.0, 60.0, 270.0, 84.0, 260.0, 90.0, 44.0);
        assert!(x > 0.6 && x <= 1.0);
        assert!(y.abs() <= 1.0);
    }

    #[test]
    fn ts_viseme_timeline_and_animated_engine() {
        let timeline = text_to_viseme_timeline("All tests are passing now.");
        assert!(!timeline.is_empty());
        assert_eq!(text_to_viseme_timeline("   "), vec![MouthViseme::Small, MouthViseme::Mid, MouthViseme::Wide, MouthViseme::Round]);

        // Non-speaking moods map to static visemes.
        assert_eq!(amplitude_to_viseme_animated(0.9, ExpressionMood::Happy, 0, false, &[], ""), MouthViseme::Smile);
        assert_eq!(amplitude_to_viseme_animated(0.9, ExpressionMood::Alert, 0, false, &[], ""), MouthViseme::Small);
        assert_eq!(amplitude_to_viseme_animated(0.9, ExpressionMood::Listening, 0, false, &[], ""), MouthViseme::Rest);
        assert_eq!(amplitude_to_viseme_animated(0.9, ExpressionMood::Calm, 0, false, &[], ""), MouthViseme::Closed);

        // Speaking with strong energy never returns closed.
        let v = amplitude_to_viseme_animated(0.78, ExpressionMood::Speaking, 3, true, &[0.6, 0.85, 0.72, 0.4], "All tests are passing now.");
        assert_ne!(v, MouthViseme::Closed);
    }
}