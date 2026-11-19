pub mod ascii3d;
pub mod gpu;
pub mod horizon;
pub mod portrait;
pub mod raster;
pub mod shapes;

pub use ascii3d::*;
pub use gpu::*;
pub use horizon::*;
pub use portrait::*;
pub use raster::*;
pub use shapes::*;

#[cfg(test)]
mod tests {
    use super::*;
    use voxcell_core::canvas::CanvasShapeKind;
    use voxcell_core::{
        AsciiShaderMode, DotProfile, ExpressionMood, EyeState, HorizonMode, MouthViseme,
        PhosphorTheme,
    };

    #[test]
    fn loads_embedded_portrait_and_12_panel_sprite_bank() {
        let assets = portrait_assets();
        assert!(assets.reference.width > 100);
        assert!(assets.reference.height > 100);
        assert_eq!(assets.sprite_bank.len(), 12);
        for panel in &assets.sprite_bank {
            assert_eq!(panel.width, 160);
            assert_eq!(panel.height, 160);
            let non_zero = panel.base_lum.iter().filter(|&&v| v > 0.05).count();
            assert!(non_zero > 200, "expected populated silhouette pixels, got {non_zero}");
        }
    }

    #[test]
    fn natural_blink_curve_stays_bounded_and_triggers_periodically() {
        let mut max_blink = 0.0_f32;
        let mut min_blink = 1.0_f32;
        for step in 0..400 {
            let t = step as f32 * 0.01;
            let b = evaluate_natural_blink_curve(t, 0.0, 3);
            assert!((0.0..=1.0).contains(&b));
            max_blink = max_blink.max(b);
            min_blink = min_blink.min(b);
        }
        assert!(max_blink > 0.9, "expected blink peak > 0.9, got {max_blink}");
        assert!(min_blink < 0.05);
    }

    #[test]
    fn portrait_renders_distinct_moods_and_eye_states() {
        let mut anim = PortraitAnimator::new();
        let m_calm = anim
            .step(
                &PortraitAnimatorInput {
                    time: 1.0,
                    mood: ExpressionMood::Calm,
                    eye_state: EyeState::Open,
                    viseme: MouthViseme::Closed,
                    energy: 0.0,
                    is_speaking: false,
                    is_listening: false,
                    pointer_x: None,
                    pointer_y: None,
                    tap_impulse: 0.0,
                },
                0.016,
                false,
            )
            .clone();

        let calm_rgba = render_portrait_rgba(
            &m_calm,
            &PortraitRenderOptions {
                pixel_w: 160,
                pixel_h: 148,
                cols: 42,
                rows: 20,
                time: 1.0,
                mood: ExpressionMood::Calm,
                viseme: MouthViseme::Closed,
                energy: 0.0,
                is_speaking: false,
                is_listening: false,
                is_loading: false,
                is_thumb: false,
                dot_profile: DotProfile::Balanced,
                bloom: 0.38,
                tint_hex: None,
            },
        );
        assert_eq!(calm_rgba.len(), 160 * 148 * 4);
        assert!(calm_rgba.iter().skip(3).step_by(4).any(|&a| a > 120));

        let m_think = anim
            .step(
                &PortraitAnimatorInput {
                    time: 2.35,
                    mood: ExpressionMood::Thinking,
                    eye_state: EyeState::Blink,
                    viseme: MouthViseme::Wide,
                    energy: 0.85,
                    is_speaking: true,
                    is_listening: false,
                    pointer_x: Some(0.5),
                    pointer_y: Some(-0.2),
                    tap_impulse: 0.6,
                },
                0.016,
                false,
            )
            .clone();
        let think_rgba = render_portrait_rgba(
            &m_think,
            &PortraitRenderOptions {
                pixel_w: 160,
                pixel_h: 148,
                cols: 42,
                rows: 20,
                time: 2.35,
                mood: ExpressionMood::Thinking,
                viseme: MouthViseme::Wide,
                energy: 0.85,
                is_speaking: true,
                is_listening: false,
                is_loading: false,
                is_thumb: false,
                dot_profile: DotProfile::Phosphor,
                bloom: 0.55,
                tint_hex: None,
            },
        );
        let diff_bytes: usize = calm_rgba
            .iter()
            .zip(think_rgba.iter())
            .filter(|(a, b)| a.abs_diff(**b) > 12)
            .count();
        assert!(diff_bytes > 500, "expected visible pixel diff, got {diff_bytes}");
    }

    #[test]
    fn renders_all_four_horizon_modes_and_nine_canvas_shapes() {
        let mut h_anim = HorizonAnimator::new();
        let bands = [0.2, 0.5, 0.8, 0.6, 0.4, 0.7, 0.5, 0.3, 0.6, 0.4, 0.2, 0.1];
        let h_state = h_anim.step(0.65, 0.25, -0.1, &bands, 0.016, false).clone();

        for mode in [
            HorizonMode::Silk,
            HorizonMode::Peaks,
            HorizonMode::Pulse,
            HorizonMode::Ribbon,
        ] {
            let rgba = render_horizon_rgba(
                240,
                64,
                48,
                8,
                1.5,
                &h_state,
                mode,
                "#34D399",
                DotProfile::Balanced,
                0.35,
            );
            assert_eq!(rgba.len(), 240 * 64 * 4);
            let lit = rgba.iter().skip(3).step_by(4).filter(|&&a| a > 20).count();
            assert!(lit > 40, "horizon mode {mode:?} had too few lit pixels ({lit})");
        }

        for &shape in CanvasShapeKind::all() {
            let rgba = render_canvas_dot_rgba(
                96,
                96,
                22,
                11,
                shape,
                1.2,
                ExpressionMood::Happy,
                MouthViseme::Smile,
                0.4,
                false,
                false,
                false,
                None,
                DotProfile::Balanced,
                0.35,
            );
            assert_eq!(rgba.len(), 96 * 96 * 4);
            let lit = rgba.iter().skip(3).step_by(4).filter(|&&a| a > 40).count();
            assert!(lit > 80, "shape {shape:?} had too few lit pixels ({lit})");
        }
    }

    #[test]
    fn renders_3d_ascii_sdf_shaders() {
        for mode in [
            AsciiShaderMode::Solid,
            AsciiShaderMode::Wire,
            AsciiShaderMode::Depth,
            AsciiShaderMode::Holo,
            AsciiShaderMode::Norm,
        ] {
            let frame = render_ascii3d_frame(&Ascii3DParams {
                cols: 44,
                rows: 22,
                time: 1.0,
                yaw: 0.2,
                pitch: 0.05,
                zoom: 1.0,
                mood: ExpressionMood::Speaking,
                viseme: MouthViseme::Mid,
                eye_state: EyeState::Open,
                energy: 0.6,
                is_speaking: true,
                is_listening: false,
                shader_mode: mode,
                bands: [0.4; 12],
            });
            let active = frame.cells.iter().filter(|c| c.ch != ' ').count();
            assert!(active > 50, "ascii3d shader {mode:?} had only {active} cells");
            let rgba = render_ascii3d_rgba(220, 176, &frame, PhosphorTheme::Emerald);
            assert!(rgba.iter().skip(3).step_by(4).any(|&a| a > 100));
        }
    }
}

/// Unit tests mirroring the TypeScript reference suites:
/// asciiPortraitEngine.test.ts, brailleCanvasRaster.test.ts, brailleFieldEngine.test.ts,
/// portraitLoadingEngine.test.ts, asciiCharacter.test.ts.
#[cfg(test)]
mod ts_parity {
    use super::*;
    use voxcell_core::canvas::CanvasShapeKind;
    use voxcell_core::{DotProfile, ExpressionMood, EyeState, HorizonMode, MouthViseme};

    fn input(time: f32, mood: ExpressionMood, viseme: MouthViseme, eye: EyeState, energy: f32, speaking: bool) -> PortraitAnimatorInput {
        PortraitAnimatorInput {
            time,
            mood,
            eye_state: eye,
            viseme,
            energy,
            is_speaking: speaking,
            is_listening: false,
            pointer_x: None,
            pointer_y: None,
            tap_impulse: 0.0,
        }
    }

    /// Steps the animator toward the TS `getTargetMotionFromParams` choreography target.
    fn settled_motion(inp: &PortraitAnimatorInput) -> PortraitMotionState {
        let mut anim = PortraitAnimator::new();
        let mut out = PortraitMotionState::default();
        for k in 0..8 {
            let mut step_inp = inp.clone();
            step_inp.time = inp.time + k as f32 * 0.05;
            out = anim.step(&step_inp, 0.05, false).clone();
        }
        out
    }

    fn ring_field(size: usize, lo: usize, hi: usize) -> PortraitSourceField {
        let mut rgba = vec![0u8; size * size * 4];
        for y in lo..=hi {
            for x in lo..=hi {
                if x == lo || x == hi || y == lo || y == hi {
                    rgba[(y * size + x) * 4 + 3] = 255; // opaque black ink
                }
            }
        }
        build_portrait_source_from_rgba(&rgba, size, size)
    }

    fn field_assets(field: PortraitSourceField) -> PortraitAssets {
        PortraitAssets { reference: field, sprite_bank: Vec::new() }
    }

    fn render_field_lines(assets: &PortraitAssets, motion: &PortraitMotionState, cols: usize, rows: usize, time: f32, mood: ExpressionMood, viseme: MouthViseme, energy: f32, speaking: bool) -> Vec<String> {
        let raw = sample_portrait_luminance_field(cols, rows, time, mood, viseme, energy, speaking, false, motion, assets);
        pack_braille_subpixels_ordered_bayer(&raw, cols, rows).lines()
    }

    // ---- asciiPortraitEngine.test.ts ----------------------------------------

    #[test]
    fn animator_smoothly_ramps_blink_and_mouth_in_one_frame() {
        let mut anim = PortraitAnimator::new();
        let m1 = anim.step(&input(0.0, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.05, false), 0.016, false).clone();
        let m2 = anim.step(&input(0.016, ExpressionMood::Speaking, MouthViseme::Wide, EyeState::Blink, 0.95, true), 0.016, false).clone();
        assert!(m2.blink > m1.blink && m2.blink < 0.9, "smooth blink ramp, got {}", m2.blink);
        assert!(m2.mouth_open > m1.mouth_open && m2.mouth_open < 0.8, "smooth mouth ramp, got {}", m2.mouth_open);
    }

    #[test]
    fn look_target_steers_head_and_gaze() {
        let mut left = PortraitAnimator::new();
        let mut right = PortraitAnimator::new();
        let mut ls = PortraitMotionState::default();
        let mut rs = PortraitMotionState::default();
        for i in 0..20 {
            let mut li = input(i as f32 * 0.016, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.1, false);
            li.pointer_x = Some(-0.9);
            li.pointer_y = Some(-0.8);
            let mut ri = li.clone();
            ri.pointer_x = Some(0.9);
            ri.pointer_y = Some(0.8);
            ls = left.step(&li, 0.016, false).clone();
            rs = right.step(&ri, 0.016, false).clone();
        }
        assert!(ls.head_yaw < 0.0 && rs.head_yaw > 0.0);
        assert!(ls.head_pitch < 0.0 && rs.head_pitch > 0.0);
        assert!(ls.gaze_x < rs.gaze_x);
    }

    #[test]
    fn reduced_motion_zeroes_secondary_hair_lag() {
        let mut anim = PortraitAnimator::new();
        let s = anim.step(&input(0.5, ExpressionMood::Speaking, MouthViseme::Wide, EyeState::Open, 0.8, true), 0.016, true);
        assert_eq!(s.hair_lag_x, 0.0);
        assert_eq!(s.hair_lag_y, 0.0);
        assert_eq!(s.head_sway_x, 0.0);
    }

    #[test]
    fn mouth_has_no_teleport_spikes_and_idle_gaze_saccades() {
        let mut anim = PortraitAnimator::new();
        let dt = 1.0 / 30.0;
        let mut mouth_opens = Vec::new();
        let mut idle_gaze = Vec::new();
        for i in 0..90 {
            let t = i as f32 * dt;
            let speaking = (10..60).contains(&i);
            let viseme = if speaking {
                match i % 4 { 0 => MouthViseme::Wide, 1 => MouthViseme::Ee, _ => MouthViseme::Round }
            } else {
                MouthViseme::Closed
            };
            let st = anim.step(
                &input(t, if speaking { ExpressionMood::Speaking } else { ExpressionMood::Calm }, viseme, EyeState::Open, if speaking { 0.88 } else { 0.08 }, speaking),
                dt,
                false,
            );
            mouth_opens.push(st.mouth_open);
            if !speaking {
                idle_gaze.push(st.gaze_x);
            }
        }
        let max_jump = mouth_opens.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0_f32, f32::max);
        assert!(max_jump < 0.26, "2nd-order damped mouthOpen, got {max_jump}");
        assert!(idle_gaze.iter().any(|g| g.abs() > 0.05), "idle saccade motion");
    }

    #[test]
    fn tap_impulse_excites_pitch_nod_and_brow() {
        let mut anim = PortraitAnimator::new();
        let base = anim.step(&input(0.1, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.08, false), 0.016, false).clone();
        anim.trigger_impulse(ImpulseKind::Tap);
        let tapped = anim.step(&input(0.12, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.08, false), 0.02, false).clone();
        assert!(tapped.head_pitch > base.head_pitch + 0.005, "tap pitch nod");
        assert!(tapped.brow_lift.abs() > base.brow_lift.abs(), "tap brow bounce");
    }

    #[test]
    fn idle_cadence_produces_natural_blinks_within_six_seconds() {
        let mut anim = PortraitAnimator::new();
        let mut saw_blink = false;
        for i in 0..200 {
            let s = anim.step(&input(i as f32 * 0.03, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.08, false), 0.03, false);
            if s.blink > 0.65 {
                saw_blink = true;
                break;
            }
        }
        assert!(saw_blink);
    }

    #[test]
    fn blink_curve_has_dwell_asymmetry_and_double_blinks() {
        let dt = 0.005;
        let samples: Vec<f32> = (0..=3200).map(|i| evaluate_natural_blink_curve(i as f32 * dt, 0.0, 4)).collect();
        let dwell = samples.iter().filter(|&&v| v >= 0.99).count();
        assert!(dwell >= 12, "closed dwell samples, got {dwell}");

        let first_start = samples.iter().position(|&v| v > 0.01).expect("blink within 16s");
        let mut peak_idx = first_start;
        while peak_idx < samples.len() && samples[peak_idx] < 0.99 {
            peak_idx += 1;
        }
        let mut dwell_end = peak_idx;
        while dwell_end < samples.len() && samples[dwell_end] >= 0.99 {
            dwell_end += 1;
        }
        let mut close_end = dwell_end;
        while close_end < samples.len() && samples[close_end] > 0.01 {
            close_end += 1;
        }
        let close_steps = peak_idx - first_start;
        let open_steps = close_end - dwell_end;
        assert!(open_steps as f32 > close_steps as f32 * 1.3, "asymmetric blink open {open_steps} vs close {close_steps}");

        let mut peaks = 0;
        let mut in_blink = false;
        for &v in &samples {
            if !in_blink && v > 0.5 {
                peaks += 1;
                in_blink = true;
            } else if in_blink && v < 0.1 {
                in_blink = false;
            }
        }
        assert!(peaks >= 4, "multiple blinks incl double-blink over 16s, got {peaks}");
    }

    #[test]
    fn pose_blend_avoids_happy_ghosting_and_is_continuous() {
        let mut anim = PortraitAnimator::new();
        let base = anim.step(&input(0.0, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.05, false), 0.016, false).clone();

        let mut happy_rest = base.clone();
        happy_rest.blink = 0.0;
        happy_rest.squint = 0.34;
        let p = resolve_reference_pose_blend(&happy_rest, MouthViseme::Closed, ExpressionMood::Happy, false);
        assert_eq!(p.eye_half_weight, 0.0);
        assert_eq!(p.eye_closed_weight, 0.0);

        let mut focused = base.clone();
        focused.blink = 0.0;
        focused.squint = 0.56;
        let p = resolve_reference_pose_blend(&focused, MouthViseme::Closed, ExpressionMood::Focused, false);
        assert!(p.eye_half_weight > 0.25, "focused half-lid squint");

        let mut b60m = base.clone();
        b60m.blink = 0.60;
        b60m.squint = 0.0;
        let mut b65m = base.clone();
        b65m.blink = 0.65;
        b65m.squint = 0.0;
        let mut b100m = base.clone();
        b100m.blink = 1.0;
        b100m.squint = 0.0;
        let b60 = resolve_reference_pose_blend(&b60m, MouthViseme::Closed, ExpressionMood::Calm, false);
        let b65 = resolve_reference_pose_blend(&b65m, MouthViseme::Closed, ExpressionMood::Calm, false);
        let b100 = resolve_reference_pose_blend(&b100m, MouthViseme::Closed, ExpressionMood::Calm, false);
        let total60 = b60.eye_half_weight + b60.eye_closed_weight;
        let total65 = b65.eye_half_weight + b65.eye_closed_weight;
        assert!((total65 - total60).abs() < 0.15 && b65.eye_closed_weight >= b60.eye_closed_weight);
        assert!(b100.eye_closed_weight > 0.95 && b100.eye_half_weight < 0.05);
    }

    #[test]
    fn rapid_click_impulses_stay_bounded() {
        let mut anim = PortraitAnimator::new();
        for _ in 0..6 {
            anim.trigger_impulse(ImpulseKind::Tap);
            anim.trigger_impulse(ImpulseKind::Mood);
        }
        let s = anim.step(&input(0.1, ExpressionMood::Focused, MouthViseme::Closed, EyeState::Half, 0.08, false), 0.05, false);
        assert!(s.head_pitch < 0.20, "bounded pitch after rapid clicks, got {}", s.head_pitch);
    }

    #[test]
    fn happy_idle_kinematics_span_yaw_nod_and_hair() {
        let mut anim = PortraitAnimator::new();
        let (mut min_yaw, mut max_yaw) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_nod, mut max_nod) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_hair, mut max_hair) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..180 {
            let s = anim.step(&input(i as f32 * 0.03, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.06, false), 0.03, false);
            assert!(s.head_yaw.is_finite() && s.hair_lag_x.is_finite() && s.head_nod_y.is_finite());
            min_yaw = min_yaw.min(s.head_yaw);
            max_yaw = max_yaw.max(s.head_yaw);
            min_nod = min_nod.min(s.head_nod_y);
            max_nod = max_nod.max(s.head_nod_y);
            min_hair = min_hair.min(s.hair_lag_x);
            max_hair = max_hair.max(s.hair_lag_x);
        }
        assert!(max_yaw - min_yaw > 0.22, "happy yaw span {}", max_yaw - min_yaw);
        assert!(max_nod - min_nod > 0.018, "breath nod span {}", max_nod - min_nod);
        assert!(max_hair - min_hair > 0.035, "hair span {}", max_hair - min_hair);
    }

    #[test]
    fn neutral_idle_kinematics_span_yaw_and_hair() {
        let mut anim = PortraitAnimator::new();
        let (mut min_yaw, mut max_yaw) = (f32::INFINITY, f32::NEG_INFINITY);
        let (mut min_hair, mut max_hair) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..180 {
            let s = anim.step(&input(i as f32 * 0.03, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.06, false), 0.03, false);
            min_yaw = min_yaw.min(s.head_yaw);
            max_yaw = max_yaw.max(s.head_yaw);
            min_hair = min_hair.min(s.hair_lag_x);
            max_hair = max_hair.max(s.hair_lag_x);
        }
        assert!(max_yaw - min_yaw > 0.30, "neutral yaw span {}", max_yaw - min_yaw);
        assert!(max_hair - min_hair > 0.055, "neutral hair span {}", max_hair - min_hair);
    }

    #[test]
    fn visemes_articulate_jaw_teeth_tongue_and_lips() {
        let mut anim = PortraitAnimator::new();
        let s_closed = anim.step(&input(0.0, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.05, false), 0.016, false).clone();
        let s_wide = anim.step(&input(0.1, ExpressionMood::Speaking, MouthViseme::Wide, EyeState::Open, 0.9, true), 0.05, false).clone();
        assert!(s_wide.jaw_drop > s_closed.jaw_drop);
        assert!(s_wide.teeth_top > s_closed.teeth_top);
        assert!(s_wide.tongue_show > s_closed.tongue_show);
        let s_fv = anim.step(&input(0.2, ExpressionMood::Speaking, MouthViseme::Fv, EyeState::Open, 0.45, true), 0.05, false).clone();
        assert!(s_fv.lip_bite > 0.2);
        let s_mbp = anim.step(&input(0.3, ExpressionMood::Speaking, MouthViseme::Mbp, EyeState::Open, 0.25, true), 0.05, false).clone();
        assert!(s_mbp.lip_press > 0.2);
    }

    #[test]
    fn source_field_isolates_exterior_and_lights_enclosed_interior() {
        let field = ring_field(40, 8, 32);
        assert_eq!(field.base_lum[40 + 1], 0.0, "exterior corner dark");
        assert!(field.base_lum[20 * 40 + 12] > 0.5, "enclosed interior studio shading");
        assert!(field.base_lum[8 * 40 + 20] > 0.2, "ink rim separation");
    }

    #[test]
    fn reference_field_responds_to_visemes_blinks_and_head_turns() {
        let assets = field_assets(ring_field(40, 8, 32));
        let closed = settled_motion(&input(1.0, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.0, false));
        let wide = settled_motion(&input(1.0, ExpressionMood::Happy, MouthViseme::Wide, EyeState::Open, 0.9, true));
        let a = render_field_lines(&assets, &closed, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.0, false);
        let b = render_field_lines(&assets, &wide, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Wide, 0.9, true);
        assert_ne!(a, b, "mouth viseme change must render differently");

        let mut base = settled_motion(&input(1.0, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.2, false));
        base.blink = 0.0;
        let open_lines = render_field_lines(&assets, &base, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.2, false);
        base.blink = 0.46;
        let half_lines = render_field_lines(&assets, &base, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.2, false);
        base.blink = 1.0;
        let blink_lines = render_field_lines(&assets, &base, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.2, false);
        assert_ne!(open_lines, half_lines);
        assert_ne!(half_lines, blink_lines);

        let mut turned = closed.clone();
        turned.head_yaw = -0.3;
        turned.head_pitch = -0.14;
        let left = render_field_lines(&assets, &turned, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.2, false);
        turned.head_yaw = 0.3;
        turned.head_pitch = 0.14;
        let right = render_field_lines(&assets, &turned, 54, 28, 1.0, ExpressionMood::Happy, MouthViseme::Closed, 0.2, false);
        assert_ne!(left, right, "2.5D head turn shifts perspective");
    }

    #[test]
    fn ten_viseme_reference_frames_render_uniquely() {
        let assets = field_assets(ring_field(96, 16, 80));
        let specs: [(MouthViseme, f32); 10] = [
            (MouthViseme::Closed, 0.08),
            (MouthViseme::Small, 0.36),
            (MouthViseme::Wide, 0.92),
            (MouthViseme::Round, 0.72),
            (MouthViseme::Round, 0.25),
            (MouthViseme::Ee, 0.68),
            (MouthViseme::Lth, 0.56),
            (MouthViseme::Fv, 0.44),
            (MouthViseme::Rest, 0.12),
            (MouthViseme::Mbp, 0.22),
        ];
        let mut outputs = std::collections::HashSet::new();
        for (viseme, energy) in specs {
            let speaking = viseme != MouthViseme::Closed && viseme != MouthViseme::Rest;
            let motion = settled_motion(&input(0.4, ExpressionMood::Speaking, viseme, EyeState::Open, energy, speaking));
            let lines = render_field_lines(&assets, &motion, 92, 42, 0.4, ExpressionMood::Speaking, viseme, energy, speaking);
            outputs.insert(lines.join("\n"));
        }
        assert_eq!(outputs.len(), 10, "all 10 talking panels incl Oh vs Oo must be unique");
    }

    #[test]
    fn sprite_bank_extracts_12_panels_strips_labels_and_blends_poses() {
        let (w, h) = (160usize, 120usize);
        let mut rgba = vec![255u8; w * h * 4];
        for vx in [40usize, 80, 120] {
            for y in 0..h {
                let i = (y * w + vx) * 4;
                rgba[i] = 170;
                rgba[i + 1] = 170;
                rgba[i + 2] = 170;
            }
        }
        for hy in [40usize, 80] {
            for x in 0..w {
                let i = (hy * w + x) * 4;
                rgba[i] = 170;
                rgba[i + 1] = 170;
                rgba[i + 2] = 170;
            }
        }
        for r in 0..3 {
            for c in 0..4 {
                let panel_idx = r * 4 + c;
                let x0 = c * 40 + 2;
                let y0 = r * 40 + 2;
                for dy in 1..=5 {
                    for dx in 1..=5 {
                        let i = ((y0 + dy) * w + (x0 + dx)) * 4;
                        let v = if (2..=4).contains(&dy) && (2..=4).contains(&dx) { 10 } else { 215 };
                        rgba[i] = v;
                        rgba[i + 1] = v;
                        rgba[i + 2] = v;
                    }
                }
                for dy in 6..=32 {
                    for dx in 10..=30 {
                        let dist = ((dx as f32 - 20.0).powi(2) + (dy as f32 - 18.0).powi(2)).sqrt();
                        let is_outline = (dist - 10.0).abs() < 1.3;
                        let is_mouth = dy >= 22 && dy <= 22 + panel_idx / 3 && (dx as isize - 20).abs() <= (panel_idx % 4) as isize + 1;
                        if is_outline || is_mouth {
                            let i = ((y0 + dy) * w + (x0 + dx)) * 4;
                            rgba[i] = 0;
                            rgba[i + 1] = 0;
                            rgba[i + 2] = 0;
                        }
                    }
                }
            }
        }
        let bank = build_portrait_bank_from_sprite_sheet(&rgba, w, h, 80);
        assert_eq!(bank.len(), 12);
        for py in 0..12 {
            for px in 0..14 {
                assert_eq!(bank[0].base_lum[py * 80 + px], 0.0, "label/halo ({px},{py}) cleared");
            }
        }

        let mut anim = PortraitAnimator::new();
        let closed_motion = anim.step(&input(0.0, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.05, false), 0.05, false).clone();
        let wide_motion = anim.step(&input(0.2, ExpressionMood::Speaking, MouthViseme::Wide, EyeState::Open, 0.92, true), 0.06, false).clone();
        let round_motion = anim.step(&input(0.3, ExpressionMood::Speaking, MouthViseme::Round, EyeState::Open, 0.72, true), 0.06, false).clone();
        let blink_motion = anim.step(&input(0.4, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Blink, 0.05, false), 0.06, false).clone();

        let pose_closed = resolve_reference_pose_blend(&closed_motion, MouthViseme::Closed, ExpressionMood::Calm, false);
        let pose_wide = resolve_reference_pose_blend(&wide_motion, MouthViseme::Wide, ExpressionMood::Speaking, true);
        let pose_round = resolve_reference_pose_blend(&round_motion, MouthViseme::Round, ExpressionMood::Speaking, true);
        let pose_blink = resolve_reference_pose_blend(&blink_motion, MouthViseme::Closed, ExpressionMood::Calm, false);
        assert_eq!(pose_closed.mouth_index_a, 0);
        assert!(pose_wide.mouth_index_b == 3 || pose_wide.mouth_index_a == 2);
        assert_eq!(pose_round.mouth_index_b, 7);
        assert!(pose_blink.eye_blend > 0.2 && [5, 6, 10].contains(&pose_blink.eye_index));

        let assets = PortraitAssets { reference: bank[0].clone(), sprite_bank: bank.clone() };
        let frame_closed = render_field_lines(&assets, &closed_motion, 54, 28, 0.0, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false);
        let frame_wide = render_field_lines(&assets, &wide_motion, 54, 28, 0.2, ExpressionMood::Speaking, MouthViseme::Wide, 0.9, true);
        assert_ne!(frame_closed, frame_wide);
    }

    #[test]
    fn rigid_geometry_ignores_hair_shear_and_bayer_dots_stay_stable() {
        let size = 96usize;
        let mut base_lum = vec![0.0_f32; size * size];
        let mut ink_mask = vec![0.0_f32; size * size];
        for y in 14..=86 {
            for x in 38..=58 {
                base_lum[y * size + x] = 0.62;
                ink_mask[y * size + x] = 0.05;
            }
        }
        let field = PortraitSourceField {
            width: size,
            height: size,
            base_lum,
            ink_mask,
            min_ux: 38.0 / 95.0,
            max_ux: 58.0 / 95.0,
            min_uy: 14.0 / 95.0,
            max_uy: 86.0 / 95.0,
        };
        let assets = field_assets(field);
        let mut anim = PortraitAnimator::new();
        let base = anim.step(&input(0.5, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.05, false), 0.016, false).clone();
        let mut zero_rigid = base.clone();
        zero_rigid.head_yaw = 0.0;
        zero_rigid.head_pitch = 0.0;
        zero_rigid.head_tilt = 0.0;
        zero_rigid.head_sway_x = 0.0;
        zero_rigid.head_nod_y = 0.0;
        zero_rigid.shoulder_y = 0.0;
        zero_rigid.blink = 0.0;
        zero_rigid.brow_lift = 0.0;
        zero_rigid.mouth_open = 0.0;
        zero_rigid.jaw_drop = 0.0;
        zero_rigid.listen_pulse = 0.0;

        let mut ma = zero_rigid.clone();
        ma.hair_lag_x = -0.06;
        ma.hair_lag_y = 0.03;
        let mut mb = zero_rigid.clone();
        mb.hair_lag_x = 0.06;
        mb.hair_lag_y = -0.03;
        let fa = render_field_lines(&assets, &ma, 62, 28, 0.4, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false);
        let fb = render_field_lines(&assets, &mb, 62, 28, 1.9, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false);
        assert_eq!(fa, fb, "hair lag/time wave must not shear rigid geometry");

        let mut m1 = zero_rigid.clone();
        m1.head_nod_y = 0.002;
        let mut m2 = zero_rigid;
        m2.head_nod_y = 0.004;
        let f1 = render_field_lines(&assets, &m1, 62, 28, 1.0, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false);
        let f2 = render_field_lines(&assets, &m2, 62, 28, 1.03, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false);
        let mut total_non_empty = 0usize;
        let mut changed = 0usize;
        for r in 0..28 {
            let ca: Vec<char> = f1[r].chars().collect();
            let cb: Vec<char> = f2[r].chars().collect();
            for c in 0..62 {
                if ca[c] != ' ' || cb[c] != ' ' {
                    total_non_empty += 1;
                }
                if ca[c] != cb[c] {
                    changed += 1;
                }
            }
        }
        let churn = if total_non_empty > 0 { changed as f32 / total_non_empty as f32 } else { 1.0 };
        assert!(churn < 0.12, "ordered-Bayer dot churn {:.1}%", churn * 100.0);
    }

    #[test]
    fn reference_feathers_bottom_edge_and_speaking_halo_differs() {
        let size = 96usize;
        let mut rgba = vec![0u8; size * size * 4];
        for y in 16..=94 {
            for x in 16..=94 {
                let i = (y * size + x) * 4;
                let border = x == 16 || x == 94 || y == 16 || y == 94;
                let v = if border { 0 } else { 210 };
                rgba[i] = v;
                rgba[i + 1] = v;
                rgba[i + 2] = v;
                rgba[i + 3] = 255;
            }
        }
        let field = build_portrait_source_from_rgba(&rgba, size, size);
        let assets = field_assets(field);
        let idle_motion = settled_motion(&input(0.4, ExpressionMood::Calm, MouthViseme::Closed, EyeState::Open, 0.05, false));
        let raw = sample_portrait_luminance_field(80, 40, 0.4, ExpressionMood::Calm, MouthViseme::Closed, 0.05, false, false, &idle_motion, &assets);
        let idle_grid = pack_braille_subpixels_ordered_bayer(&raw, 80, 40);
        let bottom_sum: u32 = idle_grid.cell_lums[39 * 80..40 * 80].iter().map(|&v| v as u32).sum();
        assert!(bottom_sum as f32 / 80.0 < 40.0, "feathered bottom row, got {}", bottom_sum as f32 / 80.0);

        let speak_motion = settled_motion(&input(0.4, ExpressionMood::Speaking, MouthViseme::Wide, EyeState::Open, 0.9, true));
        let speak_lines = render_field_lines(&assets, &speak_motion, 80, 40, 0.4, ExpressionMood::Speaking, MouthViseme::Wide, 0.9, true);
        let idle_lines = idle_grid.lines();
        assert_ne!(idle_lines, speak_lines);
    }

    #[test]
    fn math_fallback_eye_region_responds_to_blink() {
        let open = sample_portrait_luminance(-0.18, -0.06, 1.0, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Open, 0.2, false, true, None);
        let blink = sample_portrait_luminance(-0.18, -0.06, 1.0, ExpressionMood::Happy, MouthViseme::Closed, EyeState::Blink, 0.2, false, true, None);
        assert_ne!(open, blink);
    }

    // ---- brailleCanvasRaster.test.ts ----------------------------------------

    #[test]
    fn hex_parse_and_mix_match_ts() {
        assert_eq!(parse_hex_rgb("#34D399"), [52, 211, 153]);
        assert_eq!(parse_hex_rgb("#FFF"), [255, 255, 255]);
        assert_eq!(parse_hex_rgb("invalid"), [230, 240, 236]);
        assert_eq!(mix_rgb([0, 0, 0], [255, 255, 255], 0.0), [0, 0, 0]);
        assert_eq!(mix_rgb([0, 0, 0], [255, 255, 255], 1.0), [255, 255, 255]);
        assert_eq!(mix_rgb([0, 0, 0], [255, 255, 255], 0.5), [128, 128, 128]);
    }

    #[test]
    fn portrait_and_shape_palettes_shift_with_state() {
        let idle = resolve_portrait_tone_palette(ExpressionMood::Happy, false, false, false, false, 0.08, None);
        let thinking = resolve_portrait_tone_palette(ExpressionMood::Thinking, false, false, false, false, 0.12, None);
        let speaking = resolve_portrait_tone_palette(ExpressionMood::Happy, true, false, false, false, 0.85, None);
        assert_ne!(idle[0].rgb, thinking[0].rgb);
        assert_eq!(thinking[0].rgb, parse_hex_rgb("#B48448"));
        assert_eq!(speaking[0].rgb, parse_hex_rgb("#BAE6FD"));
        assert!(speaking[0].alpha > idle[0].alpha);

        let low = parse_hex_rgb("#4D7C0F");
        let mid = parse_hex_rgb("#84CC16");
        let high = parse_hex_rgb("#ECFCCB");
        let shape_idle = resolve_shape_tone_palette(low, mid, high, false, false, 0.08);
        let shape_think = resolve_shape_tone_palette(low, mid, high, true, false, 0.25);
        assert_ne!(shape_idle[2].rgb, shape_think[2].rgb);
    }

    #[test]
    fn tonal_diameter_scale_modulates_coverage_with_circular_aa() {
        let alpha_sum = |bytes: &[u8]| -> u64 { bytes.iter().skip(3).step_by(4).map(|&a| a as u64).sum() };
        let mut small = vec![0u8; 16 * 16 * 4];
        let mut large = vec![0u8; 16 * 16 * 4];
        let pal_small = [TonalTier { rgb: [143, 166, 158], alpha: 0.9, diameter_scale: 0.65, bloom_weight: 0.0 }; 5];
        let pal_large = [TonalTier { rgb: [255, 255, 255], alpha: 0.9, diameter_scale: 1.3, bloom_weight: 0.0 }; 5];
        let base_dot_px = 3.0 * profile_scale(DotProfile::Balanced);
        let dot_small = TonalDot { cx: 7.0, cy: 7.0, radius: (base_dot_px * 0.65).max(0.65) * 0.5, tier: 0 };
        let dot_large = TonalDot { cx: 7.0, cy: 7.0, radius: (base_dot_px * 1.3).max(0.65) * 0.5, tier: 0 };
        rasterize_tonal_dots_rgba(&mut small, 16, 16, &[dot_small], &pal_small, DotProfile::Balanced, 0.0);
        rasterize_tonal_dots_rgba(&mut large, 16, 16, &[dot_large], &pal_large, DotProfile::Balanced, 0.0);
        let sc = alpha_sum(&small);
        let lc = alpha_sum(&large);
        assert!(sc > 0 && lc as f32 > sc as f32 * 1.8, "diameter coverage {sc} vs {lc}");
        let center_a = large[(7 * 16 + 7) * 4 + 3];
        let outer_a = large[(5 * 16 + 5) * 4 + 3];
        assert!(center_a > outer_a, "circular falloff {center_a} vs {outer_a}");
    }

    #[test]
    fn source_over_keeps_bright_cores_luminous_over_dim_halos() {
        let mut bytes = vec![0u8; 12 * 12 * 4];
        let palette = [
            TonalTier { rgb: [16, 20, 24], alpha: 0.35, diameter_scale: 1.5, bloom_weight: 0.0 },
            TonalTier { rgb: [255, 255, 255], alpha: 0.95, diameter_scale: 0.95, bloom_weight: 0.0 },
            TonalTier { rgb: [255, 255, 255], alpha: 0.95, diameter_scale: 0.95, bloom_weight: 0.0 },
            TonalTier { rgb: [255, 255, 255], alpha: 0.95, diameter_scale: 0.95, bloom_weight: 0.0 },
            TonalTier { rgb: [255, 255, 255], alpha: 0.95, diameter_scale: 0.95, bloom_weight: 0.0 },
        ];
        let base_dot_px = 3.0 * profile_scale(DotProfile::Balanced);
        let halo = TonalDot { cx: 5.0, cy: 5.0, radius: base_dot_px * 1.5 * 0.5, tier: 0 };
        let core = TonalDot { cx: 5.2, cy: 5.2, radius: base_dot_px * 0.95 * 0.5, tier: 1 };
        rasterize_tonal_dots_rgba(&mut bytes, 12, 12, &[halo, core], &palette, DotProfile::Balanced, 0.0);
        let idx = (5 * 12 + 5) * 4;
        assert!(bytes[idx + 3] > 230, "combined alpha {}", bytes[idx + 3]);
        assert!(bytes[idx] > 235 && bytes[idx + 1] > 235 && bytes[idx + 2] > 235, "white core dominates");
    }

    // ---- brailleFieldEngine.test.ts -----------------------------------------

    fn horizon_motion(energy: f32, px: f32, py: f32, bands: [f32; 12]) -> HorizonMotionState {
        HorizonMotionState { energy, pointer_x: px, pointer_y: py, bands, pointer_vel_x: 0.0, wake_phase: 1.4 }
    }

    #[test]
    fn horizon_dimensions_and_clean_top_row() {
        let m = horizon_motion(0.35, 0.25, -0.2, [0.4; 12]);
        let grid = sample_horizon_grid(80, 14, 1.2, &m, HorizonMode::Silk);
        assert_eq!(grid.cell_bits.len(), 80 * 14);
        assert!(grid.cell_bits[..80].iter().all(|&b| b == 0), "top row fades clean");
        assert!(grid.cell_bits.iter().any(|&b| b != 0));
    }

    #[test]
    fn horizon_responds_to_pointer_and_energy() {
        let calm = sample_horizon_grid(60, 12, 0.5, &horizon_motion(0.05, -0.8, 0.0, [0.0; 12]), HorizonMode::Silk);
        let active = sample_horizon_grid(60, 12, 0.5, &horizon_motion(0.85, 0.8, -0.5, [0.9; 12]), HorizonMode::Silk);
        assert_ne!(calm.cell_bits, active.cell_bits);
    }

    #[test]
    fn horizon_animator_damps_energy_pointer_and_bands() {
        let mut anim = HorizonAnimator::new();
        let s0 = anim.step(0.05, 0.0, 0.0, &[0.05; 12], 1.0 / 30.0, false).clone();
        let s1 = anim.step(0.95, 0.9, -0.5, &[0.95; 12], 1.0 / 30.0, false).clone();
        assert!(s1.energy > s0.energy && s1.energy < 0.45, "energy ramp {}", s1.energy);
        assert!(s1.pointer_x > 0.0 && s1.pointer_x < 0.5, "pointer shift {}", s1.pointer_x);
        assert!(s1.bands[0] > 0.05 && s1.bands[0] < 0.6, "band attack {}", s1.bands[0]);
    }

    #[test]
    fn horizon_interpolates_alternating_fft_bands() {
        let bands = [1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0];
        let grid = sample_horizon_grid(72, 14, 0.8, &horizon_motion(0.75, 0.0, 0.0, bands), HorizonMode::Silk);
        let lit = grid.cell_bits.iter().filter(|&&b| b != 0).count();
        assert!(lit > 80, "crest density across alternating bands, got {lit}");
    }

    #[test]
    fn horizon_wake_tracking_keeps_row_zero_clean() {
        let mut anim = HorizonAnimator::new();
        anim.step(0.4, -0.6, 0.0, &[0.4; 12], 1.0 / 30.0, false);
        let swept = anim.step(1.0, 0.7, -0.8, &[1.0; 12], 1.0 / 30.0, false).clone();
        assert!(swept.pointer_vel_x > 0.1, "rightward wake vel {}", swept.pointer_vel_x);
        assert!(swept.wake_phase > 0.0);
        let grid = sample_horizon_grid(80, 14, 2.1, &swept, HorizonMode::Silk);
        assert!(grid.cell_bits[..80].iter().all(|&b| b == 0), "row 0 clean under wake");
    }

    #[test]
    fn ribbon_style_tapers_edges() {
        let grid = sample_horizon_grid(80, 6, 1.4, &horizon_motion(0.45, 0.15, 0.0, [0.0; 12]), HorizonMode::Ribbon);
        let lit = grid.cell_bits.iter().filter(|&&b| b != 0).count();
        assert!(lit > 35, "ribbon dots, got {lit}");
        for r in 0..6 {
            assert_eq!(grid.cell_bits[r * 80], 0, "left taper row {r}");
            assert_eq!(grid.cell_bits[r * 80 + 79], 0, "right taper row {r}");
        }
    }

    #[test]
    fn horizon_sustains_bottom_rows_and_peak_cells() {
        let grid = sample_horizon_grid(120, 24, 1.8, &horizon_motion(0.42, 0.15, 0.1, [0.5; 12]), HorizonMode::Silk);
        assert!(grid.cell_bits[..120].iter().all(|&b| b == 0));
        let bottom_lit = grid.cell_bits[22 * 120..24 * 120].iter().filter(|&&b| b != 0).count();
        assert!(bottom_lit >= 20, "bottom bathymetry dots, got {bottom_lit}");
        assert!(grid.cell_lums.iter().any(|&v| v >= 3), "peak-weighted crest cells");
    }

    #[test]
    fn silk_has_high_contrast_traces_and_quiet_background() {
        let grid = sample_horizon_grid(180, 28, 0.9, &horizon_motion(0.55, 0.1, 0.0, [0.45; 12]), HorizonMode::Silk);
        let high = grid.cell_lums.iter().filter(|&&v| v >= 180).count();
        let empty = grid.cell_lums.iter().filter(|&&v| v == 0).count();
        assert!(high >= 40, "luminous carrier/pin cells, got {high}");
        assert!(empty as f32 > grid.cell_lums.len() as f32 * 0.25, "quiet background {empty}/{}", grid.cell_lums.len());
    }

    // ---- portraitLoadingEngine.test.ts --------------------------------------

    #[test]
    fn loader_dimensions_animate_and_support_thumb_error() {
        let f1 = render_portrait_loading_frame(62, 28, 0.2, LoadingVariant::Stage, LoadingStatus::Loading, false);
        let f2 = render_portrait_loading_frame(62, 28, 1.1, LoadingVariant::Stage, LoadingStatus::Loading, false);
        assert_eq!(f1.cell_bits.len(), 62 * 28);
        let glyphs1 = f1.cell_bits.iter().filter(|&&b| b != 0).count();
        assert!(glyphs1 > 40, "loader glyphs, got {glyphs1}");
        assert_ne!(f1.cell_bits, f2.cell_bits, "loader animates");

        let thumb = render_portrait_loading_frame(22, 11, 0.5, LoadingVariant::Thumb, LoadingStatus::Loading, false);
        assert!(thumb.cell_bits.iter().filter(|&&b| b != 0).count() > 5);
        let err = render_portrait_loading_frame(62, 28, 0.5, LoadingVariant::Stage, LoadingStatus::Error, false);
        assert!(err.cell_bits.iter().filter(|&&b| b != 0).count() > 20);
    }

    #[test]
    fn materialize_progress_eases_out_and_skips_under_reduced_motion() {
        assert_eq!(compute_materialize_progress(0.0, false), 0.0);
        let mid = compute_materialize_progress(0.21, false);
        assert!(mid > 0.5 && mid < 1.0, "ease-out mid, got {mid}");
        assert_eq!(compute_materialize_progress(1.0, false), 1.0);
        assert_eq!(compute_materialize_progress(0.0, true), 1.0);
    }

    #[test]
    fn loading_blend_scans_wavefront_into_portrait() {
        let (cols, rows) = (20usize, 8usize);
        let loading = render_portrait_loading_frame(cols, rows, 0.3, LoadingVariant::Hero, LoadingStatus::Loading, false);
        let portrait = BrailleFrameGrid {
            cols,
            rows,
            cell_bits: vec![0xFF; cols * rows],
            cell_lums: vec![240; cols * rows],
        };
        let at_start = blend_loading_to_portrait_frame(&loading, &portrait, 0.0);
        assert_eq!(at_start.cell_bits, loading.cell_bits);
        let at_mid = blend_loading_to_portrait_frame(&loading, &portrait, 0.5);
        assert_ne!(at_mid.cell_bits, loading.cell_bits);
        assert_ne!(at_mid.cell_bits, portrait.cell_bits);
        let at_end = blend_loading_to_portrait_frame(&loading, &portrait, 1.0);
        assert_eq!(at_end.cell_bits, portrait.cell_bits);
        assert!(at_mid.cell_lums[4 * cols..5 * cols].iter().any(|&v| v >= 215), "luminous wavefront");
    }

    // ---- asciiCharacter.test.ts ---------------------------------------------

    #[test]
    fn nous_girl_fixed_width_and_unique_viseme_mouths() {
        let visemes = [
            MouthViseme::Closed, MouthViseme::Rest, MouthViseme::Smile, MouthViseme::Mbp,
            MouthViseme::Fv, MouthViseme::Lth, MouthViseme::Small, MouthViseme::Mid,
            MouthViseme::Round, MouthViseme::Ee, MouthViseme::Wide,
        ];
        let mut mouths = std::collections::HashSet::new();
        for v in visemes {
            let frame = render_nous_girl_ascii(ExpressionMood::Speaking, v, CharacterEyeState::Open, 0.7, 12);
            assert_eq!(frame.len(), 18);
            for line in &frame {
                assert_eq!(line.chars().count(), ASCII_WIDTH);
            }
            mouths.insert(frame[8].clone());
        }
        assert_eq!(mouths.len(), visemes.len());
    }

    #[test]
    fn text_timeline_extracts_phoneme_visemes() {
        let timeline = text_to_viseme_timeline("mab fava thol ee");
        for expected in [MouthViseme::Mbp, MouthViseme::Fv, MouthViseme::Lth, MouthViseme::Round, MouthViseme::Ee, MouthViseme::Wide, MouthViseme::Rest] {
            assert!(timeline.contains(&expected), "missing {expected:?} in {timeline:?}");
        }
    }

    #[test]
    fn amplitude_maps_silence_energy_bands_and_text() {
        assert_eq!(amplitude_to_viseme(0.0, ExpressionMood::Calm, 1, false, None, None), MouthViseme::Closed);
        assert_eq!(amplitude_to_viseme(0.0, ExpressionMood::Happy, 1, false, None, None), MouthViseme::Smile);
        assert_eq!(amplitude_to_viseme(0.0, ExpressionMood::Listening, 1, false, None, None), MouthViseme::Rest);
        assert_eq!(amplitude_to_viseme(0.01, ExpressionMood::Speaking, 4, true, None, None), MouthViseme::Small);
        assert_eq!(amplitude_to_viseme(0.85, ExpressionMood::Speaking, 2, true, None, None), MouthViseme::Wide);

        let text = "Two mcp duet approvals for your morning brief.";
        let mut hit = std::collections::HashSet::new();
        for tick in 1..=40 {
            let bands: Vec<f32> = (0..12)
                .map(|idx| (0.45 + 0.4 * (tick as f32 * 0.5 + idx as f32 * 0.8).sin()).clamp(0.05, 1.0))
                .collect();
            hit.insert(amplitude_to_viseme(0.5, ExpressionMood::Speaking, tick, true, Some(&bands), Some(text)));
        }
        assert!(hit.len() >= 6, "diverse phoneme visemes, got {hit:?}");
    }

    #[test]
    fn eye_state_cycle_and_mood_overrides() {
        assert_eq!(resolve_eye_state(ExpressionMood::Calm, 61, false), CharacterEyeState::Blink);
        assert_eq!(resolve_eye_state(ExpressionMood::Happy, 10, false), CharacterEyeState::Happy);
        assert_eq!(resolve_eye_state(ExpressionMood::Alert, 10, false), CharacterEyeState::Wide);
        assert_eq!(resolve_eye_state(ExpressionMood::Calm, 10, false), CharacterEyeState::Open);
        assert_eq!(resolve_eye_state(ExpressionMood::Calm, 60, false), CharacterEyeState::Half);
        assert_eq!(resolve_eye_state(ExpressionMood::Calm, 63, false), CharacterEyeState::Half);
    }

    // ---- brailleIdleLoops.test.ts -------------------------------------------

    #[test]
    fn idle_sets_define_36_unique_actions() {
        assert_eq!(IDLE_SET_ORDER.len(), 6);
        let mut seen = std::collections::HashSet::new();
        for set in IDLE_SET_ORDER {
            let meta = idle_set_meta(set);
            assert_eq!(meta.actions.len(), 6);
            for action in &meta.actions {
                assert!(!action.id.is_empty() && !action.label.is_empty());
                assert!(seen.insert(action.id), "duplicate action id {}", action.id);
            }
        }
        assert_eq!(seen.len(), 36);
    }

    #[test]
    fn primary_shapes_map_one_to_one_across_idle_sets() {
        let primary = [
            CanvasShapeKind::HermesOrb,
            CanvasShapeKind::AngelBot,
            CanvasShapeKind::GhostShell,
            CanvasShapeKind::LotusCore,
            CanvasShapeKind::CatBot,
            CanvasShapeKind::FoxSpirit,
        ];
        let resolved: std::collections::HashSet<_> = primary.iter().map(|&s| resolve_idle_set(s)).collect();
        assert_eq!(resolved.len(), 6);
    }

    #[test]
    fn idle_action_slots_step_rest_and_wrap() {
        for set in IDLE_SET_ORDER {
            let mut visited = std::collections::HashSet::new();
            for idx in 0..6 {
                let active = evaluate_idle_action_step(set, idx as f32 * IDLE_ACTION_SLOT_SECONDS + 1.4, false);
                assert_eq!(active.action_index, idx);
                assert!(!active.is_resting);
                assert_eq!(active.envelope, 1.0);
                visited.insert(active.action_id);

                let rest = evaluate_idle_action_step(
                    set,
                    idx as f32 * IDLE_ACTION_SLOT_SECONDS + IDLE_ACTIVE_END_SECONDS + 0.35,
                    false,
                );
                assert_eq!(rest.action_index, idx);
                assert!(rest.is_resting);
                assert_eq!(rest.envelope, 0.0);
                assert_eq!(rest.pose.hand, IdleHandPose::None);
                assert_eq!(rest.pose.prop, IdlePropKind::None);
            }
            assert_eq!(visited.len(), 6);
            let wrapped = evaluate_idle_action_step(set, IDLE_FULL_LOOP_SECONDS + 1.4, false);
            assert_eq!(wrapped.action_index, 0);
            assert!(!wrapped.is_resting);
        }
    }

    #[test]
    fn idle_reduced_motion_returns_neutral_rest() {
        for set in IDLE_SET_ORDER {
            let frame = evaluate_idle_action_step(set, 1.5, true);
            assert!(frame.is_resting);
            assert_eq!(frame.envelope, 0.0);
            assert_eq!(frame.pose.head_x, 0.0);
            assert_eq!(frame.pose.head_y, 0.0);
            assert_eq!(frame.pose.tilt, 0.0);
            assert_eq!(frame.pose.hand, IdleHandPose::None);
            assert_eq!(frame.pose.prop, IdlePropKind::None);
        }
    }

    #[test]
    fn idle_action_poses_are_distinct_and_transition_smoothly() {
        for set in IDLE_SET_ORDER {
            // Every action must produce a distinct pose at its expressive peak.
            let mut poses = std::collections::HashSet::new();
            for idx in 0..6 {
                let frame = evaluate_idle_action_step(set, idx as f32 * IDLE_ACTION_SLOT_SECONDS + 1.45, false);
                let key = format!("{:?}", frame.pose);
                assert!(poses.insert(key), "{set:?} action {idx} pose not distinct");
            }

            // Anticipation -> peak -> settle without abrupt pops.
            let mut prev = evaluate_idle_action_step(set, 0.0, false);
            for step in 1..=64 {
                let curr = evaluate_idle_action_step(set, step as f32 * (IDLE_ACTIVE_END_SECONDS / 64.0), false);
                assert!(
                    (curr.pose.head_y - prev.pose.head_y).abs() < 0.035,
                    "{set:?} headY jumped at step {step}"
                );
                assert!(
                    (curr.pose.tilt - prev.pose.tilt).abs() < 0.055,
                    "{set:?} tilt jumped at step {step}"
                );
                prev = curr;
            }
        }
    }
}
