//! Responsive layout math — port of `src/utils/responsiveLayout.ts` (spec P1-05).
//! See docs/RUNE_DESKTOP_SPEC.md §5.1 and §6.3. Owned by the layout-motion workstream.
//!
//! Pure functions, no framework imports. Port notes:
//! - TS `number | null | undefined` becomes `Option<f64>`; non-finite widths are
//!   rejected exactly like the TS `Number.isFinite` guard.
//! - TS string-literal unions become local enums.

/// Minimum center-stage width (px) needed to dock the 312–328px Thread Tasks
/// sidebar side-by-side without squishing the transcript column below ~560px.
pub const THREAD_SPLIT_MIN_STAGE_WIDTH: f64 = 900.0;

/// Viewport height (px) at/above which Queue and CLI keep a compact portrait header.
const QUEUE_CLI_COMPACT_MIN_HEIGHT: f64 = 860.0;

/// Viewport height (px) at/above which the Thread (voice) view keeps a compact
/// portrait header; shorter windows get the slim strip.
const VOICE_COMPACT_MIN_HEIGHT: f64 = 700.0;

/// Dock mode of the Thread Tasks panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadTasksDockMode {
    /// Side-by-side right drawer (stage width >= 900px on desktop-lg).
    Split,
    /// Overlay drawer above the transcript (narrow stages / mobile / tablet).
    Overlay,
}

/// Horizontal alignment of a Dot's floating 308px popover on `sm+` spatial
/// canvases, so dots near the left/right edge never clip outside the container.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasPopoverAlign {
    Left,
    Center,
    Right,
}

/// Height tier of the desktop top-center portrait header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageHeaderSize {
    Full,
    Compact,
    Strip,
}

/// Panel view whose stage header height is being resolved.
/// `Voice` is the Thread view (`VoiceWorkspacePane` in the web app).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelView {
    Home,
    Canvas,
    Voice,
    Queue,
    Cli,
}

/// Resolves whether the Thread Tasks panel docks `split` (right drawer) or
/// floats as an `overlay`. Split docking requires desktop-lg AND a finite,
/// positive stage width of at least [`THREAD_SPLIT_MIN_STAGE_WIDTH`] px.
///
/// Mirrors `resolveThreadTasksLayoutMode(stageWidthPx, isDesktopLg)`.
pub fn resolve_thread_tasks_layout_mode(
    stage_width_px: Option<f64>,
    is_desktop_lg: bool,
) -> ThreadTasksDockMode {
    if !is_desktop_lg {
        return ThreadTasksDockMode::Overlay;
    }
    let width = match stage_width_px {
        Some(w) if w.is_finite() && w > 0.0 => w,
        _ => return ThreadTasksDockMode::Overlay,
    };
    if width >= THREAD_SPLIT_MIN_STAGE_WIDTH {
        ThreadTasksDockMode::Split
    } else {
        ThreadTasksDockMode::Overlay
    }
}

/// Computes horizontal alignment for a Dot's floating popover from its canvas
/// x position in percent: `left` when x < 24%, `right` when x > 76%, else
/// `center`. Non-finite positions fall back to `center`.
///
/// Mirrors `computeCanvasPopoverAlignment(xPct)`.
pub fn compute_canvas_popover_alignment(x_pct: f64) -> CanvasPopoverAlign {
    if !x_pct.is_finite() {
        return CanvasPopoverAlign::Center;
    }
    if x_pct < 24.0 {
        CanvasPopoverAlign::Left
    } else if x_pct > 76.0 {
        CanvasPopoverAlign::Right
    } else {
        CanvasPopoverAlign::Center
    }
}

/// Chooses how tall the desktop top-center portrait header should be so
/// Thread, Queue, and CLI views keep generous vertical scroll space on
/// shorter screens (e.g. 1024x768 or 1280x720) without sacrificing character
/// presence on taller monitors.
///
/// - `Home` always gets `full`; `Canvas` always gets `strip`.
/// - `Queue`/`Cli`: `compact` at viewport height >= 860px, else `strip`.
/// - `Voice` (Thread): `compact` at >= 700px, else `strip`.
///
/// Mirrors `resolveStageHeaderSize({ panelView, viewportHeightPx })`.
pub fn resolve_stage_header_size(panel_view: PanelView, viewport_height_px: f64) -> StageHeaderSize {
    match panel_view {
        PanelView::Home => StageHeaderSize::Full,
        PanelView::Canvas => StageHeaderSize::Strip,
        PanelView::Queue | PanelView::Cli => {
            if viewport_height_px >= QUEUE_CLI_COMPACT_MIN_HEIGHT {
                StageHeaderSize::Compact
            } else {
                StageHeaderSize::Strip
            }
        }
        // Thread view: keep compact portrait on >=700px height, slim strip on very short windows.
        PanelView::Voice => {
            if viewport_height_px >= VOICE_COMPACT_MIN_HEIGHT {
                StageHeaderSize::Compact
            } else {
                StageHeaderSize::Strip
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirrors responsiveLayout.test.ts:
    // "resolveThreadTasksLayoutMode uses overlay on mobile/tablet and narrow desktop stages"
    #[test]
    fn thread_tasks_overlay_on_mobile_tablet_and_narrow_stages() {
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(1200.0), false),
            ThreadTasksDockMode::Overlay
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(688.0), true),
            ThreadTasksDockMode::Overlay
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(THREAD_SPLIT_MIN_STAGE_WIDTH - 1.0), true),
            ThreadTasksDockMode::Overlay
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(0.0), true),
            ThreadTasksDockMode::Overlay
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(None, true),
            ThreadTasksDockMode::Overlay
        );
    }

    // Mirrors: "resolveThreadTasksLayoutMode uses split docking only when desktop stage has >= 900px width"
    #[test]
    fn thread_tasks_split_only_at_900px_and_above() {
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(THREAD_SPLIT_MIN_STAGE_WIDTH), true),
            ThreadTasksDockMode::Split
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(940.0), true),
            ThreadTasksDockMode::Split
        );
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(1100.0), true),
            ThreadTasksDockMode::Split
        );
    }

    // TS Number.isFinite guard: null/undefined covered above; negative, NaN and
    // infinite widths must also stay overlay.
    #[test]
    fn thread_tasks_rejects_nonfinite_and_negative_widths() {
        for width in [-50.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                resolve_thread_tasks_layout_mode(Some(width), true),
                ThreadTasksDockMode::Overlay,
                "width {width}"
            );
        }
        // Even a huge width stays overlay off desktop-lg.
        assert_eq!(
            resolve_thread_tasks_layout_mode(Some(4000.0), false),
            ThreadTasksDockMode::Overlay
        );
    }

    // Mirrors: "computeCanvasPopoverAlignment prevents edge clipping for left and right dots"
    #[test]
    fn canvas_popover_alignment_prevents_edge_clipping() {
        assert_eq!(compute_canvas_popover_alignment(10.0), CanvasPopoverAlign::Left);
        assert_eq!(compute_canvas_popover_alignment(23.9), CanvasPopoverAlign::Left);
        assert_eq!(compute_canvas_popover_alignment(24.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(50.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(76.0), CanvasPopoverAlign::Center);
        assert_eq!(compute_canvas_popover_alignment(76.1), CanvasPopoverAlign::Right);
        assert_eq!(compute_canvas_popover_alignment(90.0), CanvasPopoverAlign::Right);
        assert_eq!(compute_canvas_popover_alignment(f64::NAN), CanvasPopoverAlign::Center);
    }

    #[test]
    fn canvas_popover_alignment_nonfinite_falls_back_to_center() {
        assert_eq!(
            compute_canvas_popover_alignment(f64::INFINITY),
            CanvasPopoverAlign::Center
        );
        assert_eq!(
            compute_canvas_popover_alignment(f64::NEG_INFINITY),
            CanvasPopoverAlign::Center
        );
        // Out-of-range but finite percentages still clamp to the edge alignments.
        assert_eq!(compute_canvas_popover_alignment(-5.0), CanvasPopoverAlign::Left);
        assert_eq!(compute_canvas_popover_alignment(105.0), CanvasPopoverAlign::Right);
    }

    // Mirrors: "resolveStageHeaderSize adapts Queue/CLI/Thread chrome to viewport height"
    #[test]
    fn stage_header_size_adapts_to_viewport_height() {
        assert_eq!(resolve_stage_header_size(PanelView::Home, 700.0), StageHeaderSize::Full);
        assert_eq!(resolve_stage_header_size(PanelView::Canvas, 900.0), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Queue, 768.0), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Queue, 900.0), StageHeaderSize::Compact);
        assert_eq!(resolve_stage_header_size(PanelView::Cli, 768.0), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Cli, 900.0), StageHeaderSize::Compact);
        assert_eq!(resolve_stage_header_size(PanelView::Voice, 768.0), StageHeaderSize::Compact);
        assert_eq!(resolve_stage_header_size(PanelView::Voice, 640.0), StageHeaderSize::Strip);
    }

    #[test]
    fn stage_header_size_exact_threshold_boundaries() {
        // Queue/CLI flip at exactly 860px.
        assert_eq!(resolve_stage_header_size(PanelView::Queue, 859.9), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Queue, 860.0), StageHeaderSize::Compact);
        assert_eq!(resolve_stage_header_size(PanelView::Cli, 860.0), StageHeaderSize::Compact);
        // Thread (voice) flips at exactly 700px.
        assert_eq!(resolve_stage_header_size(PanelView::Voice, 699.9), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Voice, 700.0), StageHeaderSize::Compact);
        // Home/Canvas ignore height entirely.
        assert_eq!(resolve_stage_header_size(PanelView::Home, 400.0), StageHeaderSize::Full);
        assert_eq!(resolve_stage_header_size(PanelView::Canvas, 2000.0), StageHeaderSize::Strip);
        // Non-finite height behaves like TS `NaN >= x === false` -> strip.
        assert_eq!(resolve_stage_header_size(PanelView::Queue, f64::NAN), StageHeaderSize::Strip);
        assert_eq!(resolve_stage_header_size(PanelView::Voice, f64::NAN), StageHeaderSize::Strip);
    }
}
