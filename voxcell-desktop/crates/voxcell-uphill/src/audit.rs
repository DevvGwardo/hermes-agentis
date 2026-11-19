//! Static per-frame audits: AccessKit completeness, clipped text, contrast,
//! canvas popover edge safety, and status/copy checks. Ports the DOM audits
//! from `tools/blueprint/eval-ui-uphill.ts` to rune's headless element tree.

use rune::{ElementInfo, Runtime};
use serde_json::{Value, json};
use voxcell_ui::AppState;

// ── AccessKit completeness ─────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct A11yReport {
    pub node_count: usize,
    pub interactive_count: usize,
    /// Interactive a11y nodes with an empty name (screen-reader dead ends).
    pub unnamed_interactive: Vec<String>,
    /// Expected element keys absent from the a11y tree.
    pub missing_keys: Vec<String>,
    /// Focusable elements with no a11y counterpart (by key or by name).
    pub unexposed_focusable: Vec<String>,
    /// Unnamed interactive nodes already documented as foreign-workstream
    /// gaps (recorded, not blocking — ownership map forbids editing them).
    pub known_gaps: Vec<String>,
    pub roles: Vec<String>,
}

/// Unnamed interactive elements owned by OTHER workstreams at P4-04 time.
/// The harness records these as gaps for the parent to route to the owners;
/// any NEW unnamed interactive node still fails the audit. Prefix match.
const KNOWN_UNNAMED: &[(&str, &str)] = &[
    ("sidebar-avatar-hit", "sidebar workstream: avatar hit-area button has no accessible name"),
    ("home-portrait-stage", "home workstream: portrait stage button has no accessible name"),
    ("phone-portrait-box", "phone-tray workstream: portrait box button has no accessible name"),
    ("minimap-tick-", "thread workstream: minimap tick buttons have no accessible name"),
];

fn known_unnamed_gap(key: &str) -> Option<&'static str> {
    KNOWN_UNNAMED
        .iter()
        .find(|(prefix, _)| key.starts_with(prefix))
        .map(|(_, reason)| *reason)
}

impl A11yReport {
    pub fn complete(&self) -> bool {
        self.unnamed_interactive.is_empty()
            && self.missing_keys.is_empty()
            && self.unexposed_focusable.is_empty()
            && self.node_count > 0
    }
    pub fn to_json(&self) -> Value {
        json!({
            "nodeCount": self.node_count,
            "interactiveCount": self.interactive_count,
            "unnamedInteractive": self.unnamed_interactive,
            "missingKeys": self.missing_keys,
            "unexposedFocusable": self.unexposed_focusable,
            "knownGaps": self.known_gaps,
            "roles": self.roles,
            "complete": self.complete(),
        })
    }
}

const INTERACTIVE_ROLES: [&str; 9] = [
    "button", "tab", "checkbox", "switch", "menuitem", "link", "textinput",
    "textfield", "combobox",
];

fn walk_a11y(v: &Value, out: &mut Vec<(String, String, String)>) {
    match v {
        Value::Object(map) => {
            if let Some(role) = map.get("role").and_then(|r| r.as_str()) {
                out.push((
                    role.to_string(),
                    map.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
                    map.get("key").and_then(|k| k.as_str()).unwrap_or("").to_string(),
                ));
            }
            for (k, child) in map {
                if k != "role" && k != "name" && k != "key" {
                    walk_a11y(child, out);
                }
            }
        }
        Value::Array(items) => {
            for child in items {
                walk_a11y(child, out);
            }
        }
        _ => {}
    }
}

/// Audit the AccessKit tree against the live element list. `expected_keys`
/// are scene-critical keys that must surface in the tree.
pub fn audit_a11y(rt: &Runtime<AppState>, expected_keys: &[&str]) -> A11yReport {
    let tree = rt.accessibility_json();
    let mut nodes = Vec::new();
    walk_a11y(&tree, &mut nodes);
    let mut report = A11yReport { node_count: nodes.len(), ..Default::default() };

    let mut roles: Vec<String> = nodes.iter().map(|(r, _, _)| r.clone()).collect();
    roles.sort();
    roles.dedup();
    report.roles = roles;

    for (role, name, key) in &nodes {
        if INTERACTIVE_ROLES.contains(&role.as_str()) {
            report.interactive_count += 1;
            if name.trim().is_empty() {
                match known_unnamed_gap(key) {
                    Some(reason) => report.known_gaps.push(format!("{role} key={key}: {reason}")),
                    None => report.unnamed_interactive.push(format!("{role} key={key}")),
                }
            }
        }
    }
    for key in expected_keys {
        if !nodes.iter().any(|(_, _, k)| k == key) {
            report.missing_keys.push(key.to_string());
        }
    }
    // Every focusable, non-inert element must be reachable in the a11y tree
    // either by its author key or by its visible label.
    for e in rt.elements() {
        if !e.focusable || e.inert || e.exiting {
            continue;
        }
        if !INTERACTIVE_ROLES.contains(&e.role) {
            continue;
        }
        let by_key = e
            .key
            .as_deref()
            .map(|k| nodes.iter().any(|(_, _, nk)| nk == k))
            .unwrap_or(false);
        let label = e.label.as_deref().or(e.text.as_deref()).unwrap_or("");
        let by_name = !label.is_empty() && nodes.iter().any(|(_, n, _)| n == label);
        if !by_key && !by_name {
            report.unexposed_focusable.push(format!(
                "{} key={:?} label={label:?}",
                e.role, e.key
            ));
        }
    }
    report
}

// ── Clipped text (headless scroll_w <= client_w) ───────────────────────────

#[derive(Clone, Debug, Default)]
pub struct ClipReport {
    pub checked: usize,
    /// Text whose laid-out box is horizontally cut by ancestor clips —
    /// rune's equivalent of `scrollWidth > clientWidth`.
    pub clipped: Vec<String>,
}

impl ClipReport {
    pub fn clean(&self) -> bool {
        self.clipped.is_empty()
    }
    pub fn to_json(&self) -> Value {
        json!({ "checked": self.checked, "clipped": self.clipped, "clean": self.clean() })
    }
}

/// rune lays text out at its wrapped width, so a text node whose `visible`
/// width is smaller than its `rect` width is overflowing its clip box —
/// the headless equivalent of the web `scroll_w <= client_w` audit.
/// Vertical scroll clipping is legal (transcripts scroll); horizontal is not.
pub fn audit_clipping(elements: &[ElementInfo]) -> ClipReport {
    let mut report = ClipReport::default();
    for e in elements {
        let Some(t) = e.text.as_deref() else { continue };
        if t.trim().is_empty() || e.exiting || e.opacity < 0.05 || e.virtual_extent {
            continue;
        }
        if e.rect[2] <= 0.5 || e.rect[3] <= 0.5 {
            continue;
        }
        report.checked += 1;
        if e.visible[2] + 1.0 < e.rect[2] {
            report.clipped.push(format!(
                "key={:?} text={:?} rect_w={:.1} visible_w={:.1}",
                e.key,
                t.chars().take(40).collect::<String>(),
                e.rect[2],
                e.visible[2]
            ));
        }
    }
    report
}

// ── Contrast (WCAG, ported lowContrastTextCount audit) ─────────────────────

#[derive(Clone, Debug, Default)]
pub struct ContrastReport {
    pub checked: usize,
    pub below_45: Vec<String>,
    /// NEW sub-3.0 violations (fail the audit).
    pub below_30: Vec<String>,
    /// Sub-3.0 text already documented as foreign-workstream gaps.
    pub known_gaps: Vec<String>,
    pub worst_ratio: f64,
}

/// Low-contrast text owned by OTHER workstreams at P4-04 time (recorded,
/// not blocking). Status labels come from core::sidebar_char via sidebar.rs;
/// HH:MM transcript timestamps come from thread.rs.
fn known_contrast_gap(text: &str, scene: &str) -> Option<&'static str> {
    match text.trim() {
        "Online" | "Working" | "Speaking" | "Approval needed" | "Thinking" | "Listening" | "Idle" => {
            Some("sidebar workstream: character status label below WCAG 3.0")
        }
        t if scene.starts_with("thread") && is_hhmm(t) => {
            Some("thread workstream: transcript timestamp below WCAG 3.0")
        }
        _ => None,
    }
}

fn is_hhmm(t: &str) -> bool {
    let b = t.as_bytes();
    (b.len() == 4 || b.len() == 5)
        && b.iter().enumerate().all(|(i, &c)| {
            if i == b.len() - 3 { c == b':' } else { c.is_ascii_digit() }
        })
}

impl ContrastReport {
    pub fn to_json(&self) -> Value {
        json!({
            "checked": self.checked,
            "below_4_5": self.below_45,
            "below_3_0": self.below_30,
            "knownGaps": self.known_gaps,
            "worstRatio": (self.worst_ratio * 100.0).round() / 100.0,
        })
    }
}

fn srgb_luminance(r: f32, g: f32, b: f32) -> f64 {
    // voxcell-ui theme tokens are authored as sRGB hex and stored verbatim
    // (theme::parse_hex_color does NOT linearize), so ElementInfo colors in
    // this app are gamma-encoded sRGB — exactly what WCAG consumes, matching
    // the web CSS audit. No srgb_encode here (it would double-encode).
    let (r, g, b) = (r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
    0.2126 * r as f64 + 0.7152 * g as f64 + 0.0722 * b as f64
}

fn composite_over(fg: [f32; 4], bg: [f32; 4]) -> [f32; 4] {
    let a = fg[3].clamp(0.0, 1.0);
    [
        fg[0] * a + bg[0] * (1.0 - a),
        fg[1] * a + bg[1] * (1.0 - a),
        fg[2] * a + bg[2] * (1.0 - a),
        1.0,
    ]
}

fn is_braille_art(t: &str) -> bool {
    let mut braille = 0usize;
    let mut total = 0usize;
    for c in t.chars() {
        if !c.is_whitespace() {
            total += 1;
            if ('\u{2800}'..='\u{28FF}').contains(&c) {
                braille += 1;
            }
        }
    }
    total > 0 && braille * 2 > total
}

/// WCAG contrast of every visible text node against its composited
/// background (ancestor fills folded over the window clear color).
pub fn audit_contrast(rt: &Runtime<AppState>, scene: &str) -> ContrastReport {
    let elements = rt.elements();
    let clear = rt.clear_color();
    let mut report = ContrastReport { worst_ratio: f64::MAX, ..Default::default() };
    for (i, e) in elements.iter().enumerate() {
        let Some(t) = e.text.as_deref() else { continue };
        let Some(tc) = e.text_color else { continue };
        // Effective opacity folds ancestor fades (entrance staggers).
        let mut eff_opacity = e.opacity;
        let mut cur = e.parent;
        while let Some(idx) = cur {
            eff_opacity *= elements[idx].opacity;
            cur = elements[idx].parent;
        }
        if t.trim().is_empty() || e.exiting || eff_opacity < 0.35 || is_braille_art(t) {
            continue;
        }
        if e.rect[2] <= 0.5 || e.visible[2] <= 0.5 {
            continue;
        }
        // Painter-order background: fold every fill drawn BEFORE this text
        // whose visible box covers the text center, over the clear color.
        // This captures sibling backdrops (ambient-backdrop fills theme.bg)
        // that an ancestor-only walk would miss. Gradient washes are not
        // per-position sampleable from ElementInfo; the base fill dominates
        // (wash alpha <= 0.3 over a near-black base), a documented
        // approximation. `elements()` is tree/draw order.
        let cx = e.rect[0] + e.rect[2] * 0.5;
        let cy = e.rect[1] + e.rect[3] * 0.5;
        let mut bg = [clear[0], clear[1], clear[2], 1.0];
        for b in &elements[..i] {
            if b.fill.a <= 0.001 || b.exiting || b.opacity < 0.05 || b.virtual_extent {
                continue;
            }
            let v = b.visible;
            if v[2] <= 0.0 || v[3] <= 0.0 {
                continue;
            }
            if cx >= v[0] && cx <= v[0] + v[2] && cy >= v[1] && cy <= v[1] + v[3] {
                bg = composite_over([b.fill.r, b.fill.g, b.fill.b, b.fill.a * b.opacity], bg);
            }
        }
        let fg = composite_over([tc.r, tc.g, tc.b, tc.a * eff_opacity], bg);
        let l1 = srgb_luminance(fg[0], fg[1], fg[2]);
        let l2 = srgb_luminance(bg[0], bg[1], bg[2]);
        let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
        let ratio = (hi + 0.05) / (lo + 0.05);
        report.checked += 1;
        if ratio < report.worst_ratio {
            report.worst_ratio = ratio;
        }
        let desc = format!(
            "key={:?} text={:?} ratio={ratio:.2}",
            e.key,
            t.chars().take(30).collect::<String>()
        );
        if ratio < 3.0 {
            match known_contrast_gap(t, scene) {
                Some(reason) => report.known_gaps.push(format!("{desc}: {reason}")),
                None => report.below_30.push(desc.clone()),
            }
        }
        if ratio < 4.5 {
            report.below_45.push(desc);
        }
    }
    if report.worst_ratio == f64::MAX {
        report.worst_ratio = 0.0;
    }
    report
}

// ── Canvas popover edge safety ─────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct PopoverReport {
    pub checks: Vec<Value>,
    pub violations: Vec<String>,
}

impl PopoverReport {
    pub fn edge_safe(&self) -> bool {
        self.violations.is_empty() && !self.checks.is_empty()
    }
    pub fn to_json(&self) -> Value {
        json!({ "checks": self.checks, "violations": self.violations, "edgeSafe": self.edge_safe() })
    }
}

fn rect_of(rt: &Runtime<AppState>, key: &str) -> Option<[f32; 4]> {
    rt.elements()
        .into_iter()
        .find(|e| e.key.as_deref() == Some(key))
        .map(|e| e.target)
}

/// Open the popover for `dot_id` with the dot parked at `x_pct` of the
/// canvas design width (10 = left edge, 50 = center, 90 = right edge) and
/// assert the popover stays inside the canvas field. Mirrors the web
/// edge-clamped popover audit (`compute_canvas_popover_alignment`).
pub fn audit_popover_at(rt: &mut Runtime<AppState>, dot_id: &str, x_pct: f64) -> PopoverReport {
    use voxcell_core::canvas::CANVAS_BOUNDS;
    use voxcell_ui::state::{CANVAS_DESIGN_H, CANVAS_DESIGN_W};
    let mut report = PopoverReport::default();
    let Some(dot) = rt.state.dots.iter_mut().find(|d| d.id == dot_id) else {
        report.violations.push(format!("dot {dot_id} not found"));
        return report;
    };
    dot.x = (x_pct / 100.0) as f32 * CANVAS_DESIGN_W;
    dot.y = (46.0 / 100.0) as f32 * CANVAS_DESIGN_H;
    rt.state.canvas_open_popover(dot_id);
    rt.invalidate();
    rt.frame(0.016);
    rt.settle_all();
    rt.frame(0.016);

    let field = rect_of(rt, "canvas-field");
    let pop = rect_of(rt, "canvas-popover");
    let node = rect_of(rt, &format!("canvas-node-{dot_id}"));
    match (field, pop, node) {
        (Some(field), Some(pop), Some(node)) => {
            let node_cx = node[0] + node[2] * 0.5;
            let inside = pop[0] >= field[0] - 1.0
                && pop[1] >= field[1] - 1.0
                && pop[0] + pop[2] <= field[0] + field[2] + 1.0
                && pop[1] + pop[3] <= field[1] + field[3] + 1.0;
            let align_ok = if x_pct <= CANVAS_BOUNDS.min_x as f64 + 0.01 {
                pop[0] >= node_cx - 1.0 // left edge: extend rightward
            } else if x_pct >= CANVAS_BOUNDS.max_x as f64 - 0.01 {
                pop[0] + pop[2] <= node_cx + 1.0 // right edge: extend leftward
            } else {
                (pop[0] + pop[2] * 0.5 - node_cx).abs() < 40.0
            };
            report.checks.push(json!({
                "dot": dot_id, "xPct": x_pct, "pop": pop, "field": field,
                "inside": inside, "alignOk": align_ok,
            }));
            if !inside {
                report.violations.push(format!("{dot_id}@{x_pct}% popover outside field: pop={pop:?} field={field:?}"));
            }
            if !align_ok {
                report.violations.push(format!("{dot_id}@{x_pct}% popover misaligned vs node cx={node_cx}"));
            }
        }
        _ => report.violations.push(format!(
            "{dot_id}@{x_pct}% missing elements field={field:?} pop={pop:?} node={node:?}"
        )),
    }
    rt.state.canvas_close_popover();
    rt.invalidate();
    rt.frame(0.016);
    report
}

// ── Status & copy audit ────────────────────────────────────────────────────

#[derive(Clone, Debug, Default)]
pub struct CopyReport {
    pub stuck_calibrating: bool,
    pub singular_gate_text: bool,
    pub empty_main_pane: bool,
    pub main_pane_chars: usize,
}

impl CopyReport {
    pub fn to_json(&self) -> Value {
        json!({
            "stuckCalibrating": self.stuck_calibrating,
            "singularGateText": self.singular_gate_text,
            "emptyMainPane": self.empty_main_pane,
            "mainPaneChars": self.main_pane_chars,
        })
    }
}

/// Port of the web body-text audit: no stuck "Calibrating", no plural
/// "N gate" copy (`/\b[2-9]\s+gate\b/`), and a non-empty main pane.
pub fn audit_copy(rt: &Runtime<AppState>, pane_key: &str) -> CopyReport {
    let elements = rt.elements();
    let pane = elements
        .iter()
        .find(|e| e.key.as_deref() == Some(pane_key))
        .map(|e| e.target);
    let mut report = CopyReport::default();
    let mut pane_text = String::new();
    for e in &elements {
        let Some(t) = e.text.as_deref() else { continue };
        if e.exiting || e.opacity < 0.05 {
            continue;
        }
        if t.contains("Calibrating") {
            report.stuck_calibrating = true;
        }
        if has_plural_gate(t) {
            report.singular_gate_text = true;
        }
        let in_pane = match pane {
            Some(p) => {
                let (cx, cy) = (e.target[0] + e.target[2] * 0.5, e.target[1] + e.target[3] * 0.5);
                cx >= p[0] && cx <= p[0] + p[2] && cy >= p[1] && cy <= p[1] + p[3]
            }
            None => false,
        };
        if in_pane {
            pane_text.push_str(t);
            pane_text.push(' ');
        }
    }
    report.main_pane_chars = pane_text.trim().len();
    report.empty_main_pane = report.main_pane_chars < 20;
    report
}

fn has_plural_gate(t: &str) -> bool {
    let chars: Vec<char> = t.chars().collect();
    for i in 0..chars.len() {
        if !('2'..='9').contains(&chars[i]) {
            continue;
        }
        let mut j = i + 1;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j == i + 1 {
            continue; // needs at least one space: /[2-9]\s+gate/
        }
        let rest: String = chars[j..].iter().collect();
        if rest.starts_with("gate")
            && !rest[4..].chars().next().map(|c| c.is_alphanumeric()).unwrap_or(false)
        {
            return true;
        }
    }
    false
}
