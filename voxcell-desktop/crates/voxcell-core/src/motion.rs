//! Motion choreography helpers — port of `src/utils/motionChoreography.ts` (spec P1-05).
//! See docs/RUNE_DESKTOP_SPEC.md §5.1. Owned by the layout-motion workstream.
//!
//! Pure functions, no framework imports; feeds `rune-motion` upstream. Port notes:
//! - TS optional `StaggerOptions` fields become `Option<T>` (`None` = TS
//!   `undefined`/`null`, resolved with the same `??` defaults).
//! - `Math.round` on the non-negative stagger result matches Rust `f64::round`
//!   (half away from zero == half up for values >= 0).
//! - `Number(x.toFixed(2))` becomes [`round2`].

/// Default stagger step between successive items, in ms (TS `stepMs ?? 30`).
const DEFAULT_STEP_MS: f64 = 30.0;
/// Default cap on the raw stagger delay, in ms (TS `maxDelayMs ?? 240`).
const DEFAULT_MAX_DELAY_MS: f64 = 240.0;
/// Lower/upper bounds of the speed multiplier (TS `Math.max(0.25, Math.min(2.5, ...))`).
const MIN_SPEED_SCALE: f64 = 0.25;
const MAX_SPEED_SCALE: f64 = 2.5;

/// Default scroll-top fade threshold in px (TS `thresholdPx = 12`).
pub const SCROLL_TOP_FADE_THRESHOLD_PX: f64 = 12.0;

/// GPU transform coordinates for a gliding active-tab indicator inside a container.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlidingIndicatorRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub ready: bool,
}

impl SlidingIndicatorRect {
    /// The zeroed, not-ready rect returned when geometry is missing/invalid.
    const NOT_READY: Self = Self { x: 0.0, y: 0.0, width: 0.0, height: 0.0, ready: false };
}

/// Container geometry for [`compute_sliding_indicator_rect`]; scroll offsets are
/// optional and default to 0 (TS `scrollLeft?` / `scrollTop?`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorContainerRect {
    pub left: f64,
    pub top: f64,
    pub scroll_left: Option<f64>,
    pub scroll_top: Option<f64>,
}

/// Measured item (tab) geometry for [`compute_sliding_indicator_rect`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct IndicatorItemRect {
    pub left: f64,
    pub top: f64,
    pub width: f64,
    pub height: f64,
}

/// Optional knobs for [`compute_stagger_delay_ms`]; `None` fields use the TS
/// defaults (step 30ms, cap 240ms, speed 1.0, reduced motion off).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StaggerOptions {
    pub step_ms: Option<f64>,
    pub max_delay_ms: Option<f64>,
    pub speed_scale: Option<f64>,
    pub reduced_motion: Option<bool>,
}

/// Computes deterministic, speed-scaled stagger delay in milliseconds for
/// list/card cascades. Clamped to keep longer lists snappy and zeroed when
/// reduced motion is active.
///
/// Mirrors `computeStaggerDelayMs(index, options)`:
/// - non-finite `index` is treated as 0, then floored and clamped at 0;
/// - negative step/cap clamp to 0; speed clamps to `[0.25, 2.5]`;
/// - result is `round(min(index * step, cap) / speed)`, whole milliseconds.
pub fn compute_stagger_delay_ms(index: f64, options: StaggerOptions) -> f64 {
    if options.reduced_motion.unwrap_or(false) {
        return 0.0;
    }
    let safe_index = if index.is_finite() { index.floor().max(0.0) } else { 0.0 };
    let step_ms = options.step_ms.unwrap_or(DEFAULT_STEP_MS).max(0.0);
    let max_delay_ms = options.max_delay_ms.unwrap_or(DEFAULT_MAX_DELAY_MS).max(0.0);
    let speed = options
        .speed_scale
        .unwrap_or(1.0)
        .clamp(MIN_SPEED_SCALE, MAX_SPEED_SCALE);
    let raw = (safe_index * step_ms).min(max_delay_ms);
    (raw / speed).round()
}

/// Rounds to 2 decimal places, mirroring `Number(x.toFixed(2))` for the
/// non-negative pixel geometry this module produces.
fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Computes GPU transform coordinates for a gliding active tab indicator inside
/// a container. Returns a zeroed `ready: false` rect when either rect is missing
/// or the item has non-positive width/height.
///
/// Mirrors `computeSlidingIndicatorRect(containerRect, itemRect)`.
pub fn compute_sliding_indicator_rect(
    container_rect: Option<IndicatorContainerRect>,
    item_rect: Option<IndicatorItemRect>,
) -> SlidingIndicatorRect {
    let (container, item) = match (container_rect, item_rect) {
        (Some(c), Some(i)) if i.width > 0.0 && i.height > 0.0 => (c, i),
        _ => return SlidingIndicatorRect::NOT_READY,
    };
    let scroll_left = container.scroll_left.unwrap_or(0.0);
    let scroll_top = container.scroll_top.unwrap_or(0.0);
    SlidingIndicatorRect {
        x: round2(item.left - container.left + scroll_left),
        y: round2(item.top - container.top + scroll_top),
        width: round2(item.width),
        height: round2(item.height),
        ready: true,
    }
}

/// Top edge scroll fade should only appear once content has actually scrolled
/// under the viewport, keeping top messages at `scrollTop === 0` crisp and
/// un-dimmed. Uses the default 12px threshold.
///
/// Mirrors `shouldShowScrollTopFade(scrollTop)`.
pub fn should_show_scroll_top_fade(scroll_top: f64) -> bool {
    should_show_scroll_top_fade_with_threshold(scroll_top, SCROLL_TOP_FADE_THRESHOLD_PX)
}

/// [`should_show_scroll_top_fade`] with an explicit threshold (TS
/// `shouldShowScrollTopFade(scrollTop, thresholdPx)`). Non-finite scroll
/// positions never show the fade.
pub fn should_show_scroll_top_fade_with_threshold(scroll_top: f64, threshold_px: f64) -> bool {
    if !scroll_top.is_finite() {
        return false;
    }
    scroll_top > threshold_px
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(step_ms: Option<f64>, max_delay_ms: Option<f64>, speed_scale: Option<f64>) -> StaggerOptions {
        StaggerOptions { step_ms, max_delay_ms, speed_scale, reduced_motion: None }
    }

    // Mirrors motionChoreography.test.ts:
    // "computeStaggerDelayMs clamps long lists and respects speedScale + reducedMotion"
    #[test]
    fn stagger_clamps_long_lists_and_respects_speed_and_reduced_motion() {
        assert_eq!(compute_stagger_delay_ms(0.0, StaggerOptions::default()), 0.0);
        assert_eq!(compute_stagger_delay_ms(3.0, opts(Some(30.0), Some(240.0), None)), 90.0);
        assert_eq!(compute_stagger_delay_ms(20.0, opts(Some(30.0), Some(240.0), None)), 240.0);
        assert_eq!(compute_stagger_delay_ms(4.0, opts(Some(30.0), None, Some(2.0))), 60.0);
        assert_eq!(
            compute_stagger_delay_ms(
                4.0,
                StaggerOptions { reduced_motion: Some(true), ..StaggerOptions::default() }
            ),
            0.0
        );
        assert_eq!(compute_stagger_delay_ms(-3.0, StaggerOptions::default()), 0.0);
    }

    #[test]
    fn stagger_defaults_and_speed_clamping() {
        // Defaults: step 30, cap 240 -> long lists cap out.
        assert_eq!(compute_stagger_delay_ms(100.0, StaggerOptions::default()), 240.0);
        // speedScale below 0.25 clamps up to 0.25 (slower cascade, 4x delay).
        assert_eq!(compute_stagger_delay_ms(4.0, opts(Some(30.0), None, Some(0.1))), 480.0);
        // speedScale above 2.5 clamps down to 2.5.
        assert_eq!(compute_stagger_delay_ms(4.0, opts(Some(30.0), None, Some(10.0))), 48.0);
        // Negative step/cap clamp to 0.
        assert_eq!(compute_stagger_delay_ms(5.0, opts(Some(-10.0), None, None)), 0.0);
        assert_eq!(compute_stagger_delay_ms(5.0, opts(None, Some(-240.0), None)), 0.0);
        // Fractional index floors; non-finite index behaves as 0.
        assert_eq!(compute_stagger_delay_ms(2.9, StaggerOptions::default()), 60.0);
        assert_eq!(compute_stagger_delay_ms(f64::NAN, StaggerOptions::default()), 0.0);
        assert_eq!(compute_stagger_delay_ms(f64::INFINITY, StaggerOptions::default()), 0.0);
        // Half-millisecond results round half up, like Math.round.
        assert_eq!(compute_stagger_delay_ms(1.0, opts(Some(25.0), None, Some(2.0))), 13.0);
        // Reduced motion wins over any other knob.
        assert_eq!(
            compute_stagger_delay_ms(
                20.0,
                StaggerOptions {
                    step_ms: Some(30.0),
                    max_delay_ms: Some(240.0),
                    speed_scale: Some(2.0),
                    reduced_motion: Some(true),
                }
            ),
            0.0
        );
    }

    // Mirrors: "computeSlidingIndicatorRect calculates container-relative geometry accurately"
    #[test]
    fn sliding_indicator_container_relative_geometry() {
        assert!(!compute_sliding_indicator_rect(None, None).ready);
        let rect = compute_sliding_indicator_rect(
            Some(IndicatorContainerRect { left: 100.0, top: 40.0, scroll_left: Some(24.0), scroll_top: Some(0.0) }),
            Some(IndicatorItemRect { left: 180.0, top: 44.0, width: 72.0, height: 32.0 }),
        );
        assert_eq!(
            rect,
            SlidingIndicatorRect { x: 104.0, y: 4.0, width: 72.0, height: 32.0, ready: true }
        );
    }

    #[test]
    fn sliding_indicator_not_ready_on_missing_or_degenerate_geometry() {
        let container = IndicatorContainerRect { left: 0.0, top: 0.0, scroll_left: None, scroll_top: None };
        let zeroed = SlidingIndicatorRect { x: 0.0, y: 0.0, width: 0.0, height: 0.0, ready: false };
        // Missing either rect.
        assert_eq!(compute_sliding_indicator_rect(Some(container), None), zeroed);
        assert_eq!(
            compute_sliding_indicator_rect(None, Some(IndicatorItemRect { left: 1.0, top: 2.0, width: 3.0, height: 4.0 })),
            zeroed
        );
        // Non-positive item size.
        for (w, h) in [(0.0, 32.0), (72.0, 0.0), (-1.0, 32.0), (72.0, -5.0)] {
            assert_eq!(
                compute_sliding_indicator_rect(
                    Some(container),
                    Some(IndicatorItemRect { left: 10.0, top: 10.0, width: w, height: h })
                ),
                zeroed,
                "item {w}x{h}"
            );
        }
    }

    #[test]
    fn sliding_indicator_scroll_defaults_and_two_decimal_rounding() {
        // Missing scroll offsets default to 0.
        let rect = compute_sliding_indicator_rect(
            Some(IndicatorContainerRect { left: 10.0, top: 20.0, scroll_left: None, scroll_top: None }),
            Some(IndicatorItemRect { left: 30.5, top: 25.25, width: 72.0, height: 32.0 }),
        );
        assert_eq!(rect, SlidingIndicatorRect { x: 20.5, y: 5.25, width: 72.0, height: 32.0, ready: true });
        // scrollTop participates, and values round to 2 decimals like toFixed(2).
        let rect = compute_sliding_indicator_rect(
            Some(IndicatorContainerRect { left: 0.0, top: 40.0, scroll_left: Some(0.0), scroll_top: Some(12.5) }),
            Some(IndicatorItemRect { left: 10.126, top: 44.0, width: 72.456, height: 31.999 }),
        );
        assert_eq!(rect, SlidingIndicatorRect { x: 10.13, y: 16.5, width: 72.46, height: 32.0, ready: true });
    }

    // Mirrors: "shouldShowScrollTopFade keeps top messages crisp at scrollTop=0"
    #[test]
    fn scroll_top_fade_keeps_top_messages_crisp() {
        assert!(!should_show_scroll_top_fade(0.0));
        assert!(!should_show_scroll_top_fade(8.0));
        assert!(should_show_scroll_top_fade(24.0));
        assert!(!should_show_scroll_top_fade(f64::NAN));
    }

    #[test]
    fn scroll_top_fade_threshold_boundary_and_custom_threshold() {
        // Strictly greater than 12px: exactly at the threshold stays crisp.
        assert!(!should_show_scroll_top_fade(12.0));
        assert!(should_show_scroll_top_fade(12.0001));
        assert!(!should_show_scroll_top_fade(-30.0));
        assert!(!should_show_scroll_top_fade(f64::INFINITY));
        assert!(!should_show_scroll_top_fade(f64::NEG_INFINITY));
        // Explicit threshold variant.
        assert!(!should_show_scroll_top_fade_with_threshold(15.0, 20.0));
        assert!(should_show_scroll_top_fade_with_threshold(25.0, 20.0));
        assert!(!should_show_scroll_top_fade_with_threshold(f64::NAN, 20.0));
    }
}
