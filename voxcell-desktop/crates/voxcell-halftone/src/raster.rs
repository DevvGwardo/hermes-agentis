//! Braille halftone raster — port of `src/utils/brailleCanvasRaster.ts` plus the
//! ordered-Bayer subpixel packing from `asciiPortraitEngine.ts::renderHalftonePortrait`.
//!
//! Pure data / CPU rasterization. The GPU instance-buffer emitter lives in `gpu.rs`
//! and consumes the same `BrailleFrameGrid` + `TonalTier` palette produced here.

use crate::portrait::{PortraitMotionState, portrait_assets, sample_portrait_luminance_field};
use voxcell_core::{DotProfile, ExpressionMood, MouthViseme};

/// Raw 8x8 Bayer rank matrix (values 0..63), row-major.
pub const BAYER_8X8_RANKS: [u8; 64] = [
    0, 32, 8, 40, 2, 34, 10, 42,
    48, 16, 56, 24, 50, 18, 58, 26,
    12, 44, 4, 36, 14, 46, 6, 38,
    60, 28, 52, 20, 62, 30, 54, 22,
    3, 35, 11, 43, 1, 33, 9, 41,
    51, 19, 59, 27, 49, 17, 57, 25,
    15, 47, 7, 39, 13, 45, 5, 37,
    63, 31, 55, 23, 61, 29, 53, 21,
];

/// Normalized Bayer ranks in [0,1) used for horizon/shape threshold jitter.
pub const BAYER_8X8: [f32; 64] = {
    let mut out = [0.0_f32; 64];
    let mut i = 0;
    while i < 64 {
        out[i] = BAYER_8X8_RANKS[i] as f32 / 64.0;
        i += 1;
    }
    out
};

/// Deterministic ordered 8x8 Bayer halftone thresholds in [0.065, 0.935].
/// Prevents serpentine error-diffusion dot-boiling/liquid shimmer during subtle motion
/// while keeping deep anime ink (<0.065) cleanly empty and specular highlights (>=0.94) solid.
pub const BAYER_THRESH_8X8: [f32; 64] = {
    let mut out = [0.0_f32; 64];
    let mut i = 0;
    while i < 64 {
        out[i] = 0.065 + ((BAYER_8X8_RANKS[i] as f32 + 0.5) / 64.0) * 0.87;
        i += 1;
    }
    out
};

pub fn parse_hex_rgb(hex: &str) -> [u8; 3] {
    let clean = hex.strip_prefix('#').unwrap_or(hex);
    if clean.len() == 3 {
        let mut out = [0u8; 3];
        for (i, ch) in clean.chars().take(3).enumerate() {
            let doubled = format!("{ch}{ch}");
            out[i] = u8::from_str_radix(&doubled, 16).unwrap_or(0);
        }
        return out;
    }
    if clean.len() == 6 {
        if let Ok(v) = u32::from_str_radix(clean, 16) {
            return [((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8];
        }
    }
    [230, 240, 236]
}

pub fn mix_rgb(a: [u8; 3], b: [u8; 3], t: f32) -> [u8; 3] {
    let w = t.clamp(0.0, 1.0);
    [
        (a[0] as f32 + (b[0] as f32 - a[0] as f32) * w).round() as u8,
        (a[1] as f32 + (b[1] as f32 - a[1] as f32) * w).round() as u8,
        (a[2] as f32 + (b[2] as f32 - a[2] as f32) * w).round() as u8,
    ]
}

#[derive(Clone, Copy, Debug)]
pub struct TonalTier {
    pub rgb: [u8; 3],
    pub alpha: f32,
    pub diameter_scale: f32,
    pub bloom_weight: f32,
}

/// State-reactive 5-tier halftone dot colors & opacities for the Braille portrait
/// (thinking amber phosphor, listening/speaking sky-silver glow, alert rose, idle slate).
pub fn resolve_portrait_tone_palette(
    mood: ExpressionMood,
    is_speaking: bool,
    is_listening: bool,
    is_loading: bool,
    is_thumb: bool,
    energy: f32,
    tint_hex: Option<&str>,
) -> [TonalTier; 5] {
    let speaking_hot = !is_thumb && is_speaking && energy > 0.22;
    let active_voice = !is_thumb && (is_loading || is_listening || speaking_hot);
    let is_thinking = !is_thumb && !active_voice && mood == ExpressionMood::Thinking;
    let is_alert = !is_thumb && !active_voice && mood == ExpressionMood::Alert;

    let base_hexes = if is_thinking {
        ["#B48448", "#D9B26F", "#EFE2C4", "#F8F3E8", "#FFFFFF"]
    } else if is_alert {
        ["#B0707C", "#D89AA6", "#EFD4D9", "#F9ECEF", "#FFFFFF"]
    } else if active_voice {
        ["#BAE6FD", "#E0F2FE", "#F1F5F9", "#F8FAFC", "#FFFFFF"]
    } else {
        ["#94A3B8", "#CBD5E1", "#E2E8F0", "#F1F5F9", "#FFFFFF"]
    };

    let tint = tint_hex.map(parse_hex_rgb);
    let tint_weights = [
        if active_voice { 0.85 } else { 0.52 },
        if active_voice { 0.72 } else { 0.44 },
        0.52,
        0.34,
        0.16,
    ];

    let boost = if active_voice { (energy * 0.05).min(0.06) } else { 0.0 };
    let alphas = if is_loading {
        [0.46, 0.62, 0.76, 0.88, 0.96]
    } else {
        [
            (0.56 + boost).min(0.99),
            (0.74 + boost).min(0.99),
            (0.87 + boost * 0.5).min(0.99),
            0.95,
            0.99,
        ]
    };
    // Per-tier dot diameter modulation from AsciiPortraitCanvas tonal buckets.
    let diameters = [0.76, 0.85, 0.94, 1.02, 1.08];
    // Bloom only engages on the top two tiers (rasterizer blooms buckets[len-2..]).
    let blooms = [0.0, 0.0, 0.0, 1.0, 1.0];

    std::array::from_fn(|i| {
        let base_rgb = parse_hex_rgb(base_hexes[i]);
        let rgb = match tint {
            Some(t_rgb) => mix_rgb(base_rgb, t_rgb, tint_weights[i]),
            None => base_rgb,
        };
        TonalTier { rgb, alpha: alphas[i], diameter_scale: diameters[i], bloom_weight: blooms[i] }
    })
}

pub fn resolve_shape_tone_palette(
    low_rgb: [u8; 3],
    mid_rgb: [u8; 3],
    high_rgb: [u8; 3],
    is_thinking: bool,
    is_speaking: bool,
    energy: f32,
) -> [TonalTier; 5] {
    let warm_mid = parse_hex_rgb("#FBBF24");
    let warm_high = parse_hex_rgb("#FEF3C7");
    let amber_low = parse_hex_rgb("#D97706");

    let c0 = if is_thinking { mix_rgb(low_rgb, amber_low, 0.22) } else { low_rgb };
    let c1 = if is_thinking {
        mix_rgb(mix_rgb(low_rgb, mid_rgb, 0.58), warm_mid, 0.20)
    } else {
        mix_rgb(low_rgb, mid_rgb, 0.58)
    };
    let c2 = if is_thinking { mix_rgb(mid_rgb, warm_mid, 0.28) } else { mid_rgb };
    let c3 = if is_thinking {
        mix_rgb(mix_rgb(mid_rgb, high_rgb, 0.55), warm_high, 0.32)
    } else {
        mix_rgb(mid_rgb, high_rgb, 0.55)
    };
    let c4 = if is_speaking {
        mix_rgb(high_rgb, [255, 255, 255], (0.18 + energy * 0.35).min(0.55))
    } else if is_thinking {
        mix_rgb(high_rgb, warm_high, 0.35)
    } else {
        high_rgb
    };

    let boost = if is_speaking { (energy * 0.04).min(0.05) } else { 0.0 };
    let alphas = [(0.74 + boost).min(0.99), (0.86 + boost).min(0.99), 0.93, 0.96, 0.99];
    let colors = [c0, c1, c2, c3, c4];
    let diameters = [0.68, 0.82, 0.95, 1.08, 1.18];
    let blooms = [0.0, 0.0, 0.0, 0.6, 1.0];

    std::array::from_fn(|i| TonalTier {
        rgb: colors[i],
        alpha: alphas[i],
        diameter_scale: diameters[i],
        bloom_weight: blooms[i],
    })
}

#[derive(Clone, Debug)]
pub struct BrailleFrameGrid {
    pub cols: usize,
    pub rows: usize,
    pub cell_bits: Vec<u8>,
    pub cell_lums: Vec<u8>,
}

impl BrailleFrameGrid {
    pub fn braille_char(&self, idx: usize) -> char {
        let bits = self.cell_bits.get(idx).copied().unwrap_or(0);
        if bits == 0 { ' ' } else { char::from_u32(0x2800 + bits as u32).unwrap_or(' ') }
    }

    pub fn lines(&self) -> Vec<String> {
        (0..self.rows)
            .map(|r| (0..self.cols).map(|c| self.braille_char(r * self.cols + c)).collect())
            .collect()
    }
}

/// Contour-preserving unsharp contrast boost + deterministic ordered 8x8 Bayer halftone
/// packing into Unicode Braille bits. Zero serpentine error-diffusion cascade -> zero
/// liquid dot-boiling across frames (matches `renderHalftonePortrait` steps 2-3).
pub fn pack_braille_subpixels_ordered_bayer(raw: &[f32], cols: usize, rows: usize) -> BrailleFrameGrid {
    let sub_w = cols * 2;
    let sub_h = rows * 4;
    let mut work = vec![0.0_f32; sub_w * sub_h];

    // 1. Unsharp contrast boost (clamped 4-neighbor average).
    for sy in 0..sub_h {
        let row_off = sy * sub_w;
        let up_off = sy.saturating_sub(1) * sub_w;
        let dn_off = (sy + 1).min(sub_h - 1) * sub_w;
        for sx in 0..sub_w {
            let idx = row_off + sx;
            let c = raw[idx];
            if c <= 0.03 {
                work[idx] = 0.0;
                continue;
            }
            if c >= 0.94 {
                work[idx] = 1.0;
                continue;
            }
            let l = raw[row_off + sx.saturating_sub(1)];
            let r = raw[row_off + (sx + 1).min(sub_w - 1)];
            let u = raw[up_off + sx];
            let d = raw[dn_off + sx];
            let avg = (l + r + u + d) * 0.25;
            let sharpened = c + (c - avg) * 0.48;
            let mut t = ((sharpened - 0.03) * 1.075_268_8).clamp(0.0, 1.0);
            t = t * 0.42 + t * t * (3.0 - 2.0 * t) * 0.58;
            work[idx] = t;
        }
    }

    // 2. Ordered 8x8 Bayer threshold packing into 2x4 Braille cells.
    let mut cell_bits = vec![0u8; cols * rows];
    let mut cell_lums = vec![0u8; cols * rows];
    for r in 0..rows {
        let sy_base = r * 4;
        let r0 = sy_base * sub_w;
        let r1 = r0 + sub_w;
        let r2 = r1 + sub_w;
        let r3 = r2 + sub_w;
        let b_row0 = (sy_base & 7) << 3;
        let b_row1 = ((sy_base + 1) & 7) << 3;
        let b_row2 = ((sy_base + 2) & 7) << 3;
        let b_row3 = ((sy_base + 3) & 7) << 3;
        let cell_row = r * cols;

        for c in 0..cols {
            let c2 = c * 2;
            let lum_sum = raw[r0 + c2]
                + raw[r0 + c2 + 1]
                + raw[r1 + c2]
                + raw[r1 + c2 + 1]
                + raw[r2 + c2]
                + raw[r2 + c2 + 1]
                + raw[r3 + c2]
                + raw[r3 + c2 + 1];
            if lum_sum <= 0.04 {
                continue;
            }
            let bx0 = c2 & 7;
            let bx1 = (c2 + 1) & 7;
            let mut bits = 0u8;
            if work[r0 + c2] >= BAYER_THRESH_8X8[b_row0 + bx0] { bits |= 0x01; }
            if work[r0 + c2 + 1] >= BAYER_THRESH_8X8[b_row0 + bx1] { bits |= 0x08; }
            if work[r1 + c2] >= BAYER_THRESH_8X8[b_row1 + bx0] { bits |= 0x02; }
            if work[r1 + c2 + 1] >= BAYER_THRESH_8X8[b_row1 + bx1] { bits |= 0x10; }
            if work[r2 + c2] >= BAYER_THRESH_8X8[b_row2 + bx0] { bits |= 0x04; }
            if work[r2 + c2 + 1] >= BAYER_THRESH_8X8[b_row2 + bx1] { bits |= 0x20; }
            if work[r3 + c2] >= BAYER_THRESH_8X8[b_row3 + bx0] { bits |= 0x40; }
            if work[r3 + c2 + 1] >= BAYER_THRESH_8X8[b_row3 + bx1] { bits |= 0x80; }

            cell_lums[cell_row + c] = (lum_sum * 31.875 + 0.5).clamp(0.0, 255.0) as u8;
            cell_bits[cell_row + c] = bits;
        }
    }

    BrailleFrameGrid { cols, rows, cell_bits, cell_lums }
}

/// Back-compat alias: the engine now uses ordered Bayer (not serpentine error diffusion).
pub fn diffuse_and_pack_braille_subpixels(raw: &[f32], cols: usize, rows: usize) -> BrailleFrameGrid {
    pack_braille_subpixels_ordered_bayer(raw, cols, rows)
}

/// Subpixel bit -> (px, py) within a 2x4 Braille cell (matches SUBPIXEL_BITS).
pub const SUBPIXEL_BITS: [(u8, usize, usize); 8] = [
    (0x01, 0, 0),
    (0x08, 1, 0),
    (0x02, 0, 1),
    (0x10, 1, 1),
    (0x04, 0, 2),
    (0x20, 1, 2),
    (0x40, 0, 3),
    (0x80, 1, 3),
];

/// Brightness thresholds (0..1) selecting tonal tier 4/3/2/1 from a cell's normalized
/// luminance; anything below the last threshold falls to tier 0. Portrait uses the
/// AsciiPortraitCanvas bucket cutoffs.
pub const PORTRAIT_TIER_THRESHOLDS: [f32; 4] = [0.24, 0.44, 0.64, 0.82];
pub const HORIZON_TIER_THRESHOLDS: [f32; 4] = [0.20, 0.38, 0.56, 0.74];

#[inline]
pub fn tier_from_bright(bright: f32, thresholds: &[f32; 4]) -> usize {
    if bright > thresholds[3] {
        4
    } else if bright > thresholds[2] {
        3
    } else if bright > thresholds[1] {
        2
    } else if bright > thresholds[0] {
        1
    } else {
        0
    }
}

/// Per-tier dot geometry shared by the CPU RGBA rasterizer and the GPU instance emitter.
#[derive(Clone, Copy, Debug)]
pub struct DotGeometry {
    pub cell_w: f32,
    pub cell_h: f32,
    pub dot_w: f32,
    pub dot_h: f32,
    /// Canvas dot radius used to inset subpixel top-left anchors.
    pub dot_radius: f32,
    /// Base dot diameter (px) fed to the tonal rasterizer before profile scaling.
    pub dot_diameter: f32,
}

impl DotGeometry {
    /// Portrait geometry from AsciiPortraitCanvas (hero default factor 0.42).
    pub fn portrait(pixel_w: f32, pixel_h: f32, cols: usize, rows: usize) -> Self {
        let cell_w = pixel_w / cols.max(1) as f32;
        let cell_h = pixel_h / rows.max(1) as f32;
        let dot_w = cell_w / 2.0;
        let dot_h = cell_h / 4.0;
        let dot_radius = dot_w.min(dot_h) * 0.42;
        let dot_diameter = (dot_radius * 1.96).max(1.15);
        Self { cell_w, cell_h, dot_w, dot_h, dot_radius, dot_diameter }
    }

    /// Horizon geometry from BrailleHorizonCanvas.
    pub fn horizon(pixel_w: f32, pixel_h: f32, cols: usize, rows: usize) -> Self {
        let cell_w = pixel_w / cols.max(1) as f32;
        let cell_h = pixel_h / rows.max(1) as f32;
        let dot_w = cell_w / 2.0;
        let dot_h = cell_h / 4.0;
        let dot_radius = (dot_w.min(dot_h) * 0.35).max(0.64);
        let dot_diameter = (dot_radius * 1.88).max(1.10);
        Self { cell_w, cell_h, dot_w, dot_h, dot_radius, dot_diameter }
    }
}

#[inline]
pub fn profile_scale(dot_profile: DotProfile) -> f32 {
    match dot_profile {
        DotProfile::Crisp => 0.76,
        DotProfile::Balanced => 0.84,
        DotProfile::Phosphor => 1.04,
    }
}

#[inline]
pub fn soft_edge_cut(dot_profile: DotProfile) -> f32 {
    match dot_profile {
        DotProfile::Crisp => 0.58,
        DotProfile::Balanced => 0.52,
        DotProfile::Phosphor => 0.32,
    }
}

/// A single emitted halftone dot (center + radius) with its tonal tier styling.
#[derive(Clone, Copy, Debug)]
pub struct TonalDot {
    pub cx: f32,
    pub cy: f32,
    pub radius: f32,
    pub tier: usize,
}

/// Converts a Braille grid into per-tier dot centers/radii using the shared canvas
/// geometry + tonal profile scaling. This is the single source of truth consumed by
/// both `rasterize_tonal_grid_rgba` (CPU) and `gpu::emit_dot_instances` (GPU).
pub fn grid_to_tonal_dots(
    grid: &BrailleFrameGrid,
    palette: &[TonalTier; 5],
    geom: &DotGeometry,
    dot_profile: DotProfile,
    thresholds: &[f32; 4],
) -> Vec<TonalDot> {
    let base_dot_px = (geom.dot_diameter * profile_scale(dot_profile)).max(0.72);
    let mut dots = Vec::new();
    for r in 0..grid.rows {
        let base_y = r as f32 * geom.cell_h;
        let row_off = r * grid.cols;
        for c in 0..grid.cols {
            let bits = grid.cell_bits[row_off + c];
            if bits == 0 {
                continue;
            }
            let bright = grid.cell_lums[row_off + c] as f32 / 255.0;
            let tier = tier_from_bright(bright, thresholds);
            let dot_px = (base_dot_px * palette[tier].diameter_scale).max(0.65);
            let radius = dot_px * 0.5;
            let base_x = c as f32 * geom.cell_w;
            for &(mask, px, py) in &SUBPIXEL_BITS {
                if bits & mask != 0 {
                    // Top-left anchor (TS bucket layout), then the rasterizer's centering
                    // shift collapses to + base_dot_px/2 for the final dot center.
                    let x0 = base_x + (px as f32 + 0.5) * geom.dot_w - geom.dot_radius;
                    let y0 = base_y + (py as f32 + 0.5) * geom.dot_h - geom.dot_radius;
                    dots.push(TonalDot { cx: x0 + base_dot_px * 0.5, cy: y0 + base_dot_px * 0.5, radius, tier });
                }
            }
        }
    }
    dots
}

/// Multi-tier halftone CPU rasterizer with per-tier dot-size modulation, squared-radial
/// circular AA (zero per-pixel sqrt), optional phosphor corona bloom on the top two tiers,
/// and source-over compositing so bright cores stay luminous over dim halos.
pub fn rasterize_tonal_grid_rgba(
    pixel_w: u32,
    pixel_h: u32,
    grid: &BrailleFrameGrid,
    palette: &[TonalTier; 5],
    dot_profile: DotProfile,
    bloom: f32,
) -> Vec<u8> {
    let w = pixel_w.max(1) as usize;
    let h = pixel_h.max(1) as usize;
    let mut bytes = vec![0u8; w * h * 4];
    if grid.cols == 0 || grid.rows == 0 {
        return bytes;
    }
    let geom = DotGeometry::portrait(w as f32, h as f32, grid.cols, grid.rows);
    let dots = grid_to_tonal_dots(grid, palette, &geom, dot_profile, &PORTRAIT_TIER_THRESHOLDS);
    rasterize_tonal_dots_rgba(&mut bytes, w, h, &dots, palette, dot_profile, bloom);
    bytes
}

/// Rasterizes pre-computed tonal dots into an RGBA buffer (shared CPU core).
pub fn rasterize_tonal_dots_rgba(
    bytes: &mut [u8],
    w: usize,
    h: usize,
    dots: &[TonalDot],
    palette: &[TonalTier; 5],
    dot_profile: DotProfile,
    bloom: f32,
) {
    let bloom = bloom.clamp(0.0, 1.5);
    let soft_cut = soft_edge_cut(dot_profile);
    let inv_edge_span = 1.0 / (1.3225 - soft_cut);

    // Pass 1: soft quadratic phosphor corona beneath top-tier highlight dots (tiers 3 & 4).
    if bloom > 0.04 {
        for tier_idx in 3..=4 {
            let tier = &palette[tier_idx];
            let tier_gain = if tier_idx == 4 { 1.0 } else { 0.40 };
            let halo_alpha_max = (tier.alpha * bloom * tier_gain * tier.bloom_weight * 44.0)
                .round()
                .min(76.0) as i32;
            if halo_alpha_max <= 2 {
                continue;
            }
            let [r, g, b] = tier.rgb;
            for dot in dots.iter().filter(|d| d.tier == tier_idx) {
                // Halo radius uses the tier's own dot diameter (radius*2).
                let dot_px = dot.radius * 2.0;
                let halo_radius = dot_px * (0.86 + bloom * 0.26);
                let inv_halo_r2 = 1.0 / (halo_radius * halo_radius);
                let cx = dot.cx;
                let cy = dot.cy;
                let ix0 = ((cx - halo_radius) as isize).max(0) as usize;
                let iy0 = ((cy - halo_radius) as isize).max(0) as usize;
                let ix1 = ((cx + halo_radius) as isize).min(w as isize - 1).max(0) as usize;
                let iy1 = ((cy + halo_radius) as isize).min(h as isize - 1).max(0) as usize;
                for py in iy0..=iy1 {
                    let dy = py as f32 + 0.5 - cy;
                    let dy2 = dy * dy;
                    let row_pix = py * w;
                    for px in ix0..=ix1 {
                        let dx = px as f32 + 0.5 - cx;
                        let norm2 = (dx * dx + dy2) * inv_halo_r2;
                        if norm2 >= 1.0 {
                            continue;
                        }
                        let falloff = 1.0 - norm2;
                        let cov_a = (halo_alpha_max as f32 * falloff * falloff + 0.5) as i32;
                        if cov_a <= 1 {
                            continue;
                        }
                        let off = (row_pix + px) * 4;
                        let prev_a = bytes[off + 3] as i32;
                        if prev_a == 0 {
                            bytes[off] = r;
                            bytes[off + 1] = g;
                            bytes[off + 2] = b;
                            bytes[off + 3] = cov_a.min(255) as u8;
                        } else {
                            let next_a = (prev_a + cov_a).min(210) as u8;
                            bytes[off] = (bytes[off] as i32 + ((r as i32 * cov_a) >> 8)).min(255) as u8;
                            bytes[off + 1] = (bytes[off + 1] as i32 + ((g as i32 * cov_a) >> 8)).min(255) as u8;
                            bytes[off + 2] = (bytes[off + 2] as i32 + ((b as i32 * cov_a) >> 8)).min(255) as u8;
                            bytes[off + 3] = next_a;
                        }
                    }
                }
            }
        }
    }

    // Pass 2: crisp circular AA dot cores with source-over compositing.
    for dot in dots {
        let tier = &palette[dot.tier];
        let base_a = (tier.alpha * 255.0).round().clamp(0.0, 255.0) as i32;
        if base_a == 0 {
            continue;
        }
        let [r, g, b] = tier.rgb;
        let radius = dot.radius;
        let eff_r = radius + 0.45;
        let inv_r2 = 1.0 / (eff_r * eff_r);
        let cx = dot.cx;
        let cy = dot.cy;
        let ix0 = ((cx - radius - 0.35) as isize).max(0) as usize;
        let iy0 = ((cy - radius - 0.35) as isize).max(0) as usize;
        let ix1 = ((cx + radius + 0.35) as isize).min(w as isize - 1).max(0) as usize;
        let iy1 = ((cy + radius + 0.35) as isize).min(h as isize - 1).max(0) as usize;
        for py in iy0..=iy1 {
            let dy = py as f32 + 0.5 - cy;
            let dy2 = dy * dy;
            let row_pix = py * w;
            for px in ix0..=ix1 {
                let dx = px as f32 + 0.5 - cx;
                let norm2 = (dx * dx + dy2) * inv_r2;
                if norm2 >= 1.3225 {
                    continue;
                }
                let radial_cov = if norm2 <= soft_cut { 1.0 } else { (1.3225 - norm2) * inv_edge_span };
                let cov_a = ((base_a as f32 * radial_cov + 0.5) as i32).min(255);
                if cov_a <= 0 {
                    continue;
                }
                let off = (row_pix + px) * 4;
                let prev_a = bytes[off + 3] as i32;
                if prev_a == 0 {
                    bytes[off] = r;
                    bytes[off + 1] = g;
                    bytes[off + 2] = b;
                    bytes[off + 3] = cov_a as u8;
                } else {
                    let src_a = cov_a as f32 / 255.0;
                    let dst_a = prev_a as f32 / 255.0;
                    let inv_src_a = 1.0 - src_a;
                    let out_a = src_a + dst_a * inv_src_a;
                    if out_a > 0.0 {
                        let dst_w = (dst_a * inv_src_a) / out_a;
                        let src_w = src_a / out_a;
                        bytes[off] = (bytes[off] as f32 * dst_w + r as f32 * src_w + 0.5) as u8;
                        bytes[off + 1] = (bytes[off + 1] as f32 * dst_w + g as f32 * src_w + 0.5) as u8;
                        bytes[off + 2] = (bytes[off + 2] as f32 * dst_w + b as f32 * src_w + 0.5) as u8;
                        bytes[off + 3] = (out_a * 255.0 + 0.5).min(255.0) as u8;
                    }
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct PortraitRenderOptions<'a> {
    pub pixel_w: u32,
    pub pixel_h: u32,
    pub cols: usize,
    pub rows: usize,
    pub time: f32,
    pub mood: ExpressionMood,
    pub viseme: MouthViseme,
    pub energy: f32,
    pub is_speaking: bool,
    pub is_listening: bool,
    pub is_loading: bool,
    pub is_thumb: bool,
    pub dot_profile: DotProfile,
    pub bloom: f32,
    pub tint_hex: Option<&'a str>,
}

pub fn render_portrait_rgba(motion: &PortraitMotionState, opts: &PortraitRenderOptions<'_>) -> Vec<u8> {
    let assets = portrait_assets();
    let raw = sample_portrait_luminance_field(
        opts.cols,
        opts.rows,
        opts.time,
        opts.mood,
        opts.viseme,
        opts.energy,
        opts.is_speaking,
        opts.is_listening,
        motion,
        assets,
    );
    let grid = pack_braille_subpixels_ordered_bayer(&raw, opts.cols, opts.rows);
    let palette = resolve_portrait_tone_palette(
        opts.mood,
        opts.is_speaking,
        opts.is_listening,
        opts.is_loading,
        opts.is_thumb,
        opts.energy,
        opts.tint_hex,
    );
    rasterize_tonal_grid_rgba(opts.pixel_w, opts.pixel_h, &grid, &palette, opts.dot_profile, opts.bloom)
}