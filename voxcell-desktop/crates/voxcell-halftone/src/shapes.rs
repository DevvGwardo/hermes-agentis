use crate::portrait::{clamp01, evaluate_natural_blink_curve};
use crate::raster::{
    BAYER_8X8, BrailleFrameGrid, parse_hex_rgb, rasterize_tonal_grid_rgba, resolve_shape_tone_palette,
};
use std::f32::consts::PI;
use voxcell_core::canvas::CanvasShapeKind;
use voxcell_core::{DotProfile, ExpressionMood, MouthViseme};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IdleAnimationSet {
    Tactician,
    Celestial,
    Scholar,
    Idol,
    Neko,
    Trickster,
}

pub fn resolve_idle_set(shape: CanvasShapeKind) -> IdleAnimationSet {
    match shape {
        CanvasShapeKind::HermesOrb | CanvasShapeKind::MechaHelm => IdleAnimationSet::Tactician,
        CanvasShapeKind::AngelBot => IdleAnimationSet::Celestial,
        CanvasShapeKind::GhostShell => IdleAnimationSet::Scholar,
        CanvasShapeKind::LotusCore => IdleAnimationSet::Idol,
        CanvasShapeKind::CatBot | CanvasShapeKind::CyberSkull => IdleAnimationSet::Neko,
        CanvasShapeKind::FoxSpirit | CanvasShapeKind::PrismCore => IdleAnimationSet::Trickster,
    }
}

pub fn default_shape_tints(shape: CanvasShapeKind) -> (&'static str, &'static str, &'static str) {
    match shape {
        CanvasShapeKind::HermesOrb => ("#4D7C0F", "#84CC16", "#ECFCCB"),
        CanvasShapeKind::LotusCore => ("#BE185D", "#F472B6", "#FCE7F3"),
        CanvasShapeKind::CatBot => ("#7C3AED", "#C4B5FD", "#F5F3FF"),
        CanvasShapeKind::FoxSpirit => ("#C2410C", "#FB7185", "#FFE4E6"),
        CanvasShapeKind::PrismCore => ("#C2410C", "#FB923C", "#FFEDD5"),
        CanvasShapeKind::MechaHelm => ("#0284C7", "#38BDF8", "#E0F2FE"),
        CanvasShapeKind::GhostShell => ("#4F46E5", "#A5B4FC", "#EEF2FF"),
        CanvasShapeKind::AngelBot => ("#B45309", "#FBBF24", "#FEF3C7"),
        CanvasShapeKind::CyberSkull => ("#9D174D", "#EC4899", "#FCE7F3"),
    }
}

#[inline]
fn smin(a: f32, b: f32, k: f32) -> f32 {
    if a + k <= b {
        return a;
    }
    if b + k <= a {
        return b;
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b * (1.0 - h) + a * h - k * h * (1.0 - h)
}

#[inline]
fn sd_circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r
}

#[inline]
fn sd_ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let dx = (x - cx) / rx;
    let dy = (y - cy) / ry;
    ((dx * dx + dy * dy).sqrt() - 1.0) * rx.min(ry)
}

#[inline]
fn sd_capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = (bax * bax + bay * bay).max(1e-6);
    let h = ((pax * bax + pay * bay) / denom).clamp(0.0, 1.0);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt() - r
}

#[inline]
fn sd_triangle(px: f32, py: f32, p0x: f32, p0y: f32, p1x: f32, p1y: f32, p2x: f32, p2y: f32) -> f32 {
    let e0x = p1x - p0x;
    let e0y = p1y - p0y;
    let e1x = p2x - p1x;
    let e1y = p2y - p1y;
    let e2x = p0x - p2x;
    let e2y = p0y - p2y;

    let v0x = px - p0x;
    let v0y = py - p0y;
    let v1x = px - p1x;
    let v1y = py - p1y;
    let v2x = px - p2x;
    let v2y = py - p2y;

    let h0 = ((v0x * e0x + v0y * e0y) / (e0x * e0x + e0y * e0y).max(1e-6)).clamp(0.0, 1.0);
    let h1 = ((v1x * e1x + v1y * e1y) / (e1x * e1x + e1y * e1y).max(1e-6)).clamp(0.0, 1.0);
    let h2 = ((v2x * e2x + v2y * e2y) / (e2x * e2x + e2y * e2y).max(1e-6)).clamp(0.0, 1.0);

    let d0 = (v0x - e0x * h0).powi(2) + (v0y - e0y * h0).powi(2);
    let d1 = (v1x - e1x * h1).powi(2) + (v1y - e1y * h1).powi(2);
    let d2 = (v2x - e2x * h2).powi(2) + (v2y - e2y * h2).powi(2);

    let s = (e0x * e2y - e0y * e2x).signum();
    let s0 = s * (v0x * e0y - v0y * e0x);
    let s1 = s * (v1x * e1y - v1y * e1x);
    let s2 = s * (v2x * e2y - v2y * e2x);

    let min_d = d0.min(d1).min(d2);
    let min_s = s0.min(s1).min(s2);
    -min_d.sqrt() * min_s.signum()
}

// ---------------------------------------------------------------------------
// brailleIdleLoops.ts — 6 animation sets x 6 separated actions = 36 idle loops
// ---------------------------------------------------------------------------

pub const IDLE_ACTION_SLOT_SECONDS: f32 = 4.0;
pub const IDLE_ACTIVE_END_SECONDS: f32 = 3.15;
pub const IDLE_EASE_IN_SECONDS: f32 = 0.45;
pub const IDLE_EASE_OUT_START_SECONDS: f32 = 2.65;
pub const IDLE_ACTIONS_PER_SET: usize = 6;
pub const IDLE_FULL_LOOP_SECONDS: f32 = IDLE_ACTION_SLOT_SECONDS * IDLE_ACTIONS_PER_SET as f32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleMouthMode {
    Default,
    Smile,
    Cat,
    Hum,
    Yawn,
    Grin,
    O,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdleHandPose {
    None,
    ChinTap,
    Salute,
    Clasp,
    Tome,
    Mug,
    Peace,
    Cheer,
    PawSwipe,
    Peek,
    Wave,
    Juggle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdlePropKind {
    None,
    Radar,
    Orbit,
    Sparkle,
    Steam,
    Bubbles,
    Note,
    Orb,
    Heart,
    Zzz,
}

#[derive(Clone, Copy, Debug)]
pub struct IdleActionDef {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct IdleAnimationSetMeta {
    pub id: IdleAnimationSet,
    pub label: &'static str,
    pub actions: [IdleActionDef; 6],
}

pub const IDLE_SET_ORDER: [IdleAnimationSet; 6] = [
    IdleAnimationSet::Tactician,
    IdleAnimationSet::Celestial,
    IdleAnimationSet::Scholar,
    IdleAnimationSet::Idol,
    IdleAnimationSet::Neko,
    IdleAnimationSet::Trickster,
];

static IDLE_ANIMATION_SETS: [IdleAnimationSetMeta; 6] = [
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Tactician,
        label: "Cyber Tactician",
        actions: [
            IdleActionDef { id: "radar-scan", label: "Radar Scan" },
            IdleActionDef { id: "chin-tap", label: "Chin Tap" },
            IdleActionDef { id: "visor-adjust", label: "Headset Adjust" },
            IdleActionDef { id: "affirm-nod", label: "Affirmative Nod" },
            IdleActionDef { id: "signal-wave", label: "Signal Wave" },
            IdleActionDef { id: "calm-wink", label: "Calm Wink" },
        ],
    },
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Celestial,
        label: "Celestial Seraph",
        actions: [
            IdleActionDef { id: "halo-spin", label: "Halo Orbit" },
            IdleActionDef { id: "halo-polish", label: "Halo Polish" },
            IdleActionDef { id: "wing-flutter", label: "Wing Flutter" },
            IdleActionDef { id: "serene-pray", label: "Serene Clasp" },
            IdleActionDef { id: "blessing-sparkle", label: "Blessing Sparkle" },
            IdleActionDef { id: "sleepy-sway", label: "Dreamy Sway" },
        ],
    },
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Scholar,
        label: "Cozy Scholar",
        actions: [
            IdleActionDef { id: "hood-snuggle", label: "Hood Snuggle" },
            IdleActionDef { id: "drowsy-yawn", label: "Drowsy Yawn" },
            IdleActionDef { id: "page-turn", label: "Tome Flip" },
            IdleActionDef { id: "thought-bubble", label: "Thought Bubbles" },
            IdleActionDef { id: "head-nod-doze", label: "Sleepy Doze" },
            IdleActionDef { id: "warm-sip", label: "Warm Cocoa Sip" },
        ],
    },
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Idol,
        label: "Starlight Idol",
        actions: [
            IdleActionDef { id: "twintail-bounce", label: "Twintail Bounce" },
            IdleActionDef { id: "idol-peace", label: "Idol Peace Wink" },
            IdleActionDef { id: "star-cheer", label: "Star Cheer" },
            IdleActionDef { id: "mic-hum", label: "Melody Hum" },
            IdleActionDef { id: "ahoge-boing", label: "Ahoge Boing" },
            IdleActionDef { id: "curtsy-bow", label: "Stage Bow" },
        ],
    },
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Neko,
        label: "Stealth Neko",
        actions: [
            IdleActionDef { id: "ear-flick", label: "Radar Ear Flick" },
            IdleActionDef { id: "paw-swipe", label: "Playful Paw" },
            IdleActionDef { id: "tail-swish", label: "Tail Swish" },
            IdleActionDef { id: "prowl-crouch", label: "Stealth Prowl" },
            IdleActionDef { id: "purr-curl", label: "Happy Purr" },
            IdleActionDef { id: "ear-groom", label: "Ear Groom" },
        ],
    },
    IdleAnimationSetMeta {
        id: IdleAnimationSet::Trickster,
        label: "Spirit Trickster",
        actions: [
            IdleActionDef { id: "floppy-ear-hop", label: "Bounce Hop" },
            IdleActionDef { id: "kitsune-peek", label: "Sleeve Peek" },
            IdleActionDef { id: "orb-juggle", label: "Spirit Orbs" },
            IdleActionDef { id: "cheek-puff", label: "Cheek Pout" },
            IdleActionDef { id: "spin-perk", label: "Joyful Perk" },
            IdleActionDef { id: "wave-hello", label: "Friendly Wave" },
        ],
    },
];

pub fn idle_set_meta(set: IdleAnimationSet) -> &'static IdleAnimationSetMeta {
    &IDLE_ANIMATION_SETS[set as usize]
}

/// Full per-action pose deltas (mirrors `IdlePoseDeltas` in brailleIdleLoops.ts).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IdlePoseDeltas {
    pub head_x: f32,
    pub head_y: f32,
    pub tilt: f32,
    pub gaze_x: f32,
    pub gaze_y: f32,
    pub blink_left: f32,
    pub blink_right: f32,
    pub happy_eyes: f32,
    pub wide_eyes: f32,
    pub squint_eyes: f32,
    pub mouth_mode: IdleMouthMode,
    pub ear_left: f32,
    pub ear_right: f32,
    pub tail_wag: f32,
    pub hair_sway: f32,
    pub halo_lift: f32,
    pub halo_spin: f32,
    pub wing_open: f32,
    pub ahoge_bounce: f32,
    pub hood_lift: f32,
    pub hood_scale_x: f32,
    pub squash_y: f32,
    pub hand: IdleHandPose,
    pub hand_phase: f32,
    pub prop: IdlePropKind,
    pub prop_phase: f32,
}

pub const NEUTRAL_IDLE_POSE: IdlePoseDeltas = IdlePoseDeltas {
    head_x: 0.0,
    head_y: 0.0,
    tilt: 0.0,
    gaze_x: 0.0,
    gaze_y: 0.0,
    blink_left: 0.0,
    blink_right: 0.0,
    happy_eyes: 0.0,
    wide_eyes: 0.0,
    squint_eyes: 0.0,
    mouth_mode: IdleMouthMode::Default,
    ear_left: 0.0,
    ear_right: 0.0,
    tail_wag: 0.0,
    hair_sway: 0.0,
    halo_lift: 0.0,
    halo_spin: 0.0,
    wing_open: 0.0,
    ahoge_bounce: 0.0,
    hood_lift: 0.0,
    hood_scale_x: 0.0,
    squash_y: 0.0,
    hand: IdleHandPose::None,
    hand_phase: 0.0,
    prop: IdlePropKind::None,
    prop_phase: 0.0,
};

impl Default for IdlePoseDeltas {
    fn default() -> Self {
        NEUTRAL_IDLE_POSE
    }
}

/// Back-compat alias: the SDF renderer consumes the full TS pose deltas.
pub type IdlePose = IdlePoseDeltas;

#[derive(Clone, Copy, Debug)]
pub struct IdleActionFrame {
    pub set_id: IdleAnimationSet,
    pub action_index: usize,
    pub action_id: &'static str,
    pub action_label: &'static str,
    pub is_resting: bool,
    pub envelope: f32,
    pub phase: f32,
    pub pose: IdlePoseDeltas,
}

#[inline]
fn smootherstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let span = edge1 - edge0;
    let t = clamp01((x - edge0) / if span == 0.0 { 1.0 } else { span });
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// Shapes normalized phase [0..1] into a smooth anticipation-and-rebound curve:
/// brief counter-dip (0.00..0.13), expressive peak, and gentle damped settle.
fn shaped_bounce_phase(phase: f32) -> f32 {
    if phase < 0.14 {
        let t = phase / 0.14;
        return -0.22 * (t * PI).sin();
    }
    let active = (phase - 0.14) / 0.86;
    (active * PI * 2.0).sin() * (1.0 - 0.12 * active)
}

fn build_pose_for_action(set: IdleAnimationSet, action_index: usize, env: f32, phase: f32) -> IdlePoseDeltas {
    let mut pose = NEUTRAL_IDLE_POSE;
    if env <= 0.001 {
        return pose;
    }
    let s = (phase * PI * 2.0).sin();
    let s2 = (phase * PI * 4.0).sin();
    let bell = (phase * PI).sin();
    let shaped = shaped_bounce_phase(phase);

    match set {
        IdleAnimationSet::Tactician => match action_index {
            0 => {
                // radar-scan
                pose.gaze_x = s * 0.78 * env;
                pose.gaze_y = (-0.18 + (phase * PI * 2.0).cos() * 0.06) * env;
                pose.tilt = -s * 0.052 * env;
                pose.hair_sway = s * 0.03 * env;
                pose.prop = IdlePropKind::Radar;
                pose.prop_phase = phase;
            }
            1 => {
                // chin-tap
                pose.tilt = -0.105 * env;
                pose.head_y = (-0.014 + s2.abs() * 0.008) * env;
                pose.gaze_x = -0.28 * env;
                pose.gaze_y = -0.26 * env;
                pose.squint_eyes = 0.28 * env;
                pose.hand = IdleHandPose::ChinTap;
                pose.hand_phase = s2.abs() * env;
            }
            2 => {
                // visor-adjust
                pose.tilt = 0.072 * env;
                pose.gaze_x = 0.24 * env;
                pose.hand = IdleHandPose::Salute;
                pose.hand_phase = bell * env;
            }
            3 => {
                // affirm-nod
                pose.head_y = (0.048 * shaped.max(0.0) - 0.012 * bell) * env;
                pose.squash_y = -0.025 * shaped.max(0.0) * env;
                pose.happy_eyes = 0.7 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
            }
            4 => {
                // signal-wave
                pose.tilt = (-0.062 + s2 * 0.015) * env;
                pose.happy_eyes = 0.52 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.hand = IdleHandPose::Wave;
                pose.hand_phase = phase;
            }
            _ => {
                // calm-wink
                pose.tilt = 0.085 * bell * env;
                pose.blink_right = (bell * 1.15).min(1.0) * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.prop = IdlePropKind::Sparkle;
                pose.prop_phase = phase;
            }
        },
        IdleAnimationSet::Celestial => match action_index {
            0 => {
                // halo-spin
                pose.halo_lift = 0.062 * bell * env;
                pose.halo_spin = s2 * 0.14 * env;
                pose.gaze_y = -0.2 * env;
                pose.prop = IdlePropKind::Orbit;
                pose.prop_phase = phase;
            }
            1 => {
                // halo-polish
                pose.tilt = -0.078 * env;
                pose.halo_lift = 0.03 * (phase * PI * 3.0).sin() * env;
                pose.happy_eyes = 0.6 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.hand = IdleHandPose::Salute;
                pose.hand_phase = phase;
            }
            2 => {
                // wing-flutter
                pose.head_y = -0.038 * bell * env;
                pose.wing_open = (0.68 + 0.32 * s2.abs()) * env;
                pose.hair_sway = s2 * 0.052 * env;
                pose.squash_y = (phase * PI * 4.0).sin() * 0.02 * env;
            }
            3 => {
                // serene-pray
                pose.head_y = 0.018 * bell * env;
                pose.blink_left = 0.88 * env;
                pose.blink_right = 0.88 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.hand = IdleHandPose::Clasp;
                pose.hand_phase = env;
            }
            4 => {
                // blessing-sparkle
                pose.tilt = 0.068 * env;
                pose.happy_eyes = 0.85 * env;
                pose.halo_lift = 0.045 * bell * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.prop = IdlePropKind::Sparkle;
                pose.prop_phase = phase;
            }
            _ => {
                // sleepy-sway
                pose.tilt = s * 0.105 * env;
                pose.squint_eyes = 0.78 * env;
                pose.halo_lift = 0.024 * s * env;
                pose.hair_sway = -s * 0.072 * env;
            }
        },
        IdleAnimationSet::Scholar => match action_index {
            0 => {
                // hood-snuggle
                pose.head_y = 0.028 * bell * env;
                pose.hood_lift = -0.034 * bell * env;
                pose.hood_scale_x = 0.052 * bell * env;
                pose.squint_eyes = 0.6 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
            }
            1 => {
                // drowsy-yawn
                pose.tilt = -0.062 * bell * env;
                pose.blink_left = 0.78 * env;
                pose.blink_right = 0.78 * env;
                pose.mouth_mode = IdleMouthMode::Yawn;
                pose.prop = IdlePropKind::Zzz;
                pose.prop_phase = phase;
            }
            2 => {
                // page-turn
                pose.gaze_x = s * 0.38 * env;
                pose.gaze_y = 0.34 * env;
                pose.hand = IdleHandPose::Tome;
                pose.hand_phase = phase;
            }
            3 => {
                // thought-bubble
                pose.tilt = 0.088 * env;
                pose.gaze_x = 0.36 * env;
                pose.gaze_y = -0.36 * env;
                pose.prop = IdlePropKind::Bubbles;
                pose.prop_phase = phase;
            }
            4 => {
                // head-nod-doze
                pose.head_y = (if phase < 0.68 {
                    smootherstep(0.0, 0.68, phase) * 0.085
                } else {
                    0.058 - (phase - 0.68) * 0.20
                }) * env;
                pose.tilt = 0.068 * env;
                pose.blink_left = (if phase < 0.70 { 0.94 } else { 0.12 }) * env;
                pose.blink_right = (if phase < 0.70 { 0.94 } else { 0.12 }) * env;
                pose.wide_eyes = if phase >= 0.70 { 0.72 * env } else { 0.0 };
            }
            _ => {
                // warm-sip
                pose.head_y = 0.015 * bell * env;
                pose.happy_eyes = 0.76 * env;
                pose.mouth_mode = IdleMouthMode::Hum;
                pose.hand = IdleHandPose::Mug;
                pose.hand_phase = env;
                pose.prop = IdlePropKind::Steam;
                pose.prop_phase = phase;
            }
        },
        IdleAnimationSet::Idol => match action_index {
            0 => {
                // twintail-bounce
                pose.head_y = (-0.052 * s2.abs() + if phase < 0.12 { 0.018 } else { 0.0 }) * env;
                pose.hair_sway = shaped * 0.125 * env;
                pose.squash_y = s2 * 0.048 * env;
                pose.happy_eyes = 0.7 * env;
                pose.mouth_mode = IdleMouthMode::Grin;
            }
            1 => {
                // idol-peace
                pose.tilt = -0.092 * env;
                pose.blink_left = 0.94 * env;
                pose.mouth_mode = IdleMouthMode::Grin;
                pose.hand = IdleHandPose::Peace;
                pose.hand_phase = env;
                pose.prop = IdlePropKind::Sparkle;
                pose.prop_phase = phase;
            }
            2 => {
                // star-cheer
                pose.head_y = (-0.044 * s2.abs() + if phase < 0.12 { 0.015 } else { 0.0 }) * env;
                pose.wide_eyes = 0.6 * env;
                pose.mouth_mode = IdleMouthMode::Grin;
                pose.hand = IdleHandPose::Cheer;
                pose.hand_phase = phase;
                pose.prop = IdlePropKind::Sparkle;
                pose.prop_phase = phase;
            }
            3 => {
                // mic-hum
                pose.tilt = s * 0.082 * env;
                pose.happy_eyes = 0.75 * env;
                pose.mouth_mode = IdleMouthMode::Hum;
                pose.prop = IdlePropKind::Note;
                pose.prop_phase = phase;
            }
            4 => {
                // ahoge-boing
                pose.ahoge_bounce = shaped * 0.11 * env;
                pose.wide_eyes = 0.5 * env;
                pose.mouth_mode = IdleMouthMode::O;
            }
            _ => {
                // curtsy-bow
                pose.head_y = 0.056 * bell * env;
                pose.squash_y = -0.032 * bell * env;
                pose.happy_eyes = 0.92 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.hair_sway = 0.058 * bell * env;
            }
        },
        IdleAnimationSet::Neko => match action_index {
            0 => {
                // ear-flick
                pose.ear_left = (if phase < 0.45 { (phase * PI * 5.0).sin() * 0.2 } else { 0.0 }) * env;
                pose.ear_right = (if phase >= 0.45 { ((phase - 0.45) * PI * 5.0).sin() * 0.2 } else { 0.0 }) * env;
                pose.gaze_x = (if phase < 0.45 { -0.48 } else { 0.48 }) * env;
                pose.mouth_mode = IdleMouthMode::Cat;
            }
            1 => {
                // paw-swipe
                pose.tilt = -0.072 * env;
                pose.mouth_mode = IdleMouthMode::Cat;
                pose.hand = IdleHandPose::PawSwipe;
                pose.hand_phase = phase;
            }
            2 => {
                // tail-swish
                pose.tail_wag = s * 1.0 * env;
                pose.mouth_mode = IdleMouthMode::Cat;
                pose.happy_eyes = 0.5 * env;
            }
            3 => {
                // prowl-crouch
                pose.head_y = 0.052 * bell * env;
                pose.squash_y = -0.052 * bell * env;
                pose.squint_eyes = 0.68 * env;
                pose.gaze_x = s * 0.58 * env;
                pose.mouth_mode = IdleMouthMode::Cat;
            }
            4 => {
                // purr-curl
                pose.happy_eyes = 0.96 * env;
                pose.tilt = 0.062 * s * env;
                pose.mouth_mode = IdleMouthMode::Cat;
                pose.prop = IdlePropKind::Heart;
                pose.prop_phase = phase;
            }
            _ => {
                // ear-groom
                pose.tilt = -0.112 * env;
                pose.blink_left = 0.85 * env;
                pose.ear_left = -0.14 * env;
                pose.mouth_mode = IdleMouthMode::Cat;
                pose.hand = IdleHandPose::PawSwipe;
                pose.hand_phase = 0.5 + 0.3 * s;
            }
        },
        IdleAnimationSet::Trickster => match action_index {
            0 => {
                // floppy-ear-hop
                pose.head_y = (-0.062 * s2.abs() + if phase < 0.12 { 0.02 } else { 0.0 }) * env;
                pose.ear_left = shaped * 0.18 * env;
                pose.ear_right = -shaped * 0.18 * env;
                pose.squash_y = s2 * 0.048 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
            }
            1 => {
                // kitsune-peek
                pose.tilt = 0.088 * bell * env;
                pose.happy_eyes = 0.6 * env;
                pose.hand = IdleHandPose::Peek;
                pose.hand_phase = env;
            }
            2 => {
                // orb-juggle
                pose.gaze_y = -0.18 * env;
                pose.mouth_mode = IdleMouthMode::Smile;
                pose.hand = IdleHandPose::Juggle;
                pose.hand_phase = phase;
                pose.prop = IdlePropKind::Orb;
                pose.prop_phase = phase;
            }
            3 => {
                // cheek-puff
                pose.tilt = -0.056 * bell * env;
                pose.squint_eyes = 0.48 * env;
                pose.mouth_mode = IdleMouthMode::O;
                pose.hood_scale_x = 0.036 * env;
            }
            4 => {
                // spin-perk
                pose.head_x = s * 0.04 * env;
                pose.tilt = s * 0.105 * env;
                pose.hair_sway = -s * 0.095 * env;
                pose.tail_wag = s * 0.85 * env;
                pose.happy_eyes = 0.85 * env;
            }
            _ => {
                // wave-hello
                pose.tilt = (-0.068 + s2 * 0.015) * env;
                pose.happy_eyes = 0.88 * env;
                pose.mouth_mode = IdleMouthMode::Grin;
                pose.hand = IdleHandPose::Wave;
                pose.hand_phase = phase;
            }
        },
    }
    pose
}

/// Evaluates a character's 6-action separated idle animation loop at `offset_time`.
/// Each action occupies a 4.0s window: 0.00-3.15s active motion + 3.15-4.00s calm neutral rest.
pub fn evaluate_idle_action_step(set: IdleAnimationSet, offset_time: f32, reduced_motion: bool) -> IdleActionFrame {
    let meta = idle_set_meta(set);
    let safe_time = if offset_time.is_finite() { offset_time } else { 0.0 };
    let wrapped = safe_time.rem_euclid(IDLE_FULL_LOOP_SECONDS);
    let action_index = ((wrapped / IDLE_ACTION_SLOT_SECONDS).floor() as usize).min(IDLE_ACTIONS_PER_SET - 1);
    let local_t = wrapped - action_index as f32 * IDLE_ACTION_SLOT_SECONDS;
    let action_def = meta.actions[action_index];

    if reduced_motion || local_t >= IDLE_ACTIVE_END_SECONDS {
        return IdleActionFrame {
            set_id: meta.id,
            action_index,
            action_id: action_def.id,
            action_label: action_def.label,
            is_resting: true,
            envelope: 0.0,
            phase: 0.0,
            pose: NEUTRAL_IDLE_POSE,
        };
    }

    let envelope = if local_t < IDLE_EASE_IN_SECONDS {
        smootherstep(0.0, IDLE_EASE_IN_SECONDS, local_t)
    } else if local_t > IDLE_EASE_OUT_START_SECONDS {
        1.0 - smootherstep(IDLE_EASE_OUT_START_SECONDS, IDLE_ACTIVE_END_SECONDS, local_t)
    } else {
        1.0
    };

    let phase = clamp01(local_t / IDLE_ACTIVE_END_SECONDS);
    let pose = build_pose_for_action(meta.id, action_index, envelope, phase);
    IdleActionFrame {
        set_id: meta.id,
        action_index,
        action_id: action_def.id,
        action_label: action_def.label,
        is_resting: false,
        envelope,
        phase,
        pose,
    }
}

/// Idle pose for the SDF renderer — projection of the full 36-action TS idle loop.
pub fn evaluate_idle_pose(shape: CanvasShapeKind, time: f32, reduced_motion: bool) -> IdlePose {
    evaluate_idle_action_step(resolve_idle_set(shape), time, reduced_motion).pose
}

pub fn evaluate_shape_sdf(shape: CanvasShapeKind, x: f32, y: f32, pose: &IdlePose) -> f32 {
    match shape {
        CanvasShapeKind::HermesOrb => {
            let crown = if y < 0.38 { sd_ellipse(x, y, 0.04, -0.18, 0.48, 0.47) } else { 1.0 };
            let back = if x > -0.12 { sd_capsule(x, y, 0.28, -0.10, 0.44, 0.54, 0.21) } else { 1.0 };
            let side_l = if x < 0.0 { sd_capsule(x, y, -0.34, -0.05, -0.44, 0.48, 0.145) } else { 1.0 };
            let curl_l = if y > 0.20 && x < -0.18 { sd_capsule(x, y, -0.44, 0.46, -0.63, 0.39, 0.095) } else { 1.0 };
            let curl_r = if y > 0.22 && x > 0.18 { sd_capsule(x, y, 0.44, 0.52, 0.61, 0.41, 0.095) } else { 1.0 };
            let chin = if (-0.10..0.52).contains(&y) { sd_ellipse(x, y, -0.08, 0.20, 0.25, 0.22) } else { 1.0 };
            let hair = smin(smin(crown, back, 0.08), smin(smin(side_l, curl_l, 0.07), curl_r, 0.07), 0.08);
            if y < 0.22 {
                return smin(hair, chin, 0.05);
            }
            let neck = if y < 0.68 && x > -0.22 && x < 0.24 { sd_capsule(x, y, 0.0, 0.33, 0.01, 0.58, 0.092) } else { 1.0 };
            let bust = if y > 0.42 { sd_ellipse(x, y, 0.02, 0.67, 0.37, 0.16) } else { 1.0 };
            smin(smin(hair, chin, 0.05), smin(neck, bust, 0.06), 0.05)
        }
        CanvasShapeKind::LotusCore => {
            let crown = if y < 0.35 && x.abs() < 0.56 { sd_ellipse(x, y, 0.0, -0.15, 0.44, 0.42) } else { 1.0 };
            let chin = if (-0.10..0.50).contains(&y) { sd_ellipse(x, y, 0.04, 0.20, 0.24, 0.215) } else { 1.0 };
            let ahoge = if y < -0.32 && x > -0.18 && x < 0.30 { sd_capsule(x, y, -0.02, -0.55, 0.14, -0.74 - pose.ahoge_bounce, 0.052) } else { 1.0 };
            let tail_l = if x < -0.15 && y > -0.32 {
                smin(
                    sd_capsule(x, y, -0.41, -0.12, -0.66, 0.33, 0.132),
                    if y > 0.14 { sd_capsule(x, y, -0.66, 0.33, -0.73, 0.54, 0.098) } else { 1.0 },
                    0.07,
                )
            } else { 1.0 };
            let tail_r = if x > 0.15 && y > -0.32 {
                smin(
                    sd_capsule(x, y, 0.41, -0.12, 0.66, 0.33, 0.132),
                    if y > 0.14 { sd_capsule(x, y, 0.66, 0.33, 0.73, 0.54, 0.098) } else { 1.0 },
                    0.07,
                )
            } else { 1.0 };
            let head = smin(smin(crown, chin, 0.05), ahoge, 0.05);
            let tails = tail_l.min(tail_r);
            if y < 0.22 {
                return smin(head, tails, 0.06);
            }
            let neck = if y < 0.70 && x.abs() < 0.22 { sd_capsule(x, y, 0.01, 0.33, 0.0, 0.57, 0.09) } else { 1.0 };
            let bust = if y > 0.40 { sd_ellipse(x, y, 0.0, 0.67, 0.36, 0.155) } else { 1.0 };
            smin(smin(head, smin(neck, bust, 0.06), 0.05), tails, 0.06)
        }
        CanvasShapeKind::CatBot => {
            let crown = if y < 0.38 { sd_ellipse(x, y, 0.03, -0.10, 0.45, 0.41) } else { 1.0 };
            let lock_l = if x < -0.05 && y > -0.15 { sd_capsule(x, y, -0.34, 0.04, -0.44, 0.54, 0.125) } else { 1.0 };
            let lock_r = if x > 0.05 && y > -0.15 { sd_capsule(x, y, 0.38, 0.02, 0.48, 0.55, 0.132) } else { 1.0 };
            let chin = if (-0.08..0.52).contains(&y) { sd_ellipse(x, y, -0.06, 0.22, 0.24, 0.215) } else { 1.0 };
            let mut core = smin(smin(crown, chin, 0.05), lock_l.min(lock_r), 0.06);
            if y >= 0.22 {
                let neck = if y < 0.70 { sd_capsule(x, y, 0.0, 0.35, 0.01, 0.58, 0.092) } else { 1.0 };
                let bust = if y > 0.40 { sd_ellipse(x, y, 0.01, 0.67, 0.36, 0.155) } else { 1.0 };
                core = smin(core, smin(neck, bust, 0.06), 0.05);
            }
            if y > -0.05 {
                return core;
            }
            let ear_l = if x < 0.02 {
                sd_triangle(x, y, -0.44, -0.22, -0.46 + pose.ear_left * 0.24, -0.71 + pose.ear_left.abs() * 0.18, -0.14, -0.42) - 0.03
            } else { 1.0 };
            let ear_r = if x > -0.02 {
                sd_triangle(x, y, 0.46, -0.20, 0.16, -0.40, 0.48 - pose.ear_right * 0.24, -0.71 + pose.ear_right.abs() * 0.18) - 0.03
            } else { 1.0 };
            smin(core, ear_l.min(ear_r), 0.04)
        }
        CanvasShapeKind::FoxSpirit => {
            let crown = if y < 0.38 { sd_ellipse(x, y, -0.03, -0.08, 0.44, 0.40) } else { 1.0 };
            let lock_l = if x < -0.05 && y > -0.15 { sd_capsule(x, y, -0.40, 0.04, -0.52, 0.55, 0.125) } else { 1.0 };
            let lock_r = if x > 0.05 && y > -0.15 { sd_capsule(x, y, 0.36, 0.06, 0.48, 0.54, 0.125) } else { 1.0 };
            let chin = if (-0.08..0.52).contains(&y) { sd_ellipse(x, y, 0.07, 0.225, 0.24, 0.215) } else { 1.0 };
            let mut core = smin(smin(crown, chin, 0.05), lock_l.min(lock_r), 0.06);
            if y >= 0.22 {
                let neck = if y < 0.70 { sd_capsule(x, y, 0.0, 0.35, -0.01, 0.58, 0.092) } else { 1.0 };
                let bust = if y > 0.40 { sd_ellipse(x, y, -0.01, 0.67, 0.36, 0.155) } else { 1.0 };
                core = smin(core, smin(neck, bust, 0.06), 0.05);
            }
            if y > 0.0 {
                return core;
            }
            let ear_l = if x < 0.04 { sd_triangle(x, y, -0.44, -0.14, -0.54 + pose.ear_left * 0.2, -0.73, -0.12, -0.36) - 0.03 } else { 1.0 };
            let ear_r = if x > -0.04 { sd_triangle(x, y, 0.42, -0.16, 0.10, -0.36, 0.52 - pose.ear_right * 0.2, -0.73) - 0.03 } else { 1.0 };
            smin(core, ear_l.min(ear_r), 0.04)
        }
        CanvasShapeKind::PrismCore => {
            let crown = if y < 0.42 { sd_ellipse(x, y, 0.03, -0.04, 0.44, 0.39) } else { 1.0 };
            let lock_l = if x < -0.05 && y > -0.10 { sd_capsule(x, y, -0.36, 0.10, -0.48, 0.52, 0.13) } else { 1.0 };
            let lock_r = if x > 0.05 && y > -0.10 { sd_capsule(x, y, 0.38, 0.08, 0.50, 0.52, 0.132) } else { 1.0 };
            let chin = if (-0.05..0.54).contains(&y) { sd_ellipse(x, y, -0.05, 0.245, 0.24, 0.215) } else { 1.0 };
            let mut core = smin(smin(crown, chin, 0.05), lock_l.min(lock_r), 0.06);
            if y >= 0.24 {
                let neck = if y < 0.72 { sd_capsule(x, y, 0.0, 0.37, 0.0, 0.59, 0.092) } else { 1.0 };
                let bust = if y > 0.42 { sd_ellipse(x, y, 0.0, 0.68, 0.35, 0.155) } else { 1.0 };
                core = smin(core, smin(neck, bust, 0.06), 0.05);
            }
            if y > -0.02 {
                return core;
            }
            let ear_l = if x < 0.02 { sd_capsule(x, y, -0.20, -0.30, -0.28 + pose.ear_left * 0.35, -0.73, 0.098) } else { 1.0 };
            let ear_r = if x > 0.02 {
                smin(
                    sd_capsule(x, y, 0.22, -0.30, 0.42, -0.66 + pose.ear_right * 0.28, 0.092),
                    sd_capsule(x, y, 0.42, -0.66 + pose.ear_right * 0.28, 0.56, -0.52 + pose.ear_right * 0.36, 0.078),
                    0.05,
                )
            } else { 1.0 };
            smin(core, ear_l.min(ear_r), 0.05)
        }
        CanvasShapeKind::MechaHelm => {
            let crown = if y < 0.38 { sd_ellipse(x, y, -0.03, -0.10, 0.43, 0.40) } else { 1.0 };
            let chin = if (-0.08..0.52).contains(&y) { sd_ellipse(x, y, 0.07, 0.22, 0.24, 0.215) } else { 1.0 };
            let side_l = if x < -0.08 && y > -0.20 { sd_capsule(x, y, -0.38, -0.02, -0.44, 0.46, 0.115) } else { 1.0 };
            let side_r = if x > 0.08 && y > -0.16 { sd_capsule(x, y, 0.36, 0.02, 0.42, 0.44, 0.105) } else { 1.0 };
            let mut body = smin(smin(crown, chin, 0.05), side_l.min(side_r), 0.06);
            if y >= 0.22 {
                let neck = if y < 0.70 { sd_capsule(x, y, 0.0, 0.35, 0.0, 0.58, 0.092) } else { 1.0 };
                let bust = if y > 0.40 { sd_ellipse(x, y, -0.01, 0.67, 0.37, 0.155) } else { 1.0 };
                body = smin(body, smin(neck, bust, 0.06), 0.05);
            }
            if y > 0.22 {
                return body;
            }
            let fin_l = if x < -0.18 { sd_triangle(x, y, -0.48, -0.02, -0.62, -0.60, -0.32, -0.26) - 0.03 } else { 1.0 };
            let fin_r = if x > 0.18 { sd_triangle(x, y, 0.48, -0.02, 0.32, -0.26, 0.62, -0.60) - 0.03 } else { 1.0 };
            let crest = if x.abs() < 0.22 && y < -0.28 { sd_triangle(x, y, -0.09, -0.40, 0.0, -0.66, 0.09, -0.40) - 0.025 } else { 1.0 };
            smin(smin(body, crest, 0.04), fin_l.min(fin_r), 0.04)
        }
        CanvasShapeKind::GhostShell => {
            let hood = if y < 0.48 { sd_ellipse(x, y, 0.02, -0.08 + pose.hood_lift, 0.50, 0.45 - pose.hood_lift * 0.4) } else { 1.0 };
            let ear_l = if y < -0.20 && x < -0.10 { sd_circle(x, y, -0.37, -0.45 + pose.hood_lift, 0.125) } else { 1.0 };
            let ear_r = if y < -0.20 && x > 0.10 { sd_circle(x, y, 0.39, -0.45 + pose.hood_lift, 0.125) } else { 1.0 };
            let cowl = if y > 0.18 { sd_ellipse(x, y, 0.0, 0.46, 0.41, 0.18) } else { 1.0 };
            let bust = if y > 0.38 { sd_ellipse(x, y, 0.0, 0.66, 0.37, 0.155) } else { 1.0 };
            smin(smin(hood, smin(cowl, bust, 0.07), 0.08), ear_l.min(ear_r), 0.06)
        }
        CanvasShapeKind::AngelBot => {
            let crown = if y < 0.38 { sd_ellipse(x, y, -0.01, -0.05, 0.43, 0.38) } else { 1.0 };
            let hime_l = if x < -0.08 && y > -0.16 { sd_capsule(x, y, -0.39, 0.02, -0.44, 0.60, 0.115) } else { 1.0 };
            let hime_r = if x > 0.08 && y > -0.16 { sd_capsule(x, y, 0.38, 0.02, 0.43, 0.60, 0.115) } else { 1.0 };
            let chin = if (-0.08..0.52).contains(&y) { sd_ellipse(x, y, 0.035, 0.22, 0.24, 0.215) } else { 1.0 };
            let mut body = smin(smin(crown, chin, 0.05), hime_l.min(hime_r), 0.05);
            if y >= 0.22 {
                let neck = if y < 0.70 { sd_capsule(x, y, 0.0, 0.35, 0.0, 0.58, 0.09) } else { 1.0 };
                let bust = if y > 0.40 { sd_ellipse(x, y, 0.0, 0.67, 0.36, 0.155) } else { 1.0 };
                body = smin(body, smin(neck, bust, 0.06), 0.05);
            }
            if y > -0.45 || x.abs() > 0.50 {
                return body;
            }
            let halo = sd_ellipse(x, y - x * (0.06 + pose.halo_spin), 0.0, -0.63, 0.35, 0.068).abs() - 0.024;
            body.min(halo)
        }
        CanvasShapeKind::CyberSkull => {
            let crown = if y < 0.38 { sd_ellipse(x, y, 0.03, -0.10, 0.45, 0.41) } else { 1.0 };
            let tuft_l = if x < -0.08 && y > -0.12 {
                smin(sd_capsule(x, y, -0.36, 0.08, -0.57, 0.38, 0.105), sd_capsule(x, y, -0.34, 0.26, -0.53, 0.56, 0.105), 0.06)
            } else { 1.0 };
            let tuft_r = if x > 0.08 && y > -0.12 {
                smin(sd_capsule(x, y, 0.38, 0.08, 0.57, 0.38, 0.105), sd_capsule(x, y, 0.34, 0.26, 0.53, 0.56, 0.105), 0.06)
            } else { 1.0 };
            let chin = if (-0.08..0.52).contains(&y) { sd_ellipse(x, y, -0.055, 0.21, 0.24, 0.215) } else { 1.0 };
            let mut body = smin(smin(crown, chin, 0.05), tuft_l.min(tuft_r), 0.06);
            if y >= 0.22 {
                let neck = if y < 0.70 { sd_capsule(x, y, 0.0, 0.35, 0.01, 0.58, 0.092) } else { 1.0 };
                let bust = if y > 0.40 { sd_ellipse(x, y, 0.01, 0.67, 0.36, 0.155) } else { 1.0 };
                body = smin(body, smin(neck, bust, 0.06), 0.05);
            }
            if y > -0.35 || x < -0.22 || x > 0.42 {
                return body;
            }
            let ahoge = smin(
                sd_capsule(x, y, -0.04, -0.48, 0.08, -0.72 - pose.ahoge_bounce, 0.052),
                sd_capsule(x, y, 0.08, -0.72 - pose.ahoge_bounce, 0.28, -0.62 - pose.ahoge_bounce * 0.7, 0.045),
                0.04,
            );
            smin(body, ahoge, 0.06)
        }
    }
}

pub fn render_canvas_dot_rgba(
    pixel_w: u32,
    pixel_h: u32,
    cols: usize,
    rows: usize,
    shape: CanvasShapeKind,
    time: f32,
    mood: ExpressionMood,
    viseme: MouthViseme,
    energy: f32,
    is_thinking: bool,
    is_speaking: bool,
    is_dragging: bool,
    color_hex: Option<&str>,
    dot_profile: DotProfile,
    bloom: f32,
) -> Vec<u8> {
    let sub_w = cols * 2;
    let sub_h = rows * 4;
    let mut sub_lum = vec![0.0_f32; sub_w * sub_h];
    let pose = evaluate_idle_pose(shape, time, false);
    let blink = evaluate_natural_blink_curve(time, (shape as usize as f32) * 0.7, 3);
    let bounce_y = (time * 2.2).sin() * 0.018
        + if is_speaking { -(time * 7.0).sin().abs() * (0.018 + energy * 0.025) } else { 0.0 }
        + if is_dragging { -0.04 } else { 0.0 }
        + pose.head_y;
    let tilt = pose.tilt
        + if is_thinking {
            (time * 1.6).sin() * 0.05 - 0.03
        } else if is_speaking {
            (time * 3.8).sin() * (0.04 + energy * 0.05)
        } else {
            (time * 1.1).sin() * 0.015
        };
    let ct = tilt.cos();
    let st = tilt.sin();

    // Turn-coupled saccade eye-lead: idle action gaze wins, then thinking scan,
    // then VOR fixation holds with a lead proportional to head tilt
    // (TS: turnLead = clamp(-dragTilt * 1.15, -0.14, 0.14)).
    let (gaze_tx, gaze_ty) = if pose.gaze_x.abs() > 0.02 || pose.gaze_y.abs() > 0.02 {
        (pose.gaze_x, pose.gaze_y)
    } else if is_thinking {
        let step_phase = (time * 1.15).floor();
        ((step_phase * 1.9).sin() * 0.52, -0.34 + (step_phase * 1.3).cos() * 0.08)
    } else {
        let idle_fix = (time * 0.72).floor() as i64;
        let turn_lead = (-tilt * 1.15).clamp(-0.14, 0.14);
        let gx = if (idle_fix * 7).rem_euclid(3) == 0 { (idle_fix as f32 * 2.1).sin() * 0.22 } else { 0.0 } + turn_lead;
        let gy = if (idle_fix * 5).rem_euclid(4) == 0 { -0.10 } else { 0.02 };
        (gx, gy)
    };
    let gx = gaze_tx.clamp(-1.0, 1.0) * 0.022;
    let gy = gaze_ty.clamp(-1.0, 1.0) * 0.018;
    let mouth_open = if is_speaking {
        match viseme {
            MouthViseme::Closed => 0.0,
            MouthViseme::Wide => 0.85,
            MouthViseme::Round => 0.65,
            _ => (0.35 + energy * 0.55).clamp(0.0, 1.0),
        }
    } else {
        0.0
    };

    let (face_cx, face_cy, turn) = match shape {
        CanvasShapeKind::HermesOrb => (-0.08_f32, 0.04_f32, -0.36_f32),
        CanvasShapeKind::LotusCore => (0.04, 0.05, 0.22),
        CanvasShapeKind::CatBot => (-0.06, 0.075, -0.30),
        CanvasShapeKind::FoxSpirit => (0.07, 0.085, 0.32),
        CanvasShapeKind::PrismCore => (-0.045, 0.105, -0.22),
        CanvasShapeKind::MechaHelm => (0.07, 0.065, 0.32),
        CanvasShapeKind::GhostShell => (-0.035, 0.07, -0.18),
        CanvasShapeKind::AngelBot => (0.035, 0.095, 0.16),
        CanvasShapeKind::CyberSkull => (-0.055, 0.07, -0.26),
    };

    let inv_w = 1.0 / sub_w.max(1) as f32;
    let inv_h = 1.0 / sub_h.max(1) as f32;

    for sy in 0..sub_h {
        let ny_raw = ((sy as f32 + 0.5) * inv_h) * 2.0 - 1.0 - bounce_y;
        let row_off = sy * sub_w;
        for sx in 0..sub_w {
            let nx_raw = ((sx as f32 + 0.5) * inv_w) * 2.0 - 1.0;
            let mut nx = nx_raw * ct - (ny_raw - 0.15) * st;
            let mut ny = nx_raw * st + (ny_raw - 0.15) * ct + 0.15;
            if ny < -0.22 {
                nx += pose.hair_sway * (-0.22 - ny) * 0.55;
            }
            if ny < -0.48 {
                ny -= pose.halo_lift;
            }

            // Orbiting cognition sparks when thinking
            let mut vfx = 0.0_f32;
            if is_thinking && (-0.78..-0.36).contains(&ny) {
                for k in 0..3 {
                    let ang = time * 2.6 + (k as f32) * (PI * 2.0 / 3.0);
                    let ox = ang.cos() * 0.52;
                    let oy = -0.60 + ang.sin() * 0.10;
                    let d = ((nx - ox).powi(2) + (ny - oy).powi(2)).sqrt();
                    if d < 0.06 {
                        vfx = vfx.max(1.0 - d / 0.06);
                    }
                }
            }

            let d = evaluate_shape_sdf(shape, nx, ny, &pose);
            if d > 0.030 {
                // Optional tail wag for CatBot/FoxSpirit/CyberSkull
                if pose.tail_wag.abs() > 0.03 && (0.02..0.72).contains(&ny) && nx.abs() > 0.24 {
                    let side = if shape == CanvasShapeKind::FoxSpirit { -1.0 } else { 1.0 };
                    let tip_x = side * (0.72 + pose.tail_wag * 0.14);
                    let tip_y = 0.14 - pose.tail_wag.abs() * 0.14;
                    let dt = smin(
                        sd_capsule(nx, ny, side * 0.28, 0.56, side * 0.56, 0.42, 0.058),
                        sd_capsule(nx, ny, side * 0.56, 0.42, tip_x, tip_y, 0.050),
                        0.055,
                    );
                    if dt < 0.024 {
                        let cov = clamp01(1.0 - (dt + 0.004) / 0.028);
                        sub_lum[row_off + sx] = (cov * 0.84).max(vfx);
                        continue;
                    }
                }
                sub_lum[row_off + sx] = vfx;
                continue;
            }

            let edge_cov = (1.0 - (d + 0.004) / 0.034).clamp(0.0, 1.0);
            let bottom_fade = if ny > 0.72 { clamp01(1.0 - (ny - 0.72) / 0.12) } else { 1.0 };
            let alpha_cov = edge_cov * bottom_fade;
            if alpha_cov <= 0.01 {
                sub_lum[row_off + sx] = vfx;
                continue;
            }

            // Inner face vs hair + collar luminance
            let dy_f = ny - face_cy;
            let t_chin = clamp01((dy_f - 0.01) / (0.30 * 0.96));
            let w_y = 0.29 * (1.0 - t_chin * t_chin * 0.54);
            let jaw_shift = turn * (0.04 - t_chin * 0.075);
            let d_face = (((nx - (face_cx + jaw_shift)) / w_y.max(0.08)).powi(2) + (dy_f / 0.30).powi(2)).sqrt() - 1.0;
            let bang_bot = -0.075 + 0.022 * ((nx - face_cx) * 18.0).cos();
            let in_face = d_face < 0.0 && ny > bang_bot;

            let mut lum = if in_face {
                let mut f_lum = 0.68;
                // Eyes (gaze-shifted with turn-coupled lead, per-eye idle blinks)
                let eye_y = face_cy + gy;
                let left_ex = face_cx - 0.135 + gx;
                let right_ex = face_cx + 0.135 + gx;
                if (ny - eye_y).abs() < 0.095 {
                    let is_left = (nx - left_ex).abs() < (nx - right_ex).abs();
                    let ex = if is_left { left_ex } else { right_ex };
                    let eye_rx = 0.068_f32;
                    let eye_blink = if is_left { pose.blink_left } else { pose.blink_right };
                    let eye_ry = 0.078_f32 * (1.0 - blink.max(eye_blink) * 0.88);
                    let dx_e = nx - ex;
                    let dy_e = ny - eye_y;
                    if pose.happy_eyes > 0.45 && blink < 0.4 {
                        let arc = (dx_e * dx_e * 3.4 + eye_y + 0.012 - ny).abs();
                        if dx_e.abs() < eye_rx && arc < 0.018 {
                            f_lum = 0.02;
                        }
                    } else if dx_e.abs() < eye_rx * 1.1 && (ny - (eye_y - eye_ry * 0.65)).abs() < 0.022 {
                        f_lum = 0.02; // upper lash
                    } else if (dx_e / eye_rx).powi(2) + (dy_e / eye_ry.max(0.01)).powi(2) < 1.0 {
                        let c_dx = nx - (ex - 0.016);
                        let c_dy = ny - (eye_y - 0.018);
                        if c_dx * c_dx + c_dy * c_dy < 0.00028 {
                            f_lum = 0.98;
                        } else {
                            f_lum = 0.04;
                        }
                    }
                }
                // Mouth
                let mouth_y = face_cy + 0.175;
                if (ny - mouth_y).abs() < 0.055 && (nx - face_cx).abs() < 0.065 {
                    let mx = nx - face_cx;
                    if mouth_open > 0.12 {
                        let mh = 0.015 + mouth_open * 0.032;
                        if (mx / 0.042).powi(2) + ((ny - mouth_y) / mh).powi(2) < 1.0 {
                            f_lum = if ny < mouth_y - mh * 0.28 { 0.90 } else { 0.05 };
                        }
                    } else if mood == ExpressionMood::Happy || pose.happy_eyes > 0.3 {
                        let smile_y = mouth_y - mx * mx * 3.8;
                        if (ny - smile_y).abs() < 0.013 {
                            f_lum = 0.06;
                        }
                    } else if (ny - mouth_y).abs() < 0.011 {
                        f_lum = 0.07;
                    }
                }
                f_lum
            } else if ny >= 0.50 && nx.abs() < 0.36 {
                let v_neck = 0.52 + (0.12 - nx.abs()).max(0.0) * 0.95;
                if (ny - v_neck).abs() < 0.024 && nx.abs() < 0.16 {
                    0.92
                } else if ny < v_neck && nx.abs() < 0.12 {
                    0.74
                } else {
                    0.26
                }
            } else {
                let rim = if d > -0.068 { clamp01(1.0 + d / 0.068) * 0.48 } else { 0.0 };
                let shine = if (-0.34..-0.14).contains(&ny) && (ny + 0.25 - nx * nx * 0.26).abs() < 0.04 {
                    0.88
                } else {
                    0.0
                };
                (0.20 + rim).max(shine)
            };

            if shape == CanvasShapeKind::AngelBot && ny < -0.51 {
                lum = 0.98;
            }
            sub_lum[row_off + sx] = (lum * alpha_cov).max(vfx);
        }
    }

    let mut cell_bits = vec![0u8; cols * rows];
    let mut cell_lums = vec![0u8; cols * rows];
    for r in 0..rows {
        let sy_base = r * 4;
        let r0 = sy_base * sub_w;
        let r1 = r0 + sub_w;
        let r2 = r1 + sub_w;
        let r3 = r2 + sub_w;
        let b0 = (sy_base & 7) << 3;
        let b1 = ((sy_base + 1) & 7) << 3;
        let b2 = ((sy_base + 2) & 7) << 3;
        let b3 = ((sy_base + 3) & 7) << 3;
        let row_off = r * cols;

        for c in 0..cols {
            let c2 = c * 2;
            let l00 = sub_lum[r0 + c2];
            let l01 = sub_lum[r0 + c2 + 1];
            let l10 = sub_lum[r1 + c2];
            let l11 = sub_lum[r1 + c2 + 1];
            let l20 = sub_lum[r2 + c2];
            let l21 = sub_lum[r2 + c2 + 1];
            let l30 = sub_lum[r3 + c2];
            let l31 = sub_lum[r3 + c2 + 1];
            let lum_sum = l00 + l01 + l10 + l11 + l20 + l21 + l30 + l31;
            if lum_sum <= 0.12 {
                continue;
            }
            let bx0 = c2 & 7;
            let bx1 = (c2 + 1) & 7;
            let mut bits = 0u8;
            if l00 > 0.14 + BAYER_8X8[b0 + bx0] * 0.68 { bits |= 0x01; }
            if l01 > 0.14 + BAYER_8X8[b0 + bx1] * 0.68 { bits |= 0x08; }
            if l10 > 0.14 + BAYER_8X8[b1 + bx0] * 0.68 { bits |= 0x02; }
            if l11 > 0.14 + BAYER_8X8[b1 + bx1] * 0.68 { bits |= 0x10; }
            if l20 > 0.14 + BAYER_8X8[b2 + bx0] * 0.68 { bits |= 0x04; }
            if l21 > 0.14 + BAYER_8X8[b2 + bx1] * 0.68 { bits |= 0x20; }
            if l30 > 0.14 + BAYER_8X8[b3 + bx0] * 0.68 { bits |= 0x40; }
            if l31 > 0.14 + BAYER_8X8[b3 + bx1] * 0.68 { bits |= 0x80; }
            cell_lums[row_off + c] = (lum_sum * 31.875 + 0.5).clamp(0.0, 255.0) as u8;
            cell_bits[row_off + c] = bits;
        }
    }

    let (def_low, def_mid, def_high) = default_shape_tints(shape);
    let mid = color_hex.map(parse_hex_rgb).unwrap_or_else(|| parse_hex_rgb(def_mid));
    let low = parse_hex_rgb(def_low);
    let high = parse_hex_rgb(def_high);
    let palette = resolve_shape_tone_palette(low, mid, high, is_thinking, is_speaking, energy);
    let grid = BrailleFrameGrid {
        cols,
        rows,
        cell_bits,
        cell_lums,
    };
    rasterize_tonal_grid_rgba(pixel_w, pixel_h, &grid, &palette, dot_profile, bloom)
}
