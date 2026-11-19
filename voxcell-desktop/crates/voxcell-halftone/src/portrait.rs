//! Halftone portrait engine — 1:1 port of `src/utils/asciiPortraitEngine.ts`,
//! `src/utils/asciiCharacter.ts` and `src/utils/portraitLoadingEngine.ts`.
//!
//! Drives multi-harmonic breath, 2.5D yaw/pitch/tilt, asymmetric blink,
//! turn-coupled saccade eye-lead, and face-plane weighted hair spring lag,
//! then samples the 12-pose bilinear sprite bank (or single reference field)
//! into a 2x4 subpixel luminance grid for ordered-Bayer halftone packing.

use crate::raster::BrailleFrameGrid;
use image::ImageFormat;
use std::sync::OnceLock;
use voxcell_core::{ExpressionMood, EyeState, MouthViseme};

#[derive(Clone, Debug)]
pub struct PortraitSourceField {
    pub width: usize,
    pub height: usize,
    pub base_lum: Vec<f32>,
    pub ink_mask: Vec<f32>,
    pub min_ux: f32,
    pub max_ux: f32,
    pub min_uy: f32,
    pub max_uy: f32,
}

/// Continuous interpolated animation state for fluid character motion
/// (2.5D head yaw/pitch turning, smooth eye blinks, eyebrow micro-expressions,
/// damped lip-sync aperture, eye gaze saccades, and overlapping hair/shoulder follow-through).
#[derive(Clone, Debug)]
pub struct PortraitMotionState {
    pub blink: f32,
    pub squint: f32,
    pub wide: f32,
    pub gaze_x: f32,
    pub gaze_y: f32,
    pub brow_lift: f32,
    pub mouth_open: f32,
    pub mouth_width: f32,
    pub mouth_round: f32,
    pub jaw_drop: f32,
    pub smile: f32,
    pub tongue_lift: f32,
    pub tongue_show: f32,
    pub teeth_top: f32,
    pub teeth_bottom: f32,
    pub lip_press: f32,
    pub lip_bite: f32,
    pub rest_part: f32,
    pub pucker_ring: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub head_tilt: f32,
    pub head_sway_x: f32,
    pub head_nod_y: f32,
    pub hair_lag_x: f32,
    pub hair_lag_y: f32,
    pub shoulder_y: f32,
    pub listen_pulse: f32,
    /// Smoothed 0..1 speaking cadence envelope.
    pub speak_envelope: f32,
    /// Smoothed 0..1 audio energy envelope.
    pub smooth_energy: f32,
}

impl Default for PortraitMotionState {
    fn default() -> Self {
        Self {
            blink: 0.0,
            squint: 0.4,
            wide: 0.0,
            gaze_x: 0.0,
            gaze_y: 0.0,
            brow_lift: 0.0,
            mouth_open: 0.0,
            mouth_width: 1.0,
            mouth_round: 0.0,
            jaw_drop: 0.0,
            smile: 0.4,
            tongue_lift: 0.0,
            tongue_show: 0.0,
            teeth_top: 0.0,
            teeth_bottom: 0.0,
            lip_press: 0.0,
            lip_bite: 0.0,
            rest_part: 0.0,
            pucker_ring: 0.0,
            head_yaw: 0.0,
            head_pitch: 0.0,
            head_tilt: 0.0,
            head_sway_x: 0.0,
            head_nod_y: 0.0,
            hair_lag_x: 0.0,
            hair_lag_y: 0.0,
            shoulder_y: 0.0,
            listen_pulse: 0.0,
            speak_envelope: 0.0,
            smooth_energy: 0.08,
        }
    }
}

#[derive(Clone, Debug)]
pub struct PortraitAnimatorInput {
    pub time: f32,
    pub mood: ExpressionMood,
    pub eye_state: EyeState,
    pub viseme: MouthViseme,
    pub energy: f32,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub pointer_x: Option<f32>,
    pub pointer_y: Option<f32>,
    pub tap_impulse: f32,
}

/// Tactile impulse kinds mirroring `createPortraitAnimator().triggerImpulse`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImpulseKind {
    Mood,
    Speak,
    Tap,
}

/// Stateful damped-spring / exponential animator for fluid Canvas/GPU playback.
#[derive(Clone, Debug)]
pub struct PortraitAnimator {
    state: PortraitMotionState,
    hair_vel_x: f32,
    hair_vel_y: f32,
    mouth_vel: f32,
    jaw_vel: f32,
    brow_vel: f32,
    impulse_nod_vel: f32,
    impulse_nod_offset: f32,
    impulse_pitch_offset: f32,
    impulse_pitch_vel: f32,
    smooth_energy: f32,
    speak_envelope: f32,
    prev_head_tilt: f32,
    prev_head_yaw: f32,
    prev_head_nod_y: f32,
    prev_mood: Option<ExpressionMood>,
    prev_speaking: bool,
}

impl Default for PortraitAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl PortraitAnimator {
    pub fn new() -> Self {
        Self {
            state: PortraitMotionState::default(),
            hair_vel_x: 0.0,
            hair_vel_y: 0.0,
            mouth_vel: 0.0,
            jaw_vel: 0.0,
            brow_vel: 0.0,
            impulse_nod_vel: 0.0,
            impulse_nod_offset: 0.0,
            impulse_pitch_offset: 0.0,
            impulse_pitch_vel: 0.0,
            smooth_energy: 0.08,
            speak_envelope: 0.0,
            prev_head_tilt: 0.0,
            prev_head_yaw: 0.0,
            prev_head_nod_y: 0.0,
            prev_mood: None,
            prev_speaking: false,
        }
    }

    pub fn state(&self) -> &PortraitMotionState {
        &self.state
    }

    /// Adds a tactile nod/brow/hair bounce (mood change, speech onset, or tap).
    pub fn trigger_impulse(&mut self, kind: ImpulseKind) {
        match kind {
            ImpulseKind::Mood | ImpulseKind::Tap => {
                self.impulse_nod_vel = (self.impulse_nod_vel + 0.12).clamp(-0.22, 0.22);
                self.impulse_pitch_vel = (self.impulse_pitch_vel + 0.68).clamp(-1.15, 1.15);
                self.brow_vel = (self.brow_vel + 1.85).clamp(-3.0, 3.0);
                self.hair_vel_x = (self.hair_vel_x + 0.14).clamp(-0.26, 0.26);
                self.hair_vel_y = (self.hair_vel_y + 0.095).clamp(-0.17, 0.17);
            }
            ImpulseKind::Speak => {
                self.impulse_nod_vel = (self.impulse_nod_vel + 0.065).clamp(-0.14, 0.14);
                self.brow_vel = (self.brow_vel + 1.10).clamp(-2.0, 2.0);
                self.hair_vel_y = (self.hair_vel_y + 0.048).clamp(-0.11, 0.11);
            }
        }
    }

    pub fn step(&mut self, input: &PortraitAnimatorInput, dt: f32, reduced_motion: bool) -> &PortraitMotionState {
        let clamped_dt = dt.clamp(0.004, 0.06);
        if !reduced_motion {
            if !self.prev_speaking && input.is_speaking {
                self.trigger_impulse(ImpulseKind::Speak);
            } else if self.prev_mood.is_some()
                && self.prev_mood != Some(input.mood)
                && input.mood != ExpressionMood::Speaking
            {
                self.trigger_impulse(ImpulseKind::Mood);
            }
            if input.tap_impulse > 0.05 {
                self.trigger_impulse(ImpulseKind::Tap);
            }
        }
        self.prev_mood = Some(input.mood);
        self.prev_speaking = input.is_speaking;

        self.speak_envelope = exp_approach(self.speak_envelope, if input.is_speaking { 1.0 } else { 0.0 }, 8.2, clamped_dt);
        self.smooth_energy = exp_approach(
            self.smooth_energy,
            clamp01(input.energy),
            if input.energy > self.smooth_energy { 8.5 } else { 6.4 },
            clamped_dt,
        );
        self.state.speak_envelope = self.speak_envelope;
        self.state.smooth_energy = self.smooth_energy;

        let target = target_motion_from_input(input, self.speak_envelope, self.smooth_energy);

        // Asymmetric crisp blink: fast snap-close (rate 52), graceful lid lift (rate 22)
        let blink_rate = if target.blink > self.state.blink { 52.0 } else { 22.0 };
        self.state.blink = exp_approach(self.state.blink, target.blink, blink_rate, clamped_dt);
        self.state.squint = exp_approach(self.state.squint, target.squint, 10.0, clamped_dt);
        self.state.wide = exp_approach(self.state.wide, target.wide, 14.0, clamped_dt);

        // Smoothly damped brow micro-expression spring for expressive emphasis
        self.brow_vel += ((target.brow_lift - self.state.brow_lift) * 125.0 - self.brow_vel * 20.0) * clamped_dt;
        self.state.brow_lift = (self.state.brow_lift + self.brow_vel * clamped_dt).clamp(-0.48, 1.0);

        // Critically-damped 2nd-order filter for lip-sync mouth & jaw (zero 1-frame teleport pops)
        let mouth_stiffness = if target.mouth_open > self.state.mouth_open { 98.0 } else { 80.0 };
        self.mouth_vel += ((target.mouth_open - self.state.mouth_open) * mouth_stiffness - self.mouth_vel * 22.8) * clamped_dt;
        self.state.mouth_open = clamp01(self.state.mouth_open + self.mouth_vel * clamped_dt);
        self.jaw_vel += ((target.jaw_drop - self.state.jaw_drop) * 102.0 - self.jaw_vel * 22.5) * clamped_dt;
        self.state.jaw_drop = clamp01(self.state.jaw_drop + self.jaw_vel * clamped_dt);

        self.state.mouth_width = exp_approach(self.state.mouth_width, target.mouth_width, 13.0, clamped_dt);
        self.state.mouth_round = exp_approach(self.state.mouth_round, target.mouth_round, 13.0, clamped_dt);
        self.state.smile = exp_approach(self.state.smile, target.smile, 11.0, clamped_dt);
        self.state.teeth_top = exp_approach(self.state.teeth_top, target.teeth_top, 14.0, clamped_dt);
        self.state.teeth_bottom = exp_approach(self.state.teeth_bottom, target.teeth_bottom, 13.5, clamped_dt);
        self.state.tongue_show = exp_approach(self.state.tongue_show, target.tongue_show, 13.5, clamped_dt);
        self.state.tongue_lift = exp_approach(self.state.tongue_lift, target.tongue_lift, 13.5, clamped_dt);
        self.state.lip_press = exp_approach(self.state.lip_press, target.lip_press, 14.0, clamped_dt);
        self.state.lip_bite = exp_approach(self.state.lip_bite, target.lip_bite, 14.0, clamped_dt);
        self.state.pucker_ring = exp_approach(self.state.pucker_ring, target.pucker_ring, 13.5, clamped_dt);
        self.state.rest_part = exp_approach(self.state.rest_part, target.rest_part, 13.5, clamped_dt);

        // Crisp eye saccades between fixation points
        self.state.gaze_x = exp_approach(self.state.gaze_x, target.gaze_x, 18.0, clamped_dt);
        self.state.gaze_y = exp_approach(self.state.gaze_y, target.gaze_y, 18.0, clamped_dt);

        if reduced_motion {
            self.state.head_yaw = exp_approach(self.state.head_yaw, target.head_yaw * 0.2, 5.0, clamped_dt);
            self.state.head_pitch = exp_approach(self.state.head_pitch, target.head_pitch * 0.2, 5.0, clamped_dt);
            self.state.head_tilt = exp_approach(self.state.head_tilt, target.head_tilt * 0.25, 6.0, clamped_dt);
            self.state.head_nod_y = exp_approach(self.state.head_nod_y, target.head_nod_y * 0.2, 6.0, clamped_dt);
            self.state.head_sway_x = 0.0;
            self.state.hair_lag_x = 0.0;
            self.state.hair_lag_y = 0.0;
            self.state.shoulder_y = 0.0;
            self.impulse_nod_vel = 0.0;
            self.impulse_nod_offset = 0.0;
            self.impulse_pitch_offset = 0.0;
            self.impulse_pitch_vel = 0.0;
        } else {
            self.impulse_nod_vel += (-self.impulse_nod_offset * 135.0 - self.impulse_nod_vel * 17.0) * clamped_dt;
            self.impulse_nod_offset = (self.impulse_nod_offset + self.impulse_nod_vel * clamped_dt).clamp(-0.022, 0.022);

            self.impulse_pitch_vel += (-self.impulse_pitch_offset * 120.0 - self.impulse_pitch_vel * 16.0) * clamped_dt;
            self.impulse_pitch_offset = (self.impulse_pitch_offset + self.impulse_pitch_vel * clamped_dt).clamp(-0.085, 0.085);

            self.state.head_yaw = exp_approach(self.state.head_yaw, target.head_yaw, 6.2, clamped_dt);
            self.state.head_pitch = (exp_approach(self.state.head_pitch, target.head_pitch, 6.6, clamped_dt)
                + self.impulse_pitch_offset)
                .clamp(-0.24, 0.24);
            self.state.head_tilt = exp_approach(self.state.head_tilt, target.head_tilt, 7.2, clamped_dt);
            self.state.head_nod_y = exp_approach(self.state.head_nod_y, target.head_nod_y, 8.4, clamped_dt)
                + self.impulse_nod_offset
                - self.jaw_vel.max(0.0) * 0.0012;
            self.state.head_sway_x = exp_approach(self.state.head_sway_x, target.head_sway_x, 7.4, clamped_dt);
            self.state.shoulder_y = exp_approach(self.state.shoulder_y, target.shoulder_y, 5.5, clamped_dt);

            // Spring-damper secondary follow-through on lower hair curls reacting smoothly to head motion.
            // Face-plane weighting lives in the sampler: hair_plane_weight = 1.0 - face_zone * 0.85,
            // i.e. the face plane stays rigid while hair planes inherit the full spring lag.
            let tilt_vel = ((self.state.head_tilt - self.prev_head_tilt) / clamped_dt).clamp(-1.2, 1.2);
            let yaw_vel = ((self.state.head_yaw - self.prev_head_yaw) / clamped_dt).clamp(-1.4, 1.4);
            let nod_vel = ((self.state.head_nod_y - self.prev_head_nod_y) / clamped_dt).clamp(-0.8, 0.8);
            let desired_lag_x = target.hair_lag_x - tilt_vel * 0.044 - yaw_vel * 0.056;
            let desired_lag_y = target.hair_lag_y - nod_vel * 0.036;

            let stiffness = 54.0;
            let damping = 10.6;
            self.hair_vel_x = (self.hair_vel_x
                + ((desired_lag_x - self.state.hair_lag_x) * stiffness - self.hair_vel_x * damping) * clamped_dt)
                .clamp(-0.36, 0.36);
            self.hair_vel_y = (self.hair_vel_y
                + ((desired_lag_y - self.state.hair_lag_y) * stiffness - self.hair_vel_y * damping) * clamped_dt)
                .clamp(-0.24, 0.24);
            self.state.hair_lag_x = (self.state.hair_lag_x + self.hair_vel_x * clamped_dt).clamp(-0.092, 0.092);
            self.state.hair_lag_y = (self.state.hair_lag_y + self.hair_vel_y * clamped_dt).clamp(-0.060, 0.060);
        }

        self.state.listen_pulse = exp_approach(self.state.listen_pulse, target.listen_pulse, 9.0, clamped_dt);
        self.prev_head_tilt = self.state.head_tilt;
        self.prev_head_yaw = self.state.head_yaw;
        self.prev_head_nod_y = self.state.head_nod_y;

        &self.state
    }
}

/// Target motion choreography (pure) — multi-harmonic breath, mood head turns,
/// turn-coupled saccade eye-lead, viseme articulation and pointer look blending.
fn target_motion_from_input(input: &PortraitAnimatorInput, speak_env: f32, eff_energy: f32) -> PortraitMotionState {
    let time = input.time;
    let mood = input.mood;
    let energy = input.energy;

    // Natural rhythmic breathing & overlapping secondary action (organic figure-8 sway)
    let breath = (time * 2.35).sin() + (time * 4.7 + 0.35).sin() * 0.16;
    let breath_lag = (time * 2.35 - 0.82).sin() + (time * 4.7 - 0.48).sin() * 0.16;
    let speak_cadence = (time * 7.6).sin() * speak_env;
    let speak_bounce = speak_env * (time * 7.6).sin().abs() * (0.010 + eff_energy * 0.015);
    let speak_nod = speak_env * (time * 4.4).sin() * (0.022 + eff_energy * 0.036);

    // Expressive autonomous 2.5D head yaw & pitch choreography per mood
    let mut head_yaw = (time * 1.18).sin() * 0.225 + (time * 0.54 + 0.65).sin() * 0.088;
    let mut head_pitch = (time * 1.48).sin() * 0.102 + (time * 0.68 + 0.4).sin() * 0.034;

    match mood {
        ExpressionMood::Thinking => {
            head_yaw = -0.19 + (time * 1.05).sin() * 0.092 + (time * 0.46).sin() * 0.032;
            head_pitch = -0.095 + (time * 0.92).cos() * 0.044;
        }
        ExpressionMood::Listening => {
            head_yaw = 0.13 + (time * 1.38).sin() * 0.095;
            head_pitch = 0.032 + (time * 2.35).sin() * (0.044 + eff_energy * 0.048);
        }
        ExpressionMood::Alert => {
            head_yaw = 0.16 + (time * 2.8).sin() * 0.048;
            head_pitch = -0.045 + (time * 2.0).sin() * 0.024;
        }
        ExpressionMood::Focused => {
            head_yaw = 0.05 + (time * 0.88).sin() * 0.088;
            head_pitch = 0.03 + (time * 1.38).sin() * 0.040;
        }
        ExpressionMood::Happy => {
            head_yaw = 0.06 + (time * 1.28).sin() * 0.205 + (time * 0.56 + 1.05).sin() * 0.078;
            head_pitch = -0.016 + (time * 2.15).sin() * 0.086 + (time * 0.85).cos() * 0.034;
        }
        _ => {}
    }

    if speak_env > 0.005 {
        // Conversational emphasis head turns toward viewer & syllable nods
        head_yaw += speak_env * (0.075 + (time * 1.55).sin() * (0.09 + eff_energy * 0.062));
        head_pitch += speak_env * (time * 3.6).sin() * (0.036 + eff_energy * 0.046);
    }

    // Blend subtle pointer-tracking head turn when user hovers/moves cursor near portrait
    let has_look = input.pointer_x.is_some() || input.pointer_y.is_some();
    let lx = input.pointer_x.unwrap_or(0.0).clamp(-1.0, 1.0);
    let ly = input.pointer_y.unwrap_or(0.0).clamp(-1.0, 1.0);
    if has_look {
        head_yaw = lerp(head_yaw, lx * 0.30, 0.66);
        head_pitch = lerp(head_pitch, ly * 0.18, 0.62);
    }

    head_yaw = head_yaw.clamp(-0.34, 0.34);
    head_pitch = head_pitch.clamp(-0.21, 0.21);

    let base_tilt = match mood {
        ExpressionMood::Thinking => -0.068 + (time * 1.55).sin() * 0.036,
        ExpressionMood::Happy => 0.028 + (time * 2.05).sin() * 0.048 + (time * 0.78 + 0.4).sin() * 0.020,
        ExpressionMood::Listening => 0.042 + (time * 2.45).sin() * 0.034,
        ExpressionMood::Alert => -0.024 + (time * 2.5).sin() * 0.018,
        ExpressionMood::Focused => -0.014 + (time * 1.48).sin() * 0.026,
        _ => (time * 1.65).sin() * 0.048 + (time * 0.72 + 0.5).sin() * 0.020,
    };
    let head_tilt = base_tilt + speak_nod - head_yaw * 0.14;

    let auto_blink = evaluate_natural_blink_curve(time, 0.0, 4);
    let blink = match input.eye_state {
        EyeState::Blink => 1.0,
        EyeState::Half => 0.46_f32.max(auto_blink),
        _ => auto_blink,
    };
    let squint = if mood == ExpressionMood::Happy {
        0.34 + speak_cadence.max(0.0) * eff_energy * 0.12
    } else if input.eye_state == EyeState::Half || mood == ExpressionMood::Focused {
        0.56
    } else if input.viseme == MouthViseme::Ee {
        0.22
    } else {
        speak_env * eff_energy * 0.14
    };
    let wide = if input.eye_state == EyeState::Wide || mood == ExpressionMood::Alert { 0.88 } else { 0.0 };

    // Dynamic eyebrow micro-expressions (emphasis raises, focused narrows, subtle blink micro-dip)
    let mut brow_lift = 0.18 * (time * 1.48).sin() + 0.06 * (time * 0.68 + 0.3).sin();
    if mood == ExpressionMood::Alert || input.eye_state == EyeState::Wide {
        brow_lift = 0.72;
    } else if mood == ExpressionMood::Thinking {
        brow_lift = 0.38 + (time * 1.6).sin() * 0.14;
    } else if mood == ExpressionMood::Focused || input.eye_state == EyeState::Half {
        brow_lift = -0.34;
    } else if mood == ExpressionMood::Happy {
        brow_lift = 0.26 + (time * 1.95).sin() * 0.13 + (time * 0.65 + 0.5).sin() * 0.05;
    }
    if speak_env > 0.005 {
        brow_lift += speak_env * (time * 3.8).sin().max(0.0) * (0.10 + eff_energy * 0.40);
    }
    brow_lift -= blink * 0.18;
    brow_lift = brow_lift.clamp(-0.45, 1.0);

    // Mouth targets mapped to the 10-viseme Talking Reference sheet
    let mouth_open;
    let mouth_width;
    let mut mouth_round = 0.0_f32;
    let mut smile: f32 = if mood == ExpressionMood::Happy { 0.68 } else { 0.08 };
    let mut jaw_drop = 0.0_f32;
    let mut teeth_top = 0.0_f32;
    let mut teeth_bottom = 0.0_f32;
    let mut tongue_show = 0.0_f32;
    let mut tongue_lift = 0.0_f32;
    let mut lip_press = 0.0_f32;
    let mut lip_bite = 0.0_f32;
    let mut pucker_ring = 0.0_f32;
    let mut rest_part = 0.0_f32;

    match input.viseme {
        MouthViseme::Closed => {
            mouth_open = 0.0;
            mouth_width = 0.92;
        }
        MouthViseme::Rest => {
            mouth_open = 0.10;
            mouth_width = 0.94;
            teeth_top = 0.48;
            jaw_drop = 0.05;
            rest_part = 1.0;
        }
        MouthViseme::Smile => {
            mouth_open = 0.0;
            mouth_width = 1.12;
            smile = 1.0;
        }
        MouthViseme::Mbp => {
            mouth_open = 0.0;
            mouth_width = 1.10;
            smile = 0.02;
            lip_press = 1.0;
        }
        MouthViseme::Fv => {
            mouth_open = 0.24 + energy * 0.06;
            mouth_width = 1.02;
            teeth_top = 0.98;
            lip_bite = 1.0;
            jaw_drop = 0.12;
        }
        MouthViseme::Lth => {
            mouth_open = 0.56 + energy * 0.12;
            mouth_width = 1.02;
            teeth_top = 0.96;
            tongue_show = 1.0;
            tongue_lift = 0.96;
            jaw_drop = 0.45;
        }
        MouthViseme::Small => {
            mouth_open = 0.30 + energy * 0.10;
            mouth_width = 1.04;
            teeth_top = 0.98;
            jaw_drop = 0.22;
        }
        MouthViseme::Mid => {
            mouth_open = 0.62 + energy * 0.14;
            mouth_width = 1.06;
            mouth_round = 0.10;
            teeth_top = 0.94;
            tongue_show = 0.58;
            jaw_drop = 0.58;
        }
        MouthViseme::Round => {
            let tight_oo = if energy < 0.45 { 1.0 - smoothstep(0.18, 0.45, energy) } else { 0.0 };
            mouth_open = lerp(0.76 + energy * 0.14, 0.38, tight_oo);
            mouth_width = lerp(0.74, 0.56, tight_oo);
            mouth_round = 0.98;
            pucker_ring = lerp(0.92, 1.0, tight_oo);
            jaw_drop = lerp(0.62, 0.28, tight_oo);
        }
        MouthViseme::Ee => {
            mouth_open = 0.54 + energy * 0.12;
            mouth_width = 1.36;
            smile = smile.max(0.96);
            teeth_top = 1.0;
            teeth_bottom = 0.98;
            jaw_drop = 0.36;
        }
        MouthViseme::Wide => {
            mouth_open = 0.92 + energy * 0.08;
            mouth_width = 1.14;
            teeth_top = 1.0;
            tongue_show = 0.96;
            tongue_lift = 0.08;
            jaw_drop = 1.0;
        }
    }

    // Natural micro-saccades with realistic VOR fixation holds & crisp glance shifts
    let fix_window = (time * 0.85).floor() as i64;
    let fix_drift_x = (time * 1.1).sin() * 0.032;
    let fix_drift_y = (time * 0.95).cos() * 0.015;
    let saccade_x = if (fix_window * 7).rem_euclid(5) == 0 {
        0.28
    } else if (fix_window * 3).rem_euclid(4) == 1 {
        -0.25
    } else {
        0.06
    } * (fix_window as f32 * 2.1 + 0.5).sin();
    let saccade_y = if (fix_window * 5).rem_euclid(4) == 0 {
        -0.14
    } else if (fix_window * 3).rem_euclid(5) == 2 {
        0.08
    } else {
        0.02
    };

    // Turn-coupled saccade eye-lead: gaze anticipates the head turn slightly.
    let mut gaze_x = match mood {
        ExpressionMood::Thinking => -0.40 + ((time * 1.15).floor() * 1.7).sin() * 0.34 + fix_drift_x,
        ExpressionMood::Listening => ((time * 0.85).floor() * 1.9).sin() * 0.26 + fix_drift_x,
        _ => saccade_x + fix_drift_x + ((head_yaw - 0.04) * 0.20).clamp(-0.09, 0.09),
    };
    let mut gaze_y = match mood {
        ExpressionMood::Thinking => -0.46 + ((time * 1.15).floor() * 1.3).cos() * 0.16 + fix_drift_y,
        ExpressionMood::Alert => -0.10,
        _ => saccade_y + fix_drift_y + (head_pitch * 0.16).clamp(-0.045, 0.045),
    };

    if has_look {
        gaze_x = lerp(gaze_x, (lx * 1.15).clamp(-1.0, 1.0), 0.75);
        gaze_y = lerp(gaze_y, (ly * 0.9).clamp(-1.0, 1.0), 0.75);
    }

    let listen_pulse = if speak_env > 0.01 {
        clamp01(
            (0.55 + eff_energy * 0.45) * speak_env
                + if input.is_listening { (0.34 + eff_energy * 0.45) * (1.0 - speak_env) } else { 0.0 },
        )
    } else if input.is_listening {
        clamp01(0.36 + eff_energy * 0.45)
    } else {
        0.0
    };

    PortraitMotionState {
        blink,
        squint,
        wide,
        brow_lift,
        mouth_open: clamp01(mouth_open),
        mouth_width,
        mouth_round,
        smile: clamp01(smile),
        jaw_drop: clamp01(jaw_drop),
        teeth_top: clamp01(teeth_top),
        teeth_bottom: clamp01(teeth_bottom),
        tongue_show: clamp01(tongue_show),
        tongue_lift: clamp01(tongue_lift),
        lip_press: clamp01(lip_press),
        lip_bite: clamp01(lip_bite),
        pucker_ring: clamp01(pucker_ring),
        rest_part: clamp01(rest_part),
        gaze_x,
        gaze_y,
        head_yaw,
        head_pitch,
        head_tilt,
        head_nod_y: breath * 0.026 - speak_bounce,
        head_sway_x: (time * 1.25).sin() * 0.022
            + (time * 0.56 + 0.7).sin() * 0.008
            + speak_env * (time * 3.8).sin() * 0.005 * eff_energy,
        hair_lag_x: breath_lag * 0.052 + speak_env * (time * 4.4 - 0.72).sin() * 0.024 * eff_energy,
        hair_lag_y: (time * 2.35 - 0.6).cos() * 0.022 + speak_env * (time * 7.6 - 0.65).sin() * 0.012 * eff_energy,
        shoulder_y: breath * 0.040,
        listen_pulse,
        speak_envelope: speak_env,
        smooth_energy: eff_energy,
    }
}

/// Evaluates a biologically-grounded asymmetric blink envelope in [0, 1]:
/// quasi-periodic warped intervals (~2.8s–4.4s) with organic jitter and occasional
/// double-blinks; fast snappy lid closure (~26%), brief full-closed dwell (~20%),
/// graceful ease-out opening (~54%).
pub fn evaluate_natural_blink_curve(time: f32, phase_offset: f32, double_blink_modulo: u32) -> f32 {
    let t = time + phase_offset;
    let blink_warp = t * 0.28 + 0.15 * (t * 0.41 + phase_offset * 0.6).sin() + 0.06 * (t * 0.19).sin();
    let cycle_index = blink_warp.floor() as i64;
    let cycle_pos = blink_warp - cycle_index as f32;

    fn shape_blink(u: f32) -> f32 {
        if u <= 0.0 || u >= 1.0 {
            return 0.0;
        }
        if u < 0.26 {
            let k = u / 0.26;
            return 1.0 - (1.0 - k) * (1.0 - k);
        }
        if u < 0.46 {
            return 1.0;
        }
        let k = (u - 0.46) / 0.54;
        let eased = k * k * (3.0 - 2.0 * k);
        1.0 - eased
    }

    // Primary blink (~215ms window in cycle)
    if cycle_pos > 0.935 {
        return shape_blink((cycle_pos - 0.935) / 0.065);
    }
    // Occasional crisp double-blink slightly before primary blink
    let modulo = double_blink_modulo.max(1) as i64;
    let has_double = cycle_index.rem_euclid(modulo) == 1;
    if has_double && cycle_pos > 0.835 && cycle_pos < 0.892 {
        return shape_blink((cycle_pos - 0.835) / 0.057) * 0.94;
    }
    0.0
}

#[inline]
fn sd_ellipse(x: f32, y: f32, cx: f32, cy: f32, rx: f32, ry: f32) -> f32 {
    let dx = (x - cx) / rx;
    let dy = (y - cy) / ry;
    ((dx * dx + dy * dy).sqrt() - 1.0) * rx.min(ry)
}

#[inline]
fn sd_capsule2(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> f32 {
    let pax = px - ax;
    let pay = py - ay;
    let bax = bx - ax;
    let bay = by - ay;
    let denom = if bax * bax + bay * bay == 0.0 { 1.0 } else { bax * bax + bay * bay };
    let h = clamp01((pax * bax + pay * bay) / denom);
    let dx = pax - bax * h;
    let dy = pay - bay * h;
    (dx * dx + dy * dy).sqrt() - r
}

/// Converts RGBA pixels of the transparent black-ink character reference artwork
/// into a sculpted chiaroscuro luminance field for dark-mode halftone dithering.
pub fn build_portrait_source_from_rgba(rgba: &[u8], width: usize, height: usize) -> PortraitSourceField {
    let total = width * height;
    let mut outside = vec![0u8; total];
    let mut queue: Vec<usize> = Vec::with_capacity(total);

    let push_outside = |x: usize, y: usize, outside: &mut [u8], queue: &mut Vec<usize>| {
        let idx = y * width + x;
        if outside[idx] == 0 && rgba[idx * 4 + 3] < 42 {
            outside[idx] = 1;
            queue.push(idx);
        }
    };

    if width > 0 && height > 0 {
        for x in 0..width {
            push_outside(x, 0, &mut outside, &mut queue);
            if (x as f32) < width as f32 * 0.26 || (x as f32) > width as f32 * 0.72 {
                push_outside(x, height - 1, &mut outside, &mut queue);
            }
        }
        for y in 0..height {
            push_outside(0, y, &mut outside, &mut queue);
            push_outside(width - 1, y, &mut outside, &mut queue);
        }
    }

    let mut head = 0usize;
    while head < queue.len() {
        let p = queue[head];
        head += 1;
        let x = p % width;
        let y = p / width;
        if x > 0 {
            push_outside(x - 1, y, &mut outside, &mut queue);
        }
        if x + 1 < width {
            push_outside(x + 1, y, &mut outside, &mut queue);
        }
        if y > 0 {
            push_outside(x, y - 1, &mut outside, &mut queue);
        }
        if y + 1 < height {
            push_outside(x, y + 1, &mut outside, &mut queue);
        }
    }

    let mut base_lum = vec![0.0_f32; total];
    let mut ink_mask = vec![0.0_f32; total];
    let mut min_x = width;
    let mut max_x = 0usize;
    let mut min_y = height;
    let mut max_y = 0usize;
    let mut any_active = false;

    for y in 0..height {
        let uy = y as f32 / (height.saturating_sub(1).max(1)) as f32;
        let bottom_fade = 1.0 - smoothstep(0.84, 0.99, uy);
        for x in 0..width {
            let ux = x as f32 / (width.saturating_sub(1).max(1)) as f32;
            let idx = y * width + x;
            if outside[idx] != 0 {
                continue;
            }
            let off = idx * 4;
            let alpha = rgba[off + 3] as f32 / 255.0;
            let r = rgba[off] as f32;
            let g = rgba[off + 1] as f32;
            let b = rgba[off + 2] as f32;
            let rgb_bright = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;
            let ink = alpha * (1.0 - rgb_bright * 0.85);
            ink_mask[idx] = ink;

            let near_out = (x > 0 && outside[idx - 1] != 0)
                || (x + 1 < width && outside[idx + 1] != 0)
                || (y > 0 && outside[idx - width] != 0)
                || (y + 1 < height && outside[idx + width] != 0)
                || (x > 1 && outside[idx - 2] != 0)
                || (x + 2 < width && outside[idx + 2] != 0);

            let lum = if ink > 0.35 {
                // Iteration #2 hair halftone volume: crown sheen + right strand wave
                // + dual (right & left-curl) halftone sheen + rim separation.
                let crown_sheen = (-((uy - 0.14) * 5.0).powi(2)).exp() * 0.16;
                let strand_wave = if ux > 0.44 && ux < 0.86 && uy > 0.24 && uy < 0.90 {
                    ((ux - uy * 0.22) * 34.0).sin().max(0.0) * 0.135 * (1.0 - smoothstep(0.76, 0.94, uy))
                } else {
                    0.0
                };
                let curl_sheen = if uy > 0.54 {
                    (-((ux - 0.72) * 4.2).powi(2)).exp() * 0.12 + (-((ux - 0.16) * 5.2).powi(2)).exp() * 0.11
                } else {
                    0.0
                };
                let rim_glow = if near_out { 0.52 } else { 0.0 };
                clamp01((0.055 + crown_sheen + strand_wave + curl_sheen + rim_glow) * bottom_fade)
            } else if uy < 0.36 {
                let band_sheen = 0.85 + (-((ux - 0.52) * 4.0).powi(2)).exp() * 0.12;
                clamp01(band_sheen * bottom_fade)
            } else {
                let is_eye_zone = ux > 0.16 && ux < 0.45 && uy > 0.39 && uy < 0.475;
                let cheek_spec = (-((ux - 0.24) * 7.2).powi(2) - ((uy - 0.535) * 7.8).powi(2)).exp() * 0.12;
                let nose_spec = (-((ux - 0.21) * 15.0).powi(2) - ((uy - 0.495) * 8.5).powi(2)).exp() * 0.14;
                let cheek_shadow = smoothstep(0.285, 0.50, ux) * 0.36;
                let bang_shadow = (1.0 - smoothstep(0.35, 0.39, uy)) * 0.16;
                let chin_shadow = smoothstep(0.62, 0.87, uy) * 0.24;
                let eye_boost = if is_eye_zone { 0.15 } else { 0.0 };
                let face_lum = 0.62 + cheek_spec + nose_spec + eye_boost - cheek_shadow - bang_shadow - chin_shadow;
                clamp01(face_lum.min(0.86) * (1.0 - ink * 0.94) * bottom_fade)
            };

            base_lum[idx] = lum;
            if lum > 0.015 || ink > 0.015 {
                any_active = true;
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }

    let inv_w = 1.0 / width.saturating_sub(1).max(1) as f32;
    let inv_h = 1.0 / height.saturating_sub(1).max(1) as f32;
    let (min_ux, max_ux, min_uy, max_uy) = if any_active {
        (min_x as f32 * inv_w, max_x as f32 * inv_w, min_y as f32 * inv_h, max_y as f32 * inv_h)
    } else {
        (0.0, 1.0, 0.0, 1.0)
    };

    PortraitSourceField { width, height, base_lum, ink_mask, min_ux, max_ux, min_uy, max_uy }
}

#[derive(Clone, Debug)]
pub struct ReferencePoseBlend {
    pub mouth_index_a: usize,
    pub mouth_index_b: usize,
    pub mouth_blend: f32,
    pub eye_index: usize,
    pub eye_blend: f32,
    pub eye_half_index: usize,
    pub eye_half_weight: f32,
    pub eye_closed_index: usize,
    pub eye_closed_weight: f32,
    pub blink_amount: f32,
}

/// Maps continuous facial motion state + phoneme viseme onto smooth crossfade indices
/// across the 12-panel reference sprite sheet, decoupling mouth articulation from
/// continuous 3-stage blinking (open -> half-lid -> closed).
pub fn resolve_reference_pose_blend(
    motion: &PortraitMotionState,
    viseme: MouthViseme,
    mood: ExpressionMood,
    is_speaking: bool,
) -> ReferencePoseBlend {
    let open = clamp01(motion.mouth_open);
    let round_amt = clamp01(motion.mouth_round);
    let smile_amt = clamp01(motion.smile);
    let blink_amt = clamp01(motion.blink);

    let (mouth_index_a, mouth_index_b, mouth_blend) =
        if !is_speaking && open < 0.08 && motion.lip_press < 0.2 && motion.lip_bite < 0.2 {
            (0, 11, smoothstep(0.15, 0.85, smile_amt))
        } else if viseme == MouthViseme::Round || round_amt > 0.45 {
            if open < 0.48 {
                (9, 7, smoothstep(0.12, 0.48, open))
            } else {
                (7, 6, smoothstep(0.48, 0.88, open))
            }
        } else if viseme == MouthViseme::Fv || motion.lip_bite > 0.42 {
            (1, 9, clamp01(motion.lip_bite))
        } else if viseme == MouthViseme::Lth || motion.tongue_lift > 0.42 {
            (8, 2, 0.45)
        } else if viseme == MouthViseme::Ee || (motion.mouth_width > 1.15 && open > 0.24) {
            (2, 8, clamp01((open - 0.24) * 1.15))
        } else if motion.lip_press > 0.45 && open < 0.14 {
            (0, 9, clamp01(motion.lip_press * 0.65))
        } else if open < 0.12 {
            (if smile_amt > 0.45 { 11 } else { 0 }, 1, smoothstep(0.02, 0.12, open))
        } else if open < 0.34 {
            (1, 2, (open - 0.12) / 0.22)
        } else if open < 0.66 {
            (2, if mood == ExpressionMood::Thinking { 8 } else { 3 }, (open - 0.34) / 0.32)
        } else {
            (2, 3, clamp01((open - 0.56) / 0.36))
        };

    let eye_half_index = if open > 0.52 { 4 } else { 6 };
    let eye_closed_index = if smile_amt > 0.45 || mood == ExpressionMood::Happy { 10 } else { 5 };
    let focused_squint = if mood == ExpressionMood::Focused {
        clamp01((motion.squint - 0.36) * 2.2)
    } else {
        0.0
    };
    let eff_blink = blink_amt.max(focused_squint * 0.52);

    let eye_half_weight = if eff_blink <= 0.5 {
        smoothstep(0.06, 0.48, eff_blink)
    } else {
        1.0 - smoothstep(0.46, 0.86, eff_blink)
    };
    let eye_closed_weight = smoothstep(0.42, 0.88, eff_blink);

    let mut eye_index = mouth_index_b;
    let mut eye_blend = clamp01(eye_half_weight + eye_closed_weight);
    if eye_closed_weight >= eye_half_weight && eye_closed_weight > 0.04 {
        eye_index = eye_closed_index;
    } else if eye_half_weight > 0.04 {
        eye_index = eye_half_index;
    } else if is_speaking && open > 0.50 {
        eye_index = 3;
        eye_blend = smoothstep(0.50, 0.85, open) * 0.58;
    }

    ReferencePoseBlend {
        mouth_index_a,
        mouth_index_b,
        mouth_blend: clamp01(mouth_blend),
        eye_index,
        eye_blend: clamp01(eye_blend),
        eye_half_index,
        eye_half_weight: clamp01(eye_half_weight),
        eye_closed_index,
        eye_closed_weight: clamp01(eye_closed_weight),
        blink_amount: blink_amt,
    }
}

fn detect_axis_segments<F>(is_divider: F, total: usize, expected_count: usize) -> Vec<(usize, usize)>
where
    F: Fn(usize) -> bool,
{
    let mut dividers: Vec<(usize, usize)> = Vec::new();
    let mut in_div = false;
    let mut div_start = 0usize;
    let min_scan = (total as f32 * 0.08).floor() as usize;
    let max_scan = (total as f32 * 0.92).ceil() as usize;

    for p in min_scan..max_scan.min(total) {
        let hit = is_divider(p);
        if hit && !in_div {
            in_div = true;
            div_start = p;
        } else if !hit && in_div {
            in_div = false;
            dividers.push((div_start, p - 1));
        }
    }

    if dividers.len() >= expected_count.saturating_sub(1) {
        let mut selected = Vec::new();
        for k in 1..expected_count {
            let target = (total * k) as f32 / expected_count as f32;
            let mut best = dividers[0];
            let mut best_dist = f32::INFINITY;
            for &d in &dividers {
                let mid = (d.0 + d.1) as f32 * 0.5;
                let dist = (mid - target).abs();
                if dist < best_dist {
                    best_dist = dist;
                    best = d;
                }
            }
            if best_dist < total as f32 * 0.08 {
                selected.push(best);
            }
        }
        if selected.len() == expected_count - 1 {
            let mut segments = Vec::with_capacity(expected_count);
            let mut cur = 2usize;
            for (d0, d1) in selected {
                segments.push((cur, (d0 - 2).max(cur + 8)));
                cur = d1 + 2;
            }
            segments.push((cur, (total - 3).max(cur + 8)));
            return segments;
        }
    }

    (0..expected_count)
        .map(|k| {
            let s = (total * k) / expected_count + 2;
            let e = ((total * (k + 1)) / expected_count).saturating_sub(2);
            (s, e.max(s + 8))
        })
        .collect()
}

/// Extracts, cleans, aligns, and builds a 12-pose PortraitSourceField bank
/// from a 4x3 character talking sprite sheet with clean bilinear resampling.
pub fn build_portrait_bank_from_sprite_sheet(
    rgba: &[u8],
    width: usize,
    height: usize,
    target_size: usize,
) -> Vec<PortraitSourceField> {
    let lum_at = |x: usize, y: usize| -> f32 {
        let i = (y * width + x) * 4;
        (0.299 * rgba[i] as f32 + 0.587 * rgba[i + 1] as f32 + 0.114 * rgba[i + 2] as f32) / 255.0
    };

    let row_segments = detect_axis_segments(
        |y| {
            let mut dark = 0usize;
            let mut x = 0usize;
            while x < width {
                if lum_at(x, y) < 0.92 {
                    dark += 1;
                }
                x += 2;
            }
            (dark as f32 / ((width + 1) / 2).max(1) as f32) > 0.72
        },
        height,
        3,
    );

    let col_segments = detect_axis_segments(
        |x| {
            let mut dark = 0usize;
            let mut y = 0usize;
            while y < height {
                if lum_at(x, y) < 0.92 {
                    dark += 1;
                }
                y += 2;
            }
            (dark as f32 / ((height + 1) / 2).max(1) as f32) > 0.72
        },
        width,
        4,
    );

    let mut bank = Vec::with_capacity(12);
    for &(y0, y1) in &row_segments {
        for &(x0, x1) in &col_segments {
            let cw = x1 - x0 + 1;
            let ch = y1 - y0 + 1;
            let c_total = cw * ch;
            let mut cell_rgb = vec![255u8; c_total * 3];
            for ry in 0..ch {
                for rx in 0..cw {
                    let src_idx = ((y0 + ry) * width + (x0 + rx)) * 4;
                    let dst_idx = (ry * cw + rx) * 3;
                    let a = rgba[src_idx + 3] as f32 / 255.0;
                    cell_rgb[dst_idx] = (rgba[src_idx] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                    cell_rgb[dst_idx + 1] = (rgba[src_idx + 1] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                    cell_rgb[dst_idx + 2] = (rgba[src_idx + 2] as f32 * a + 255.0 * (1.0 - a)).round() as u8;
                }
            }

            // 1) Remove disconnected panel number labels ("01".."12") strictly inside top-left corner
            let mut visited = vec![0u8; c_total];
            let mut queue: Vec<usize> = Vec::with_capacity(c_total);
            let label_max_x = (cw as f32 * 0.24).floor() as usize;
            let label_max_y = (ch as f32 * 0.20).floor() as usize;
            let bound_max_x = (cw as f32 * 0.27).floor() as usize;
            let bound_max_y = (ch as f32 * 0.23).floor() as usize;

            let is_label_cand = |idx: usize, rgb: &[u8]| -> bool {
                rgb[idx * 3] <= 236 || rgb[idx * 3 + 1] <= 236 || rgb[idx * 3 + 2] <= 236
            };

            for sy in 0..label_max_y.min(ch) {
                for sx in 0..label_max_x.min(cw) {
                    let seed = sy * cw + sx;
                    if visited[seed] != 0 || !is_label_cand(seed, &cell_rgb) {
                        continue;
                    }
                    queue.clear();
                    visited[seed] = 1;
                    queue.push(seed);
                    let mut q_head = 0usize;
                    let mut comp_max_x = sx;
                    let mut comp_max_y = sy;
                    while q_head < queue.len() {
                        let p = queue[q_head];
                        q_head += 1;
                        let px = p % cw;
                        let py = p / cw;
                        comp_max_x = comp_max_x.max(px);
                        comp_max_y = comp_max_y.max(py);
                        let nbs = [
                            if px > 0 { Some(p - 1) } else { None },
                            if px + 1 < cw { Some(p + 1) } else { None },
                            if py > 0 { Some(p - cw) } else { None },
                            if py + 1 < ch { Some(p + cw) } else { None },
                        ];
                        for nb in nbs.into_iter().flatten() {
                            if visited[nb] == 0 && is_label_cand(nb, &cell_rgb) {
                                visited[nb] = 1;
                                queue.push(nb);
                            }
                        }
                    }
                    if comp_max_x < bound_max_x && comp_max_y < bound_max_y {
                        for &p in &queue {
                            let px = p % cw;
                            let py = p / cw;
                            for dy in -1_isize..=1 {
                                let ny = py as isize + dy;
                                if ny < 0 || ny >= ch as isize {
                                    continue;
                                }
                                for dx in -1_isize..=1 {
                                    let nx = px as isize + dx;
                                    if nx < 0 || nx >= cw as isize {
                                        continue;
                                    }
                                    let np = ny as usize * cw + nx as usize;
                                    cell_rgb[np * 3] = 255;
                                    cell_rgb[np * 3 + 1] = 255;
                                    cell_rgb[np * 3 + 2] = 255;
                                }
                            }
                        }
                    }
                }
            }

            // 2) Exterior flood-fill from bright outer background
            let mut outside = vec![0u8; c_total];
            queue.clear();
            let push_out = |rx: usize, ry: usize, outside: &mut [u8], queue: &mut Vec<usize>, rgb: &[u8]| {
                let idx = ry * cw + rx;
                if outside[idx] == 0 && rgb[idx * 3] > 234 && rgb[idx * 3 + 1] > 234 && rgb[idx * 3 + 2] > 234 {
                    outside[idx] = 1;
                    queue.push(idx);
                }
            };
            for rx in 0..cw {
                push_out(rx, 0, &mut outside, &mut queue, &cell_rgb);
                if (rx as f32) < cw as f32 * 0.25 || (rx as f32) > cw as f32 * 0.74 {
                    push_out(rx, ch - 1, &mut outside, &mut queue, &cell_rgb);
                }
            }
            for ry in 0..ch {
                push_out(0, ry, &mut outside, &mut queue, &cell_rgb);
                push_out(cw - 1, ry, &mut outside, &mut queue, &cell_rgb);
            }
            let mut o_head = 0usize;
            while o_head < queue.len() {
                let p = queue[o_head];
                o_head += 1;
                let px = p % cw;
                let py = p / cw;
                if px > 0 {
                    push_out(px - 1, py, &mut outside, &mut queue, &cell_rgb);
                }
                if px + 1 < cw {
                    push_out(px + 1, py, &mut outside, &mut queue, &cell_rgb);
                }
                if py > 0 {
                    push_out(px, py - 1, &mut outside, &mut queue, &cell_rgb);
                }
                if py + 1 < ch {
                    push_out(px, py + 1, &mut outside, &mut queue, &cell_rgb);
                }
            }

            // 3) Measure stable alignment anchor (crown top Y & upper-hair horizontal centroid)
            let mut crown_y = (ch as f32 * 0.06).floor() as usize;
            let mut found_crown = false;
            let mut sum_x = 0.0_f32;
            let mut cnt_x = 0usize;
            for ry in 0..ch {
                for rx in 0..cw {
                    let idx = ry * cw + rx;
                    let avg = (cell_rgb[idx * 3] as u32 + cell_rgb[idx * 3 + 1] as u32 + cell_rgb[idx * 3 + 2] as u32) / 3;
                    if outside[idx] == 0 && avg < 195 {
                        if !found_crown && (rx as f32) > cw as f32 * 0.22 && (rx as f32) < cw as f32 * 0.78 {
                            crown_y = ry;
                            found_crown = true;
                        }
                        if (ry as f32) >= ch as f32 * 0.10 && (ry as f32) <= ch as f32 * 0.48 {
                            sum_x += rx as f32;
                            cnt_x += 1;
                        }
                    }
                }
            }

            let anchor_cx = if cnt_x > 0 { sum_x / cnt_x as f32 } else { cw as f32 * 0.5 };
            let anchor_crown_y = if found_crown { crown_y as f32 } else { ch as f32 * 0.06 };

            // 4) Resample aligned panel with clean outside-aware bilinear interpolation
            let mut aligned = vec![0u8; target_size * target_size * 4];
            let scale = cw.min(ch) as f32 * 0.94;
            let dst_cx = target_size as f32 * 0.505;
            let dst_crown_y = target_size as f32 * 0.055;

            for ty in 0..target_size {
                let src_y = anchor_crown_y + ((ty as f32 - dst_crown_y) / target_size as f32) * scale;
                if src_y < 0.0 || src_y >= (ch - 1) as f32 {
                    continue;
                }
                let y0i = src_y as usize;
                let y1i = if y0i + 1 < ch { y0i + 1 } else { y0i };
                let fty = src_y - y0i as f32;
                for tx in 0..target_size {
                    let src_x = anchor_cx + ((tx as f32 - dst_cx) / target_size as f32) * scale;
                    let out_idx = (ty * target_size + tx) * 4;
                    if src_x < 0.0 || src_x >= (cw - 1) as f32 {
                        continue;
                    }
                    let x0i = src_x as usize;
                    let x1i = if x0i + 1 < cw { x0i + 1 } else { x0i };
                    let ftx = src_x - x0i as f32;
                    let i00 = y0i * cw + x0i;
                    let i10 = y0i * cw + x1i;
                    let i01 = y1i * cw + x0i;
                    let i11 = y1i * cw + x1i;

                    let a00 = if outside[i00] != 0 { 0.0 } else { 1.0 };
                    let a10 = if outside[i10] != 0 { 0.0 } else { 1.0 };
                    let a01 = if outside[i01] != 0 { 0.0 } else { 1.0 };
                    let a11 = if outside[i11] != 0 { 0.0 } else { 1.0 };
                    let w00 = (1.0 - ftx) * (1.0 - fty) * a00;
                    let w10 = ftx * (1.0 - fty) * a10;
                    let w01 = (1.0 - ftx) * fty * a01;
                    let w11 = ftx * fty * a11;
                    let w_sum = w00 + w10 + w01 + w11;
                    if w_sum < 0.48 {
                        continue;
                    }
                    let inv_w = 1.0 / w_sum;
                    for ch_i in 0..3 {
                        aligned[out_idx + ch_i] = ((cell_rgb[i00 * 3 + ch_i] as f32 * w00
                            + cell_rgb[i10 * 3 + ch_i] as f32 * w10
                            + cell_rgb[i01 * 3 + ch_i] as f32 * w01
                            + cell_rgb[i11 * 3 + ch_i] as f32 * w11)
                            * inv_w)
                            .round() as u8;
                    }
                    aligned[out_idx + 3] = 255;
                }
            }
            bank.push(build_portrait_source_from_rgba(&aligned, target_size, target_size));
        }
    }
    bank
}

pub struct PortraitAssets {
    pub reference: PortraitSourceField,
    pub sprite_bank: Vec<PortraitSourceField>,
}

static PORTRAIT_ASSETS: OnceLock<PortraitAssets> = OnceLock::new();

pub fn portrait_assets() -> &'static PortraitAssets {
    PORTRAIT_ASSETS.get_or_init(|| {
        let base_bytes = include_bytes!("../../../assets/character/portrait_base.png");
        let sheet_bytes = include_bytes!("../../../assets/character/sprite_sheet.png");

        let base_img = image::load_from_memory_with_format(base_bytes, ImageFormat::Png)
            .expect("embedded portrait_base.png must decode")
            .to_rgba8();
        let reference = build_portrait_source_from_rgba(
            base_img.as_raw(),
            base_img.width() as usize,
            base_img.height() as usize,
        );

        let sheet_img = image::load_from_memory_with_format(sheet_bytes, ImageFormat::Png)
            .expect("embedded sprite_sheet.png must decode")
            .to_rgba8();
        let sprite_bank = build_portrait_bank_from_sprite_sheet(
            sheet_img.as_raw(),
            sheet_img.width() as usize,
            sheet_img.height() as usize,
            160,
        );

        PortraitAssets { reference, sprite_bank }
    })
}

/// Per-frame precomputed reference constants (rigid-body portrait kinematics:
/// zero rubber-sheet hair wave or differential shear).
pub struct FrameConstants<'a> {
    pub time: f32,
    pub energy: f32,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub has_turn: bool,
    pub has_waveform: bool,
    pub has_head_yaw_shading: bool,
    pub has_brow_lift: bool,
    pub motion: &'a PortraitMotionState,
    pub ct: f32,
    pub st: f32,
    pub rigid_shift_x: f32,
    pub rigid_shift_y: f32,
    pub glint_sweep: f32,
    pub sin_t38: f32,
    pub cos_t38: f32,
    pub sin_t24: f32,
    pub cos_t24: f32,
    pub sin_t45: f32,
    pub cos_t45: f32,
    pub wave_gain: f32,
    pub wave_amp: f32,
}

impl<'a> FrameConstants<'a> {
    pub fn new(
        time: f32,
        energy: f32,
        is_speaking: bool,
        is_listening: bool,
        motion: &'a PortraitMotionState,
    ) -> Self {
        // TS falls back to params.energy / (isSpeaking ? 1 : 0) when the motion state
        // carries no smoothed envelopes (manually constructed states).
        let eff_energy = if motion.smooth_energy == 0.0 { energy.clamp(0.0, 1.0) } else { motion.smooth_energy };
        let speak_env = if motion.speak_envelope == 0.0 && is_speaking { 1.0 } else { motion.speak_envelope };
        let has_turn = motion.head_yaw.abs() > 0.004 || motion.head_pitch.abs() > 0.004;

        let wave_amp_scale = if is_listening { speak_env.max(1.0) } else { speak_env };
        let wave_gain = clamp01(eff_energy * 1.05 * wave_amp_scale);

        // Poised, rigid-body portrait kinematics
        let rigid_tilt = (motion.head_tilt * 0.34).clamp(-0.045, 0.045);
        let rigid_shift_x = (motion.head_sway_x * 0.34 - motion.head_yaw * 0.042).clamp(-0.028, 0.028);
        let rigid_shift_y = (motion.head_nod_y * 0.42 - motion.head_pitch * 0.032).clamp(-0.022, 0.022);

        Self {
            time,
            energy: eff_energy,
            is_speaking,
            is_listening,
            has_turn,
            has_waveform: wave_gain > 0.08 && eff_energy > 0.14,
            has_head_yaw_shading: motion.head_yaw.abs() > 0.02,
            has_brow_lift: motion.brow_lift.abs() > 0.08,
            motion,
            ct: rigid_tilt.cos(),
            st: rigid_tilt.sin(),
            rigid_shift_x,
            rigid_shift_y,
            glint_sweep: (time * 0.52) % 1.15,
            sin_t38: (time * 3.8).sin(),
            cos_t38: (time * 3.8).cos(),
            sin_t24: (time * 2.4).sin(),
            cos_t24: (time * 2.4).cos(),
            sin_t45: (time * 4.5).sin(),
            cos_t45: (time * 4.5).cos(),
            wave_gain,
            wave_amp: 0.011 + eff_energy * 0.012,
        }
    }
}

pub fn sample_waveform_halo(ax: f32, ay: f32, fc: &FrameConstants<'_>) -> f32 {
    let adx = ax - 0.47;
    let ady = (ay - 0.43) * 0.92;
    let adist_sq = adx * adx + ady * ady;
    if adist_sq <= 0.172_225 || adist_sq >= 0.245_025 {
        return 0.0;
    }
    let adist = adist_sq.sqrt();
    let inv_r = 1.0 / adist;
    let u = adx * inv_r;
    let v = ady * inv_r;
    let u2 = u * u - v * v;
    let v2 = 2.0 * u * v;
    let u3 = u * (4.0 * u * u - 3.0);
    let v3 = v * (3.0 - 4.0 * v * v);
    let u4 = u2 * u2 - v2 * v2;
    let v4 = 2.0 * v2 * u2;
    let u5 = u2 * u3 - v2 * v3;
    let v5 = v2 * u3 + u2 * v3;
    let wave_r = 0.445
        + (v5 * fc.cos_t38 + u5 * fc.sin_t38) * fc.wave_amp
        + (u3 * fc.cos_t24 + v3 * fc.sin_t24) * 0.008;
    let ring_band = (adist - wave_r).abs();
    if ring_band >= 0.014 {
        return 0.0;
    }
    let arc_pulse = 0.5 + 0.5 * (v4 * fc.cos_t45 - u4 * fc.sin_t45);
    let side_weight = 0.35 + 0.65 * u.abs();
    let env = (1.0 - smoothstep(0.002, 0.014, ring_band))
        * side_weight
        * (0.28 + arc_pulse * 0.48)
        * fc.wave_gain;
    let fade = if ay > 0.74 { 1.0 - smoothstep(0.74, 0.86, ay) } else { 1.0 };
    clamp01(env * fade)
}

/// Sculpted eyelids, happy squints, dual iris catchlights & gaze-shifted glints.
fn apply_reference_eye_expression(ax: f32, ay: f32, mut lum: f32, motion: &PortraitMotionState) -> f32 {
    let blink = motion.blink;
    if blink > 0.05 {
        let upper_drop_l = lerp(0.414, 0.453, blink);
        let upper_drop_r = lerp(0.402, 0.445, blink);
        let lower_rise = smoothstep(0.55, 1.0, blink) * 0.010;
        let bot_l = 0.465 - lower_rise;
        let bot_r = 0.468 - lower_rise;

        let in_eye_xl = 1.0 - smoothstep(0.018, 0.025, (ax - 0.223).abs());
        let in_eye_xr = 1.0 - smoothstep(0.049, 0.064, (ax - 0.380).abs());
        let skin_tone_l = 0.70;
        let skin_tone_r = clamp01(0.67 - smoothstep(0.32, 0.46, ax) * 0.24);

        let wipe_bot_l = if blink > 0.64 { lerp(upper_drop_l, 0.476, smoothstep(0.64, 0.88, blink)) } else { upper_drop_l };
        let wipe_bot_r = if blink > 0.64 { lerp(upper_drop_r, 0.478, smoothstep(0.64, 0.88, blink)) } else { upper_drop_r };

        if in_eye_xl > 0.02 && ay > 0.398 && ay < wipe_bot_l + 0.004 {
            let v_mask = smoothstep(0.398, 0.408, ay) * (1.0 - smoothstep(wipe_bot_l - 0.004, wipe_bot_l + 0.004, ay));
            let mask = in_eye_xl * v_mask * smoothstep(0.04, 0.20, blink);
            lum = lerp(lum, skin_tone_l, mask * 0.98);
        }
        if in_eye_xr > 0.02 && ay > 0.384 && ay < wipe_bot_r + 0.004 {
            let v_mask = smoothstep(0.384, 0.395, ay) * (1.0 - smoothstep(wipe_bot_r - 0.004, wipe_bot_r + 0.004, ay));
            let mask = in_eye_xr * v_mask * smoothstep(0.04, 0.20, blink);
            lum = lerp(lum, skin_tone_r, mask * 0.98);
        }

        if blink > 0.70 {
            let low_clean_l = in_eye_xl * smoothstep(bot_l - 0.004, bot_l + 0.005, ay) * (1.0 - smoothstep(bot_l + 0.005, 0.476, ay));
            let low_clean_r = in_eye_xr * smoothstep(bot_r - 0.004, bot_r + 0.005, ay) * (1.0 - smoothstep(bot_r + 0.005, 0.478, ay));
            let full_clean = smoothstep(0.70, 0.92, blink);
            if low_clean_l > 0.02 {
                lum = lerp(lum, skin_tone_l, low_clean_l * full_clean * 0.92);
            }
            if low_clean_r > 0.02 {
                lum = lerp(lum, skin_tone_r, low_clean_r * full_clean * 0.92);
            }
        }

        let cl_norm = (ax - 0.223) / 0.024;
        let cr_norm = (ax - 0.380) / 0.056;
        let crease_curve_l = cl_norm * cl_norm * 0.0025;
        let crease_curve_r = cr_norm * cr_norm * 0.004;
        let d_crease_l = sd_capsule2(ax, ay + crease_curve_l, 0.202, upper_drop_l - 0.015, 0.242, upper_drop_l - 0.016, 0.0016);
        let d_crease_r = sd_capsule2(ax, ay + crease_curve_r, 0.334, upper_drop_r - 0.016, 0.426, upper_drop_r - 0.018, 0.0018);
        let d_crease = d_crease_l.min(d_crease_r);
        if d_crease < 0.004 {
            let crease_envelope = smoothstep(0.18, 0.48, blink) * (1.0 - smoothstep(0.66, 0.96, blink) * 0.72);
            let c_mask = (1.0 - smoothstep(-0.001, 0.003, d_crease)) * crease_envelope;
            lum *= 1.0 - c_mask * 0.22;
        }

        let lash_base_l = lerp(upper_drop_l, 0.453, smoothstep(0.65, 1.0, blink));
        let lash_base_r = lerp(upper_drop_r, 0.447, smoothstep(0.65, 1.0, blink));
        let al_norm = (ax - 0.222) / 0.024;
        let ar_norm = (ax - 0.380) / 0.054;
        let arc_shape_l = -al_norm * al_norm * lerp(-0.0022, 0.0052, blink);
        let arc_shape_r = -ar_norm * ar_norm * lerp(-0.003, 0.0072, blink);
        let d_lash_main_l = sd_capsule2(ax, ay + arc_shape_l, 0.198, lash_base_l, 0.245, lash_base_l - 0.002, 0.0038);
        let d_lash_main_r = sd_capsule2(ax, ay + arc_shape_r, 0.328, lash_base_r + 0.001, 0.433, lash_base_r - 0.003, 0.0047);
        let d_wing_l = sd_capsule2(ax, ay, 0.198, lash_base_l - 0.002, 0.191, lash_base_l - 0.006, 0.0024);
        let d_wing_r = sd_capsule2(ax, ay, 0.431, lash_base_r - 0.003, 0.443, lash_base_r - 0.009, 0.0030);
        let d_lm = d_lash_main_l.min(d_lash_main_r);
        let d_w = d_wing_l.min(d_wing_r);
        let d_lashes = d_lm.min(d_w);
        if d_lashes < 0.007 {
            let lash_mask = (1.0 - smoothstep(-0.002, 0.0046, d_lashes)) * smoothstep(0.14, 0.52, blink);
            lum *= 1.0 - lash_mask * 0.96;
        }

        if blink < 0.62 {
            let gx = motion.gaze_x * 0.008;
            let gy = motion.gaze_y * 0.005;
            let half_catch = 1.0 - smoothstep(0.15, 0.58, blink);
            let d_half_catch = sd_ellipse(ax, ay, 0.364 + gx, 0.436 + gy, 0.0068, 0.0065);
            if d_half_catch < 0.004 && ay > lash_base_r + 0.004 {
                let c_mask = (1.0 - smoothstep(-0.002, 0.003, d_half_catch)) * half_catch;
                lum = lum.max(c_mask * 0.94);
            }
        }
    } else {
        if motion.squint > 0.18 {
            let lift = motion.squint * 0.006;
            let cheek_warm_l = sd_ellipse(ax, ay, 0.218, 0.482 - lift, 0.036, 0.012);
            let cheek_warm_r = sd_ellipse(ax, ay, 0.378, 0.482 - lift, 0.052, 0.013);
            let d_squint = cheek_warm_l.min(cheek_warm_r);
            if d_squint < 0.008 {
                let s_mask = (1.0 - smoothstep(-0.003, 0.006, d_squint)) * motion.squint * 0.65;
                let cheek_tone = clamp01(0.70 - smoothstep(0.28, 0.48, ax) * 0.20);
                lum = lerp(lum, cheek_tone, s_mask);
            }
        }

        let gx = motion.gaze_x * 0.010;
        let gy = motion.gaze_y * 0.006;
        let catch_scale = 1.0 + motion.wide * 0.2 + motion.squint * 0.08;
        let catch_r = 0.0072 * catch_scale;
        let d_catch_l = sd_ellipse(ax, ay, 0.214 + gx, 0.428 + gy, catch_r * 0.85, catch_r);
        let d_catch_r = sd_ellipse(ax, ay, 0.360 + gx, 0.422 + gy, catch_r, catch_r);
        let d_sub_catch_r = sd_ellipse(ax, ay, 0.396 + gx * 0.65, 0.440 + gy * 0.65, catch_r * 0.52, catch_r * 0.52);
        let d_cr = d_catch_r.min(d_sub_catch_r);
        let d_catch = d_catch_l.min(d_cr);
        if d_catch < 0.004 {
            let c_mask = 1.0 - smoothstep(-0.002, 0.003, d_catch);
            lum = lum.max(c_mask * 0.98);
        }
    }
    lum
}

/// Audio-synced mouth visemes: sealed lips, smile arc, pressed seal, open cavity
/// with teeth bars, tongue floor/tip, labiodental tuck and pucker ring.
fn apply_reference_mouth_expression(ax: f32, ay: f32, mut lum: f32, motion: &PortraitMotionState) -> f32 {
    let mx_base = 0.274;
    let my = 0.614;
    let open_amt = motion.mouth_open;
    let smile_amt = motion.smile;
    let lip_press = motion.lip_press;
    let lip_bite = motion.lip_bite;
    let rest_part = motion.rest_part;
    let round_amt = clamp01(motion.mouth_round);
    let wide_stretch = clamp01((motion.mouth_width - 0.94) / 0.40) * (1.0 - round_amt);
    let is_pure_closed =
        open_amt <= 0.03 && lip_press < 0.08 && lip_bite < 0.08 && rest_part < 0.08 && smile_amt <= 0.32;

    if is_pure_closed {
        if ax >= 0.232 && ax <= 0.318 && ay >= 0.592 && ay <= 0.646 {
            let d_patch = sd_ellipse(ax, ay, 0.273, 0.616, 0.035, 0.020);
            if d_patch < 0.006 {
                let p_mask = 1.0 - smoothstep(-0.003, 0.005, d_patch);
                let skin_grad = clamp01(0.73 - smoothstep(0.245, 0.335, ax) * 0.16);
                lum = lerp(lum, skin_grad, p_mask * 0.92);
            }
            let bow_x = (ax - 0.272) * 26.0;
            let bow_curve = bow_x * bow_x * 0.0022;
            let d_closed_line = sd_capsule2(ax, ay - bow_curve, 0.246, 0.615, 0.300, 0.613, 0.0026);
            if d_closed_line < 0.005 && ax >= 0.232 {
                let c_mask = 1.0 - smoothstep(-0.002, 0.0034, d_closed_line);
                lum = lerp(lum, 0.015, c_mask * 0.92);
            }
            let d_under_lip = sd_ellipse(ax, ay, 0.272, 0.625, 0.014, 0.0032);
            if d_under_lip < 0.004 {
                let u_mask = 1.0 - smoothstep(-0.002, 0.003, d_under_lip);
                lum = lum.max(u_mask * 0.78);
            }
        }
        return lum;
    }

    let is_smile_only =
        open_amt <= 0.05 && lip_press < 0.15 && lip_bite < 0.15 && rest_part < 0.15 && smile_amt > 0.32;
    if is_smile_only {
        let d_outer_corner_patch = sd_ellipse(ax, ay, 0.294, 0.616, 0.024, 0.016);
        if d_outer_corner_patch < 0.006 && ax >= 0.232 {
            let p_mask = (1.0 - smoothstep(-0.003, 0.005, d_outer_corner_patch)) * smile_amt * 0.65;
            lum = lerp(lum, 0.68, p_mask);
        }
        let sm_x = (ax - 0.272) * 24.0;
        let smile_lift = sm_x * sm_x * (0.0034 + smile_amt * 0.0018);
        let d_smile = sd_capsule2(ax, ay + smile_lift, 0.245, 0.615, 0.306, 0.612, 0.0030);
        if d_smile < 0.005 && ax >= 0.232 {
            let s_mask = (1.0 - smoothstep(-0.002, 0.0038, d_smile)) * (smile_amt * 0.95).min(1.0);
            lum = lerp(lum, 0.015, s_mask * 0.88);
        }
        return lum;
    }

    let round_w = smoothstep(0.30, 0.72, round_amt);
    let not_round = 1.0 - round_w;
    let ee_w = not_round * smoothstep(0.22, 0.65, motion.teeth_bottom.max(wide_stretch));
    let fv_w = not_round * smoothstep(0.25, 0.65, lip_bite);
    let rest_w = not_round * smoothstep(0.25, 0.65, rest_part) * (1.0 - smoothstep(0.08, 0.22, open_amt));

    let mx = lerp(mx_base, 0.283, ee_w);
    let active_blend = clamp01(
        smoothstep(0.015, 0.28, open_amt)
            .max(smoothstep(0.08, 0.65, lip_press))
            .max(smoothstep(0.08, 0.65, lip_bite))
            .max(smoothstep(0.08, 0.65, rest_part)),
    );

    let clean_w = lerp(0.038, 0.052, open_amt.max(wide_stretch).max(lip_press * 0.72));
    let clean_h = lerp(0.024, 0.038, open_amt.max(lip_bite * 0.5));
    if ax >= 0.228 && ax <= 0.340 && ay >= 0.578 && ay <= 0.672 {
        let d_patch = sd_ellipse(ax, ay, 0.278, 0.616, clean_w, clean_h);
        if d_patch < 0.008 {
            let p_mask = (1.0 - smoothstep(-0.004, 0.006, d_patch)) * active_blend;
            let skin_grad = clamp01(0.73 - smoothstep(0.245, 0.345, ax) * 0.17);
            lum = lerp(lum, skin_grad, p_mask * 0.97);
        }
    }

    if lip_press > 0.22 && open_amt < 0.20 {
        let press_blend = smoothstep(0.22, 0.75, lip_press) * (1.0 - smoothstep(0.08, 0.20, open_amt));
        let corner_x = (ax - 0.274) * 30.0;
        let corner_curve = corner_x * corner_x * 0.0014;
        let d_seal = sd_capsule2(ax, ay - corner_curve, 0.241, 0.615, 0.308, 0.613, 0.0034);
        if d_seal < 0.005 && ax >= 0.228 {
            let s_mask = (1.0 - smoothstep(-0.002, 0.0036, d_seal)) * press_blend;
            lum = lerp(lum, 0.015, s_mask * 0.96);
        }
        let d_tick_l = sd_capsule2(ax, ay, 0.241, 0.612, 0.242, 0.618, 0.0018);
        let d_tick_r = sd_capsule2(ax, ay, 0.307, 0.610, 0.308, 0.616, 0.0018);
        let d_ticks = d_tick_l.min(d_tick_r);
        if d_ticks < 0.004 && ax >= 0.228 {
            let t_mask = (1.0 - smoothstep(-0.002, 0.003, d_ticks)) * press_blend;
            lum = lerp(lum, 0.02, t_mask * 0.88);
        }
        let d_press_glow = sd_ellipse(ax, ay, 0.274, 0.624, 0.019, 0.0042);
        if d_press_glow < 0.005 && ax >= 0.228 {
            let g_mask = (1.0 - smoothstep(-0.002, 0.0035, d_press_glow)) * press_blend;
            lum = lum.max(g_mask * 0.84);
        }
        if press_blend > 0.88 {
            return lum;
        }
    }

    let open_high = smoothstep(0.35, 0.55, open_amt);
    let base_rx = lerp(
        lerp(0.028, 0.031, clamp01(open_amt / 0.45)),
        lerp(0.031, 0.036, clamp01((open_amt - 0.45) / 0.55)),
        open_high,
    );
    let rx = lerp(
        lerp(lerp(lerp(base_rx, 0.022, rest_w), 0.030, fv_w), lerp(0.034, 0.038, open_amt.max(wide_stretch)), ee_w),
        lerp(0.014, 0.022, open_amt),
        round_w,
    );

    let base_ry = lerp(
        lerp(0.011, 0.014, clamp01(open_amt / 0.45)),
        lerp(0.018, 0.035, clamp01((open_amt - 0.45) / 0.55)),
        open_high,
    );
    let ry = lerp(
        lerp(lerp(lerp(base_ry, 0.0062, rest_w), 0.0125, fv_w), lerp(0.0165, 0.020, open_amt), ee_w),
        lerp(0.016, 0.026, open_amt),
        round_w,
    );

    let cy = my + lerp(0.000, 0.012, motion.jaw_drop);
    let slant_scale = lerp(lerp(0.0, -0.018, fv_w), -0.032, ee_w);
    let slant_y = (ax - mx) * slant_scale;
    let mouth_local_y = ay - slant_y;

    let norm_x = (ax - mx) / if rx == 0.0 { 0.01 } else { rx };
    let corner_lift = ee_w * (norm_x * norm_x - 0.20).max(0.0) * -0.0028;
    let arch_w = (1.0 - norm_x * norm_x).max(0.0);
    let top_flatten = ee_w * (arch_w * 0.0024 + corner_lift) + fv_w * (arch_w * 0.0018);

    let lip_border = lerp(lerp(0.0036, 0.0024, rest_w.max(fv_w)), 0.0034, ee_w);
    let d_outer_lip = sd_ellipse(ax, mouth_local_y + top_flatten * 0.5, mx, cy, rx + lip_border, ry + lip_border);
    let d_cavity = sd_ellipse(ax, mouth_local_y + top_flatten, mx, cy, rx, ry);

    if d_outer_lip < 0.0055 && ax >= 0.230 {
        let lip_rim = (1.0 - smoothstep(-0.002, 0.0034, d_outer_lip)) * active_blend;
        let rim_darken = lerp(0.80, 0.58, rest_w);
        let ee_target = lerp(lum, 0.02, lip_rim * 0.90);
        let std_target = lum * (1.0 - lip_rim * rim_darken);
        lum = lerp(std_target, ee_target, ee_w);
    }

    if d_cavity < 0.005 && ax >= 0.230 {
        let c_mask = (1.0 - smoothstep(-0.002, 0.0030, d_cavity)) * active_blend;
        lum = lerp(lum, 0.012, c_mask * 0.98);

        if motion.teeth_top > 0.12 && round_w < 0.92 {
            let open_cutoff = lerp(cy - ry * 0.10, cy - ry * 0.40, open_high);
            let teeth_cutoff = lerp(
                lerp(lerp(open_cutoff, cy + ry * 0.06, rest_w), cy - 0.0020, ee_w),
                cy + 0.0008,
                fv_w,
            );
            let teeth_span = lerp(lerp(0.85, 0.88, ee_w), 0.86, fv_w);
            if mouth_local_y < teeth_cutoff && norm_x.abs() < teeth_span {
                let t_edge = 1.0 - smoothstep(teeth_cutoff - 0.0020, teeth_cutoff + 0.0004, mouth_local_y);
                let t_bright = lerp(lerp(0.88, 0.98, motion.teeth_top), 0.72, rest_w);
                lum = lum.max(t_bright * t_edge * c_mask * (1.0 - round_w));
            }
        }

        let non_round_fv = not_round * (1.0 - fv_w);
        if motion.teeth_bottom > 0.2 && non_round_fv > 0.05 {
            let low_teeth_top = cy + 0.0022;
            if mouth_local_y > low_teeth_top && norm_x.abs() < 0.84 {
                let lt_edge = smoothstep(low_teeth_top, low_teeth_top + 0.0022, mouth_local_y);
                lum = lum.max(0.96 * motion.teeth_bottom * lt_edge * c_mask * non_round_fv);
            }
        }

        if motion.tongue_show > 0.18 && non_round_fv > 0.05 {
            let lift_w = smoothstep(0.18, 0.58, motion.tongue_lift);
            if lift_w > 0.02 {
                let d_tongue_tip = sd_ellipse(ax, mouth_local_y, mx, cy + ry * 0.26, rx * 0.38, ry * 0.48);
                if d_tongue_tip < 0.005 {
                    let tg_mask = (1.0 - smoothstep(-0.002, 0.0028, d_tongue_tip)) * motion.tongue_show * c_mask * non_round_fv * lift_w;
                    lum = lum.max(0.84 * tg_mask);
                }
            }
            if lift_w < 0.98 {
                let d_floor_tongue = sd_ellipse(ax, mouth_local_y, mx + 0.002, cy + ry * 0.66, rx * 0.56, ry * 0.30);
                if d_floor_tongue < 0.005 {
                    let tg_mask = (1.0 - smoothstep(-0.002, 0.003, d_floor_tongue)) * motion.tongue_show * c_mask * non_round_fv * (1.0 - lift_w);
                    lum = lum.max(0.43 * tg_mask);
                }
            }
        }

        if lip_bite > 0.18 {
            let bite_w = smoothstep(0.18, 0.52, lip_bite);
            let d_tuck_lip = sd_ellipse(ax, mouth_local_y, mx, cy + ry * 0.44, rx * 0.84, ry * 0.48);
            if d_tuck_lip < 0.005 && mouth_local_y > cy + 0.0020 {
                let b_mask = (1.0 - smoothstep(-0.002, 0.0026, d_tuck_lip)) * bite_w * c_mask;
                lum = lum.max(0.96 * b_mask);
            }
        }
    }

    if motion.pucker_ring > 0.20 {
        let d_ring = sd_ellipse(ax, mouth_local_y, mx + 0.001, cy + 0.001, rx + 0.0066, ry + 0.0066).abs() - 0.0038;
        if d_ring < 0.005 && ax >= 0.228 {
            let r_mask = (1.0 - smoothstep(-0.002, 0.0035, d_ring)) * motion.pucker_ring * active_blend;
            lum = lum.max(r_mask * 0.88);
        }
    }

    let lower_offset = lerp(ry + 0.0056, ry * 0.68 + 0.0024, fv_w);
    let d_lower_lip_rim = sd_ellipse(ax, mouth_local_y, mx + 0.001, cy + lower_offset, rx * lerp(0.80, 0.72, round_w), 0.0038);
    if d_lower_lip_rim < 0.005 && ax >= 0.228 {
        let l_mask = (1.0 - smoothstep(-0.002, 0.0035, d_lower_lip_rim)) * active_blend;
        lum = lum.max(l_mask * 0.86);
    }
    lum
}

/// Samples the single reference silhouette field at normalized UV with rigid-body
/// rotation/translation, jaw articulation, sculpted eye/mouth expressions and halo.
pub fn sample_reference_with_expression(
    ux: f32,
    uy: f32,
    fc: &FrameConstants<'_>,
    source: &PortraitSourceField,
) -> f32 {
    let motion = fc.motion;

    // 1. Safe viewport framing inset so crown arc & bob curls never clip edges
    let framed_x = (ux - 0.50) * 1.115 + 0.49;
    let framed_y = (uy - 0.56) * 1.12 + 0.51;
    if framed_x < 0.01 || framed_x > 0.99 || framed_y < 0.005 || framed_y > 0.995 {
        return 0.0;
    }

    // 2. Clean rigid-body rotation + translation around neck pivot (zero liquid shear/wave)
    let pivot_x = 0.38;
    let pivot_y = 0.74;
    let dx = framed_x - pivot_x - fc.rigid_shift_x;
    let dy = framed_y - pivot_y - fc.rigid_shift_y;
    let ax = pivot_x + dx * fc.ct - dy * fc.st;
    let mut ay = pivot_y + dx * fc.st + dy * fc.ct;

    // 3. Restrained jaw articulation only when actively speaking with wide mouth aperture
    let jaw_amt = clamp01(motion.jaw_drop + motion.mouth_open * 0.10);
    if jaw_amt > 0.08 && ay > 0.630 && ay < 0.785 && ax > 0.190 && ax < 0.455 {
        let lat_w = smoothstep(0.190, 0.228, ax) * (1.0 - smoothstep(0.320, 0.455, ax));
        let vert_w = smoothstep(0.630, 0.675, ay) * (1.0 - smoothstep(0.738, 0.785, ay));
        ay -= jaw_amt * 0.018 * lat_w * vert_w;
    }

    if !(0.0..=1.0).contains(&ax) || !(0.0..=1.0).contains(&ay) {
        return 0.0;
    }

    let w = source.width;
    let max_x = w - 1;
    let max_y = source.height - 1;
    let fx = ax * max_x as f32;
    let fy = ay * max_y as f32;
    let x0 = (fx as usize).min(max_x);
    let y0 = (fy as usize).min(max_y);
    let x1 = (x0 + 1).min(max_x);
    let y1 = (y0 + 1).min(max_y);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let i00 = y0 * w + x0;
    let i10 = y0 * w + x1;
    let i01 = y1 * w + x0;
    let i11 = y1 * w + x1;
    let w00 = (1.0 - tx) * (1.0 - ty);
    let w10 = tx * (1.0 - ty);
    let w01 = (1.0 - tx) * ty;
    let w11 = tx * ty;

    let mut lum = source.base_lum[i00] * w00 + source.base_lum[i10] * w10 + source.base_lum[i01] * w01 + source.base_lum[i11] * w11;
    let ink = source.ink_mask[i00] * w00 + source.ink_mask[i10] * w10 + source.ink_mask[i01] * w01 + source.ink_mask[i11] * w11;

    // Transparent pixel outside character silhouette: jump straight to acoustic waveform check
    if lum <= 0.001 && ink <= 0.001 {
        return if fc.has_waveform && ay < 0.86 { sample_waveform_halo(ax, ay, fc) } else { 0.0 };
    }

    if ink < 0.35 && (fc.has_head_yaw_shading || motion.head_pitch != 0.0) && ay > 0.36 && ay < 0.76 && ax > 0.17 && ax < 0.48 {
        let cheek_area = smoothstep(0.18, 0.46, ax) * smoothstep(0.38, 0.65, ay) * (1.0 - smoothstep(0.65, 0.76, ay));
        lum = clamp01(lum + (motion.head_yaw * 0.14 - motion.head_pitch * 0.06) * cheek_area);
    }

    if fc.has_brow_lift && ink < 0.55 && ay > 0.35 && ay < 0.42 && ax > 0.18 && ax < 0.44 {
        let b_offset = -motion.brow_lift * 0.014;
        let d_brow_l = sd_capsule2(ax, ay, 0.195, 0.392 + b_offset, 0.240, 0.386 + b_offset * 0.85, 0.0042);
        let d_brow_r = sd_capsule2(ax, ay, 0.330, 0.376 + b_offset * 0.9, 0.415, 0.372 + b_offset * 0.75, 0.0048);
        let d_brows = d_brow_l.min(d_brow_r);
        if d_brows < 0.006 {
            let b_mask = (1.0 - smoothstep(-0.002, 0.0042, d_brows)) * clamp01(motion.brow_lift.abs());
            lum *= 1.0 - b_mask * 0.74;
        }
    }

    if ax >= 0.16 && ax <= 0.47 && ay >= 0.37 && ay <= 0.51 {
        lum = apply_reference_eye_expression(ax, ay, lum, motion);
    }

    if ax >= 0.222 && ax <= 0.355 && ay >= 0.575 && ay <= 0.675 {
        lum = apply_reference_mouth_expression(ax, ay, lum, motion);
    }

    let aura = motion.listen_pulse;
    if aura > 0.06 && ink < 0.35 && ay < 0.28 && lum > 0.4 {
        let gx = (ax - fc.glint_sweep) * 7.5;
        let g_sq = gx * gx;
        if g_sq < 8.0 {
            lum = clamp01(lum + (-g_sq).exp() * aura * (0.14 + fc.energy * 0.14));
        }
    }

    // Soft bust bottom & outer edge feathering
    if ay > 0.83 {
        lum *= 1.0 - smoothstep(0.83, 0.98, ay);
    }
    if ax > 0.86 {
        lum *= 1.0 - smoothstep(0.86, 0.985, ax);
    }
    if ax < 0.085 {
        lum *= smoothstep(0.015, 0.085, ax);
    }

    // Subtle animated Braille acoustic waveform halo outside silhouette
    if fc.has_waveform && lum < 0.04 && ay < 0.86 {
        let wf = sample_waveform_halo(ax, ay, fc);
        if wf > lum {
            lum = wf;
        }
    }

    clamp01(lum)
}

#[inline]
fn bilinear4(arr: &[f32], i00: usize, i10: usize, i01: usize, i11: usize, w00: f32, w10: f32, w01: f32, w11: f32) -> f32 {
    arr[i00] * w00 + arr[i10] * w10 + arr[i01] * w01 + arr[i11] * w11
}

/// Samples the 12-pose aligned sprite bank with rigid-body head transform,
/// crossfaded mouth articulation and decoupled 3-stage eye-band blending.
/// Hair lag never shears the silhouette: the face plane stays rigid
/// (hair_plane_weight = 1.0 - face_zone * 0.85 with face_zone == 1 on the face).
pub fn sample_sprite_bank_with_expression(
    ux: f32,
    uy: f32,
    fc: &FrameConstants<'_>,
    bank: &[PortraitSourceField],
    pose: &ReferencePoseBlend,
) -> f32 {
    let motion = fc.motion;

    let framed_x = (ux - 0.50) * 1.088 + 0.50;
    let framed_y = (uy - 0.54) * 1.095 + 0.51;
    if framed_x < 0.01 || framed_x > 0.99 || framed_y < 0.005 || framed_y > 0.995 {
        return 0.0;
    }

    // Clean rigid-body portrait transform around neck pivot (zero hair-wave or rubber-sheet shear)
    let pivot_x = 0.42;
    let pivot_y = 0.74;
    let dx = framed_x - pivot_x - fc.rigid_shift_x;
    let dy = framed_y - pivot_y - fc.rigid_shift_y;
    let ax = pivot_x + dx * fc.ct - dy * fc.st;
    let ay = pivot_y + dx * fc.st + dy * fc.ct;
    if !(0.0..=1.0).contains(&ax) || !(0.0..=1.0).contains(&ay) {
        return 0.0;
    }

    let field_a = &bank[pose.mouth_index_a.min(bank.len() - 1)];
    let field_b = &bank[pose.mouth_index_b.min(bank.len() - 1)];
    let w = field_a.width;
    let max_x = w - 1;
    let max_y = field_a.height - 1;
    let fx = ax * max_x as f32;
    let fy = ay * max_y as f32;
    let x0 = (fx as usize).min(max_x);
    let y0 = (fy as usize).min(max_y);
    let x1 = (x0 + 1).min(max_x);
    let y1 = (y0 + 1).min(max_y);
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;

    let w00 = (1.0 - tx) * (1.0 - ty);
    let w10 = tx * (1.0 - ty);
    let w01 = (1.0 - tx) * ty;
    let w11 = tx * ty;
    let i00 = y0 * w + x0;
    let i10 = y0 * w + x1;
    let i01 = y1 * w + x0;
    let i11 = y1 * w + x1;

    let lum_a = bilinear4(&field_a.base_lum, i00, i10, i01, i11, w00, w10, w01, w11);
    let lum_b = bilinear4(&field_b.base_lum, i00, i10, i01, i11, w00, w10, w01, w11);
    let ink_a = bilinear4(&field_a.ink_mask, i00, i10, i01, i11, w00, w10, w01, w11);
    let ink_b = bilinear4(&field_b.ink_mask, i00, i10, i01, i11, w00, w10, w01, w11);
    let blend_t = smoothstep(0.06, 0.94, pose.mouth_blend);

    let mut lum = lerp(lum_a, lum_b, blend_t);
    let ink = lerp(ink_a, ink_b, blend_t);

    // Preserve crisp mouth cavity & lip line contrast during crossfades
    if pose.mouth_index_a != pose.mouth_index_b
        && blend_t > 0.04
        && blend_t < 0.96
        && ax >= 0.22
        && ax <= 0.44
        && ay >= 0.56
        && ay <= 0.71
        && (ink_a > 0.55 || ink_b > 0.55)
    {
        lum = lerp(lum, lum_a.min(lum_b), 0.35);
    }

    // Co-articulated upper eye/brow band: continuous 3-stage blend (open -> half-lid -> closed)
    if (pose.eye_blend > 0.015 || pose.blink_amount > 0.04) && ax >= 0.16 && ax <= 0.53 && ay >= 0.38 && ay <= 0.575 {
        let ex_mask = smoothstep(0.165, 0.198, ax) * (1.0 - smoothstep(0.475, 0.525, ax));
        let ey_mask = smoothstep(0.38, 0.415, ay) * (1.0 - smoothstep(0.545, 0.575, ay));
        let socket_mask = ex_mask * ey_mask;
        if socket_mask > 0.01 {
            let mut target_eye_lum = lum;
            if pose.eye_half_weight > 0.01 || pose.eye_closed_weight > 0.01 {
                let half_field = &bank[pose.eye_half_index.min(bank.len() - 1)];
                let closed_field = &bank[pose.eye_closed_index.min(bank.len() - 1)];
                let half_lum = bilinear4(&half_field.base_lum, i00, i10, i01, i11, w00, w10, w01, w11);
                let closed_lum = bilinear4(&closed_field.base_lum, i00, i10, i01, i11, w00, w10, w01, w11);
                let total_bw = clamp01(pose.eye_half_weight + pose.eye_closed_weight);
                let stage_mix = if total_bw > 0.001 {
                    lerp(half_lum, closed_lum, pose.eye_closed_weight / total_bw)
                } else {
                    lum
                };
                target_eye_lum = lerp(lum, stage_mix, total_bw);

                // Preserve crisp half-lid & closed eyelash ink contrast during 3-stage crossfades
                if pose.eye_closed_weight > 0.35 {
                    let closed_ink = bilinear4(&closed_field.ink_mask, i00, i10, i01, i11, w00, w10, w01, w11);
                    if closed_ink > 0.48 {
                        target_eye_lum = lerp(target_eye_lum, target_eye_lum.min(closed_lum), pose.eye_closed_weight * 0.55);
                    }
                } else if pose.eye_half_weight > 0.35 {
                    let half_ink = bilinear4(&half_field.ink_mask, i00, i10, i01, i11, w00, w10, w01, w11);
                    if half_ink > 0.48 {
                        target_eye_lum = lerp(target_eye_lum, target_eye_lum.min(half_lum), pose.eye_half_weight * 0.45);
                    }
                }
            } else if pose.eye_blend > 0.01 {
                let eye_field = &bank[pose.eye_index.min(bank.len() - 1)];
                let el = bilinear4(&eye_field.base_lum, i00, i10, i01, i11, w00, w10, w01, w11);
                target_eye_lum = lerp(lum, el, pose.eye_blend);
            }
            lum = lerp(lum, target_eye_lum, socket_mask);
        }
    }

    // Subtle studio key-light cheek shading shift during head turn (no pixel warp)
    if ink < 0.35 && fc.has_head_yaw_shading && ay > 0.36 && ay < 0.76 && ax > 0.18 && ax < 0.52 {
        let cheek_area = smoothstep(0.19, 0.48, ax) * smoothstep(0.38, 0.65, ay) * (1.0 - smoothstep(0.65, 0.76, ay));
        lum = clamp01(lum + motion.head_yaw * 0.12 * cheek_area);
    }

    // Traveling headband shimmer during speaking/listening
    let aura = motion.listen_pulse;
    if aura > 0.06 && ink < 0.35 && ay < 0.28 && lum > 0.4 {
        let gx = (ax - fc.glint_sweep) * 7.5;
        let g_sq = gx * gx;
        if g_sq < 8.0 {
            lum = clamp01(lum + (-g_sq).exp() * aura * (0.15 + fc.energy * 0.16));
        }
    }

    // Soft bust bottom & outer edge feathering into sparse Braille halftone stippling
    let bottom_feather = 1.0 - smoothstep(0.83, 0.98, ay);
    let right_feather = 1.0 - smoothstep(0.86, 0.985, ax);
    let left_feather = smoothstep(0.015, 0.085, ax);
    lum *= bottom_feather * right_feather * left_feather;

    // Subtle animated Braille acoustic waveform halo outside silhouette
    if fc.has_waveform && lum < 0.04 && ay < 0.86 {
        let wf = sample_waveform_halo(ax, ay, fc);
        if wf > lum {
            lum = wf;
        }
    }

    clamp01(lum)
}

/// Samples the portrait into a `[2*cols x 4*rows]` subpixel luminance buffer,
/// preferring the 12-pose sprite bank and falling back to the single reference field.
#[allow(clippy::too_many_arguments)]
pub fn sample_portrait_luminance_field(
    cols: usize,
    rows: usize,
    time: f32,
    mood: ExpressionMood,
    viseme: MouthViseme,
    energy: f32,
    is_speaking: bool,
    is_listening: bool,
    motion: &PortraitMotionState,
    assets: &PortraitAssets,
) -> Vec<f32> {
    let sub_w = cols * 2;
    let sub_h = rows * 4;
    let mut raw = vec![0.0_f32; sub_w * sub_h];
    let fc = FrameConstants::new(time, energy, is_speaking, is_listening, motion);
    let inv_sub_w = 1.0 / sub_w.max(1) as f32;
    let inv_sub_h = 1.0 / sub_h.max(1) as f32;

    if assets.sprite_bank.len() >= 12 {
        let pose = resolve_reference_pose_blend(motion, viseme, mood, is_speaking);
        let bounds = &assets.sprite_bank[0];
        let x_min_framed = if fc.has_waveform { 0.0 } else { (bounds.min_ux - 0.11).max(0.01) };
        let x_max_framed = if fc.has_waveform { 0.99 } else { (bounds.max_ux + 0.11).min(0.99) };
        let sx_start = (((x_min_framed - 0.50) / 1.088 + 0.50) * sub_w as f32 - 1.0).floor().max(0.0) as usize;
        let sx_end = (((x_max_framed - 0.50) / 1.088 + 0.50) * sub_w as f32 + 1.0).ceil().min(sub_w as f32) as usize;

        for sy in 0..sub_h {
            let uy = (sy as f32 + 0.5) * inv_sub_h;
            let framed_y = (uy - 0.54) * 1.095 + 0.51;
            if !(0.005..=0.995).contains(&framed_y) {
                continue;
            }
            let row_off = sy * sub_w;
            for sx in sx_start..sx_end {
                let ux = (sx as f32 + 0.5) * inv_sub_w;
                raw[row_off + sx] = sample_sprite_bank_with_expression(ux, uy, &fc, &assets.sprite_bank, &pose);
            }
        }
    } else {
        let src = &assets.reference;
        let x_min_framed = if fc.has_waveform { 0.0 } else { (src.min_ux - 0.11).max(0.01) };
        let x_max_framed = if fc.has_waveform { 0.99 } else { (src.max_ux + 0.11).min(0.99) };
        let sx_start = (((x_min_framed - 0.49) / 1.115 + 0.50) * sub_w as f32 - 1.0).floor().max(0.0) as usize;
        let sx_end = (((x_max_framed - 0.49) / 1.115 + 0.50) * sub_w as f32 + 1.0).ceil().min(sub_w as f32) as usize;

        for sy in 0..sub_h {
            let uy = (sy as f32 + 0.5) * inv_sub_h;
            let framed_y = (uy - 0.56) * 1.12 + 0.51;
            if !(0.005..=0.995).contains(&framed_y) {
                continue;
            }
            let row_off = sy * sub_w;
            for sx in sx_start..sx_end {
                let ux = (sx as f32 + 0.5) * inv_sub_w;
                raw[row_off + sx] = sample_reference_with_expression(ux, uy, &fc, src);
            }
        }
    }

    raw
}

/// Pure mathematical fallback portrait field (headless tests / pre-decode instant).
#[allow(clippy::too_many_arguments)]
pub fn sample_portrait_luminance(
    nx: f32,
    ny: f32,
    time: f32,
    mood: ExpressionMood,
    viseme: MouthViseme,
    eye_state: EyeState,
    energy: f32,
    is_speaking: bool,
    is_listening: bool,
    motion: Option<&PortraitMotionState>,
) -> f32 {
    let sway_x = (time * 1.35).sin() * 0.01 + if is_speaking { (time * 5.2).sin() * 0.008 * energy } else { 0.0 };
    let sway_y = (time * 1.9).cos() * 0.009 - if is_speaking { (time * 6.8).sin().abs() * 0.012 * energy } else { 0.0 };
    let tilt: f32 = match mood {
        ExpressionMood::Thinking => 0.055,
        ExpressionMood::Happy => -0.025,
        _ => 0.015,
    };
    let ct = tilt.cos();
    let st = tilt.sin();
    let rx0 = nx - sway_x;
    let ry0 = ny - sway_y;
    let x = rx0 * ct - ry0 * st;
    let y = rx0 * st + ry0 * ct;

    let yaw_shift = match motion {
        Some(m) => m.head_yaw * 0.14,
        None => (time * 0.72).sin() * 0.02,
    };
    let fx = x + 0.11 - yaw_shift;
    let fy = y + 0.02;

    let d_cranium = sd_ellipse(fx, fy, 0.0, -0.08, 0.34, 0.41);
    let d_jaw = sd_ellipse(fx, fy, -0.04, 0.19, 0.22, 0.25);
    let d_face = d_cranium.min((d_cranium + d_jaw) * 0.5);

    let d_neck = sd_capsule2(x, y, 0.01, 0.32, 0.08, 0.84, 0.125);
    let d_collar = sd_ellipse(x, y, 0.06, 0.78, 0.42, 0.18);
    let d_bust = d_neck.min(d_collar);

    let d_hair_crown = sd_ellipse(x, y, 0.03, -0.18, 0.5, 0.48);
    let d_hair_back_r = sd_capsule2(x, y, 0.28, -0.14, 0.46, 0.62, 0.17);
    let d_curl_l = sd_capsule2(x, y, -0.35, 0.12, -0.52, 0.58, 0.1);
    let d_bang_l = sd_capsule2(x, y, -0.02, -0.56, -0.44, -0.14, 0.11);
    let d_bang_c = sd_capsule2(x, y, 0.04, -0.54, -0.12, -0.12, 0.09);
    let d_hair = d_hair_crown.min(d_hair_back_r).min(d_curl_l.min(d_bang_l.min(d_bang_c)));

    let d_headband = sd_ellipse(x, y, 0.01, -0.02, 0.32, 0.55).abs() - 0.038;
    let radial_fade = 1.0 - smoothstep(0.72, 1.0, (x * 0.88).hypot(y * 0.92));
    let bottom_fade = 1.0 - smoothstep(0.55, 0.94, y);

    let mut lum = 0.0_f32;
    if d_hair < 0.065 {
        let hair_mask = 1.0 - smoothstep(-0.02, 0.05, d_hair);
        let rim = smoothstep(-0.055, 0.0, d_hair) * (1.0 - smoothstep(0.0, 0.045, d_hair));
        let zs = (y + 0.36 + (x * 22.0).sin() * 0.025) * 8.0;
        let zig_sheen = (-zs * zs).exp() * 0.65;
        lum = lum.max(hair_mask * (0.05 + rim * 0.38 + zig_sheen));
    }

    if d_headband < 0.02 && y < -0.08 && x > -0.25 && x < 0.32 {
        let hb_mask = 1.0 - smoothstep(-0.008, 0.015, d_headband);
        let pulse = if is_speaking || is_listening { 0.08 * energy } else { 0.0 };
        lum = lum.max(hb_mask * (0.94 + pulse));
    }

    if d_bust < 0.045 {
        let bust_mask = 1.0 - smoothstep(-0.015, 0.03, d_bust);
        lum = lum.max(bust_mask * 0.68);
    }

    if d_face < 0.035 {
        let face_mask = 1.0 - smoothstep(-0.018, 0.022, d_face);
        let key_light = clamp01(0.95 - (fx + 0.05) * 0.85);
        let mut skin_lum = 0.32 + key_light * 0.68;

        let eye_h = match eye_state {
            EyeState::Blink => 0.006,
            EyeState::Half => 0.019,
            EyeState::Wide => 0.044,
            EyeState::Open => 0.035,
        };
        let d_eye_l = sd_ellipse(fx, fy, -0.15, -0.02, 0.046, eye_h);
        let d_eye_r = sd_ellipse(fx, fy, 0.04, -0.03, 0.064, eye_h);
        let d_eyes = d_eye_l.min(d_eye_r);
        if d_eyes < 0.018 {
            let e_mask = 1.0 - smoothstep(-0.005, 0.014, d_eyes);
            skin_lum *= 1.0 - e_mask * 0.94;
            if eye_state != EyeState::Blink {
                let d_catch = sd_ellipse(fx, fy, -0.158, -0.03, 0.015, 0.014)
                    .min(sd_ellipse(fx, fy, 0.028, -0.04, 0.018, 0.016));
                skin_lum = skin_lum.max((1.0 - smoothstep(-0.003, 0.008, d_catch)) * 0.98);
            }
        }

        let (mouth_rx, mouth_ry, mouth_open) = get_mouth_shape(viseme, energy);
        let d_mouth = sd_ellipse(fx, fy, -0.095, 0.19, mouth_rx * 1.4, mouth_ry * 1.4);
        if d_mouth < 0.02 {
            let m_mask = 1.0 - smoothstep(-0.005, 0.01, d_mouth);
            skin_lum *= 1.0 - m_mask * if mouth_open > 0.3 { 0.95 } else { 0.85 };
        }

        lum = lum * (1.0 - face_mask) + clamp01(skin_lum) * face_mask;
    }

    clamp01(lum * radial_fade * bottom_fade)
}

fn get_mouth_shape(viseme: MouthViseme, energy: f32) -> (f32, f32, f32) {
    let e = energy * 0.025;
    match viseme {
        MouthViseme::Closed => (0.044, 0.008, 0.0),
        MouthViseme::Rest => (0.045, 0.011, 0.14),
        MouthViseme::Mbp => (0.050, 0.007, 0.0),
        MouthViseme::Fv => (0.048, 0.015 + e * 0.3, 0.34),
        MouthViseme::Lth => (0.052, 0.028 + e * 0.6, 0.68),
        MouthViseme::Smile => (0.056, 0.011, 0.15),
        MouthViseme::Small => (0.046, 0.021 + e * 0.65, 0.52),
        MouthViseme::Mid => (0.052, 0.031 + e * 0.85, 0.82),
        MouthViseme::Round => (0.034, 0.035 + e * 0.9, 0.92),
        MouthViseme::Ee => (0.068, 0.028 + e * 0.65, 0.78),
        MouthViseme::Wide => (0.060, 0.044 + e, 1.0),
    }
}

// ---------------------------------------------------------------------------
// portraitLoadingEngine.ts — orbital aperture loader + materialize wipe
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadingVariant {
    Hero,
    Compact,
    Thumb,
    Sidebar,
    Stage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadingStatus {
    Loading,
    Error,
}

/// Smooth ease-out progress (0 -> 1) over ~460ms after real portrait assets finish
/// decoding, or instant 1 when reduced motion is active.
pub fn compute_materialize_progress(elapsed_since_ready_sec: f32, reduced_motion: bool) -> f32 {
    if reduced_motion {
        return 1.0;
    }
    if elapsed_since_ready_sec <= 0.0 {
        return 0.0;
    }
    let t = clamp01(elapsed_since_ready_sec / 0.46);
    1.0 - (1.0 - t).powi(3)
}

const LOADING_BAYER_8X8: [f32; 64] = {
    let ranks = crate::raster::BAYER_8X8_RANKS;
    let mut out = [0.0_f32; 64];
    let mut i = 0;
    while i < 64 {
        out[i] = ranks[i] as f32 / 64.0 - 0.5;
        i += 1;
    }
    out
};

const BRAILLE_BITMASK: [u8; 8] = [0x01, 0x08, 0x02, 0x10, 0x04, 0x20, 0x40, 0x80];

/// Renders a bespoke animated Unicode Braille orbital aperture & scanline shimmer
/// loader (or offline signal glyph on error) — never a synthetic mock silhouette.
pub fn render_portrait_loading_frame(
    cols: usize,
    rows: usize,
    time: f32,
    variant: LoadingVariant,
    status: LoadingStatus,
    reduced_motion: bool,
) -> BrailleFrameGrid {
    let sub_w = cols * 2;
    let sub_h = rows * 4;
    let aspect = (cols as f32 * 0.53) / rows.max(1) as f32;
    let t = if reduced_motion { 0.35 } else { time };
    let is_thumb = variant == LoadingVariant::Thumb || variant == LoadingVariant::Sidebar;
    let is_error = status == LoadingStatus::Error;

    let scan_y = -0.84 + ((t * 0.68) % 1.68);
    let sweep_angle = t * 2.4;
    let inner_spin = -t * 1.65;

    let mut raw = vec![0.0_f32; sub_w * sub_h];
    for sy in 0..sub_h {
        let uy = (sy as f32 + 0.5) / sub_h as f32;
        let ny = uy * 2.0 - 1.0;
        let row_off = sy * sub_w;
        for sx in 0..sub_w {
            let ux = (sx as f32 + 0.5) / sub_w as f32;
            let nx = (ux * 2.0 - 1.0) * aspect;
            let dist = nx.hypot(ny);
            let angle = ny.atan2(nx);

            let lum = if is_error {
                let outer_ring = (-(((dist - 0.58) / 0.042).powi(2))).exp();
                let dash_mask = if (angle * 6.0).cos() > 0.05 { 0.8 } else { 0.2 };
                let slash = if dist < 0.44 { (-(((nx - ny) / 0.05).powi(2))).exp() * 0.85 } else { 0.0 };
                clamp01((outer_ring * dash_mask * 0.55).max(slash))
            } else {
                let mut lum = 0.0_f32;
                let ring_r = if is_thumb { 0.66 } else { 0.56 + (t * 1.8).sin() * 0.012 };
                let ring_band = (-(((dist - ring_r) / if is_thumb { 0.072 } else { 0.038 }).powi(2))).exp();
                let arc_delta = (angle - sweep_angle).sin().atan2((angle - sweep_angle).cos());
                let arc_glow = smoothstep(-1.55, 0.1, arc_delta) * (1.0 - smoothstep(0.05, 0.52, arc_delta));
                let twin_delta = (angle - sweep_angle + std::f32::consts::PI).sin()
                    .atan2((angle - sweep_angle + std::f32::consts::PI).cos());
                let twin_glow = smoothstep(-0.9, 0.1, twin_delta) * (1.0 - smoothstep(0.04, 0.4, twin_delta)) * 0.6;
                let ticks = (angle * 6.0 - sweep_angle * 0.3).cos().max(0.0).powf(14.0) * 0.46;
                lum += ring_band * (0.24 + 0.72 * arc_glow.max(twin_glow) + ticks);

                if !is_thumb {
                    let inner_r = 0.31 + (t * 2.6).sin() * 0.022;
                    let inner_band = (-(((dist - inner_r) / 0.029).powi(2))).exp();
                    let inner_seg = if (angle * 3.0 - inner_spin).cos() > -0.1 { 1.0 } else { 0.16 };
                    lum += inner_band * inner_seg * 0.62;

                    let core_wave = 0.5 + 0.5 * (t * 4.2 - dist * 18.0).sin();
                    let core_mask = (-((dist / 0.13).powi(2))).exp();
                    let pulse_ring = (-(((dist - ((t * 0.45) % 0.56)) / 0.038).powi(2))).exp() * 0.36;
                    lum += core_mask * (0.38 + core_wave * 0.46) + pulse_ring;

                    let scan_band = (-(((ny - scan_y) / 0.068).powi(2))).exp();
                    let aperture_clip = 1.0 - smoothstep(0.48, 0.62, dist);
                    lum += scan_band * aperture_clip * 0.52;

                    let mist_noise = 0.5 + 0.5 * (ux * 30.0 + t * 2.4).sin() * (uy * 26.0 - t * 1.9).cos();
                    let mist_ring = (-(((dist - 0.43) / 0.15).powi(2))).exp() * mist_noise * 0.22;
                    lum += mist_ring;
                } else {
                    let center_dot = (-((dist / 0.24).powi(2))).exp() * (0.55 + 0.35 * (t * 4.0).sin());
                    lum += center_dot;
                }
                lum
            };
            raw[row_off + sx] = clamp01(lum);
        }
    }

    let mut cell_bits = vec![0u8; cols * rows];
    let mut cell_lums = vec![0u8; cols * rows];
    for r in 0..rows {
        for c in 0..cols {
            let mut bits = 0u8;
            let mut lum_sum = 0.0_f32;
            for py in 0..4 {
                let sy = r * 4 + py;
                let row_off = sy * sub_w;
                let bit_base = py * 2;
                let bayer_row = (sy & 7) * 8;
                for px in 0..2 {
                    let sx = c * 2 + px;
                    let v = raw[row_off + sx];
                    lum_sum += v;
                    let jitter = if is_thumb { 0.0 } else { LOADING_BAYER_8X8[bayer_row + (sx & 7)] * 0.18 };
                    if v >= 0.36 + jitter {
                        bits |= BRAILLE_BITMASK[bit_base + px];
                    }
                }
            }
            cell_lums[r * cols + c] = ((lum_sum * 0.125) * 255.0).round().clamp(0.0, 255.0) as u8;
            cell_bits[r * cols + c] = bits;
        }
    }

    BrailleFrameGrid { cols, rows, cell_bits, cell_lums }
}

/// Smoothly dissolves from the Braille orbital loader into the decoded real portrait
/// frame along a soft top-to-bottom scan wavefront + radial focus lock.
pub fn blend_loading_to_portrait_frame(
    loading: &BrailleFrameGrid,
    portrait: &BrailleFrameGrid,
    progress: f32,
) -> BrailleFrameGrid {
    if progress >= 0.995 {
        return portrait.clone();
    }
    if progress <= 0.005 {
        return loading.clone();
    }
    let cols = portrait.cols;
    let rows = portrait.rows;
    let mut cell_bits = vec![0u8; cols * rows];
    let mut cell_lums = vec![0u8; cols * rows];
    let frontier = progress * 1.24 - 0.12;

    for r in 0..rows {
        let ny = r as f32 / rows.saturating_sub(1).max(1) as f32;
        for c in 0..cols {
            let idx = r * cols + c;
            let nx = c as f32 / cols.saturating_sub(1).max(1) as f32 - 0.5;
            let center_dist = nx.abs() * 0.24;
            let hash = (((r * 73 + c * 179) ^ (r * c * 13)) & 31) as f32 / 31.0;
            let local_threshold = ny * 0.58 + center_dist + hash * 0.22;
            let delta = frontier - local_threshold;

            let l_bits = loading.cell_bits.get(idx).copied().unwrap_or(0);
            let l_lum = loading.cell_lums.get(idx).copied().unwrap_or(0);
            let p_bits = portrait.cell_bits.get(idx).copied().unwrap_or(0);
            let p_lum = portrait.cell_lums.get(idx).copied().unwrap_or(0);

            if delta >= 0.0 {
                cell_bits[idx] = p_bits;
                cell_lums[idx] = if delta < 0.095 && p_bits != 0 { p_lum.max(244) } else { p_lum };
            } else if delta > -0.075 && ((r + c) & 1) == 0 && l_bits != 0 {
                cell_bits[idx] = l_bits;
                cell_lums[idx] = l_lum.max(218);
            } else {
                cell_bits[idx] = l_bits;
                cell_lums[idx] = l_lum;
            }
        }
    }

    BrailleFrameGrid { cols, rows, cell_bits, cell_lums }
}

// ---------------------------------------------------------------------------
// asciiCharacter.ts — compact ASCII messenger + phoneme lip-sync helpers
// ---------------------------------------------------------------------------

pub const ASCII_WIDTH: usize = 46;

/// Full 6-state ASCII eye set (the core `EyeState` only carries the 4 halftone
/// states; `Happy`/`Side` exist for the text-art character).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharacterEyeState {
    Open,
    Blink,
    Half,
    Wide,
    Happy,
    Side,
}

impl CharacterEyeState {
    /// Lossy projection onto the halftone core eye state.
    pub fn to_core(self) -> EyeState {
        match self {
            Self::Open | Self::Happy | Self::Side => EyeState::Open,
            Self::Blink => EyeState::Blink,
            Self::Half => EyeState::Half,
            Self::Wide => EyeState::Wide,
        }
    }
}

fn eye_patterns(state: CharacterEyeState) -> (&'static str, &'static str, &'static str, &'static str) {
    match state {
        CharacterEyeState::Open => ("___", "___", "(◉)", "(◉)"),
        CharacterEyeState::Blink => ("___", "___", "═══", "═══"),
        CharacterEyeState::Half => ("\\__", "__/", "(─)", "(─)"),
        CharacterEyeState::Wide => ("/¯¯", "¯¯\\", "(◎)", "(◎)"),
        CharacterEyeState::Happy => ("⌒ ", " ⌒", "^ ^", "^ ^"),
        CharacterEyeState::Side => ("___", "¯¯¯", "(◉ ", " ◉)"),
    }
}

fn mouth_pattern(viseme: MouthViseme) -> &'static str {
    match viseme {
        MouthViseme::Closed => "  ───  ",
        MouthViseme::Rest => "  ─·─  ",
        MouthViseme::Smile => "  ╰─╯  ",
        MouthViseme::Mbp => "  ═══  ",
        MouthViseme::Fv => "  ▀─▀  ",
        MouthViseme::Lth => "  ╰╦╯  ",
        MouthViseme::Small => "  ╰o╯  ",
        MouthViseme::Mid => "  (◯)  ",
        MouthViseme::Round => "  (O)  ",
        MouthViseme::Ee => " ╰───╯ ",
        MouthViseme::Wide => " ╰═══╯ ",
    }
}

/// Extracts a compact phoneme-viseme sequence from spoken text so talking lip-sync
/// hits bilabial (M/B/P), labiodental (F/V), lingual (L/Th), E/I, O/Oo, and A/Ha shapes.
pub fn text_to_viseme_timeline(text: &str) -> Vec<MouthViseme> {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return vec![MouthViseme::Small, MouthViseme::Mid, MouthViseme::Wide, MouthViseme::Round];
    }
    let chars: Vec<char> = cleaned.chars().collect();
    let mut out: Vec<MouthViseme> = Vec::new();
    let mut i = 0usize;

    // Ordered prefix rules mirroring TEXT_VISEME_MAP (first match wins, 2-char window).
    let rules: &[(&[&str], MouthViseme)] = &[
        (&["m", "b", "p"], MouthViseme::Mbp),
        (&["f", "v"], MouthViseme::Fv),
        (&["th", "l", "d", "t", "n"], MouthViseme::Lth),
        (&["ee", "ea", "ie", "i", "y", "ch", "sh", "j"], MouthViseme::Ee),
        (&["oo", "ou", "oh", "ow", "o", "w", "q"], MouthViseme::Round),
        (&["ah", "ha", "aa", "ar", "a"], MouthViseme::Wide),
        (&["uh", "eh", "er", "e", "u", "r", "s", "z", "k", "g", "c"], MouthViseme::Small),
    ];

    while i < chars.len() {
        let ch = chars[i];
        if ch.is_whitespace() || ",.;:!?—-".contains(ch) {
            if !out.is_empty() && out.last() != Some(&MouthViseme::Rest) {
                out.push(MouthViseme::Rest);
            }
            i += 1;
            continue;
        }
        let lower: String = chars[i..(i + 2).min(chars.len())].iter().collect::<String>().to_lowercase();
        let mut matched: Option<(MouthViseme, usize)> = None;
        for (prefixes, viseme) in rules {
            for p in *prefixes {
                if lower.starts_with(p) {
                    matched = Some((*viseme, p.chars().count()));
                    break;
                }
            }
            if matched.is_some() {
                break;
            }
        }
        match matched {
            Some((viseme, adv)) => {
                i += adv;
                if out.last() != Some(&viseme) {
                    out.push(viseme);
                }
            }
            None => {
                i += 1;
            }
        }
    }

    if out.is_empty() {
        vec![MouthViseme::Small, MouthViseme::Mid, MouthViseme::Wide, MouthViseme::Round]
    } else {
        out
    }
}

/// Maps instantaneous audio RMS amplitude (0..1), mood, frame tick, and optional
/// FFT bands / spoken text into an animated MouthViseme for real-time lip sync.
pub fn amplitude_to_viseme(
    amplitude: f32,
    mood: ExpressionMood,
    tick: i64,
    is_speaking: bool,
    bands: Option<&[f32]>,
    spoken_text: Option<&str>,
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
    let syllable_wave = (tick as f32 * 0.85).sin();
    let syllable_mod = (syllable_wave + 1.0) * 0.11;
    let level = (clamped + syllable_mod).min(1.0);

    if level < 0.07 {
        return MouthViseme::Closed;
    }
    if level < 0.22 {
        return MouthViseme::Small;
    }

    let band_at = |bands: &[f32], i: usize| -> f32 { bands.get(i).copied().unwrap_or(0.0) };

    if let Some(text) = spoken_text {
        if !text.trim().is_empty() {
            let timeline = text_to_viseme_timeline(text);
            let syllable_step = (tick.max(0) / 3) as usize;
            let phoneme = timeline[syllable_step % timeline.len()];

            // Keep bilabial M/B/P closures crisp on syllable onset, then release into voiced opening
            if phoneme == MouthViseme::Mbp && (tick % 3 == 0 || clamped < 0.45) {
                return MouthViseme::Mbp;
            }
            if phoneme == MouthViseme::Mbp {
                return if clamped > 0.68 { MouthViseme::Mid } else { MouthViseme::Small };
            }

            if clamped < 0.14 && phoneme != MouthViseme::Fv {
                return if syllable_step % 3 == 0 { MouthViseme::Rest } else { MouthViseme::Small };
            }
            if let Some(b) = bands {
                if b.len() >= 10 {
                    let low = (band_at(b, 1) + band_at(b, 2) + band_at(b, 3)) / 3.0;
                    let mid = (band_at(b, 4) + band_at(b, 5) + band_at(b, 6)) / 3.0;
                    let high = (band_at(b, 7) + band_at(b, 8) + band_at(b, 9)) / 3.0;
                    if high > mid * 1.18 && high > 0.28 {
                        if phoneme == MouthViseme::Fv || phoneme == MouthViseme::Lth || phoneme == MouthViseme::Ee {
                            return phoneme;
                        }
                        return if syllable_step % 2 == 0 { MouthViseme::Fv } else { MouthViseme::Ee };
                    }
                    if low > high * 1.25 && clamped > 0.52 {
                        return if phoneme == MouthViseme::Round { MouthViseme::Round } else { MouthViseme::Wide };
                    }
                }
            }
            if clamped > 0.78 && (phoneme == MouthViseme::Small || phoneme == MouthViseme::Rest) {
                return MouthViseme::Wide;
            }
            return phoneme;
        }
    }

    if let Some(b) = bands {
        if b.len() >= 10 {
            let low = (band_at(b, 1) + band_at(b, 2)) * 0.5;
            let high = (band_at(b, 7) + band_at(b, 8)) * 0.5;
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
    if tick % 6 == 0 { MouthViseme::Round } else { MouthViseme::Wide }
}

/// Resolves the character's eye state from mood, speaking state, and animation tick
/// (4-step blinking rhythm: open -> half -> blink -> half -> open).
pub fn resolve_eye_state(mood: ExpressionMood, tick: i64, is_speaking: bool) -> CharacterEyeState {
    let blink_cycle = tick.rem_euclid(64);
    if blink_cycle == 60 || blink_cycle == 63 {
        return CharacterEyeState::Half;
    }
    if blink_cycle == 61 || blink_cycle == 62 {
        return CharacterEyeState::Blink;
    }
    match mood {
        ExpressionMood::Happy => CharacterEyeState::Happy,
        ExpressionMood::Alert => CharacterEyeState::Wide,
        ExpressionMood::Thinking => {
            if tick.rem_euclid(24) < 12 { CharacterEyeState::Side } else { CharacterEyeState::Half }
        }
        ExpressionMood::Focused => CharacterEyeState::Half,
        ExpressionMood::Listening if tick % 18 == 0 => CharacterEyeState::Wide,
        _ if is_speaking && tick % 29 == 0 => CharacterEyeState::Happy,
        _ => CharacterEyeState::Open,
    }
}

fn pad_center(content: &str, width: usize) -> String {
    let len = content.chars().count();
    if len == width {
        return content.to_string();
    }
    if len > width {
        return content.chars().take(width).collect();
    }
    let total_pad = width - len;
    let left = total_pad / 2;
    let right = total_pad - left;
    format!("{}{}{}", " ".repeat(left), content, " ".repeat(right))
}

/// Renders the Hermes Agent "Nous Girl" cyber-messenger ASCII character portrait.
/// All lines are strictly normalized to ASCII_WIDTH columns to guarantee zero jitter.
pub fn render_nous_girl_ascii(
    mood: ExpressionMood,
    viseme: MouthViseme,
    eye_state: CharacterEyeState,
    energy: f32,
    tick: i64,
) -> Vec<String> {
    let (brow_l, brow_r, eye_l, eye_r) = eye_patterns(eye_state);
    let mouth = mouth_pattern(viseme);

    let active_energy = energy.clamp(0.0, 1.0);
    let phase = tick.rem_euclid(4);
    let spark_a = if active_energy > 0.35 {
        if phase % 2 == 0 { "✦" } else { "·" }
    } else if phase == 0 {
        "·"
    } else {
        " "
    };
    let spark_b = if active_energy > 0.55 {
        if phase % 2 == 1 { "✧" } else { "*" }
    } else {
        " "
    };
    let wing_tip_l = if active_energy > 0.25 && phase < 2 { "≪" } else { "«" };
    let wing_tip_r = if active_energy > 0.25 && phase < 2 { "≫" } else { "»" };

    let cheek_l = match mood {
        ExpressionMood::Happy => "〃",
        ExpressionMood::Listening => "≈",
        _ => " ",
    };
    let cheek_r = cheek_l;
    let visor_mark = match mood {
        ExpressionMood::Thinking => "◈",
        ExpressionMood::Alert => "!",
        _ => "◊",
    };
    let chin_tag = if mood == ExpressionMood::Speaking { " ·───· " } else { "       " };
    let pulse_bar = if active_energy > 0.65 {
        "▓▓▓"
    } else if active_energy > 0.35 {
        "▒▒▒"
    } else if active_energy > 0.12 {
        "░░░"
    } else {
        "───"
    };

    let raw_lines = [
        format!("      {spark_a}       ▄▄████████▄▄       {spark_b}      "),
        format!("    {wing_tip_l}══╗   ▄██▀▀      ▀▀██▄   ╔══{wing_tip_r}    "),
        format!("   /███╗ ▄██▀  ▄██{visor_mark}██▄  ▀██▄ ╔███\\   "),
        format!("  /████╚▄█▀  ▄██████████▄  ▀█▄╝████\\  "),
        format!("  \\█████▀   ███▀  ██  ▀███   ▀█████/  "),
        format!("   \\██▀    ██▀  {brow_l}    {brow_r}  ▀██    ▀██/   "),
        format!("    ▀│    ██▌   {eye_l}    {eye_r}   ▐██    │▀    "),
        format!("     │    ██  {cheek_l}     ╷╷     {cheek_r}  ██    │     "),
        format!("   {spark_b} │    ██▄    {mouth}    ▄██    │ {spark_a}   "),
        format!("     │     ▀██▄  {chin_tag}  ▄██▀     │     "),
        format!("     ╰──    ▀███▄  ──  ▄███▀    ──╯     "),
        format!("             ▀███▄▄▄▄▄▄███▀             "),
        format!("           ▄█▀  ║{pulse_bar}║  ▀█▄           "),
        format!("         ▄██▀  ▄╚════╝▄  ▀██▄         "),
        format!("       ▄██▀  ▄█▀  {visor_mark}   ▀█▄  ▀██▄       "),
        format!("      ███   ██   /  \\   ██   ███      "),
        format!("     ▐██▌   ██  /    \\  ██   ▐██▌     "),
        format!("     ███    ▀█▄/──────\\▄█▀    ███     "),
    ];

    raw_lines.iter().map(|l| pad_center(l, ASCII_WIDTH)).collect()
}

// ---------------------------------------------------------------------------
// shared math helpers
// ---------------------------------------------------------------------------

#[inline]
pub(crate) fn clamp01(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

#[inline]
pub(crate) fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
pub(crate) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let span = edge1 - edge0;
    let t = clamp01((x - edge0) / if span == 0.0 { 1.0 } else { span });
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub(crate) fn exp_approach(current: f32, target: f32, rate: f32, dt: f32) -> f32 {
    let factor = 1.0 - (-rate * dt.max(0.001)).exp();
    current + (target - current) * factor
}