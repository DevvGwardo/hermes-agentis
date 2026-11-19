//! GPU halftone instance emission (P2-02, emitter side).
//!
//! Converts the CPU halftone raster output (`BrailleFrameGrid` + 5-tier `TonalTier`
//! palette) into a GPU-ready `Vec<DotInstance>` instance buffer. Pure data only —
//! NO wgpu/rune imports: the SDF dot/corona render node lives in `voxcell-ui`.
//!
//! Dot centers/radii are produced by the exact same geometry + tonal profile scaling
//! as the CPU rasterizer (`raster::grid_to_tonal_dots`), so GPU and CPU dots land on
//! identical subpixel positions.

use crate::raster::{
    BrailleFrameGrid, DotGeometry, TonalTier, grid_to_tonal_dots, HORIZON_TIER_THRESHOLDS,
    PORTRAIT_TIER_THRESHOLDS,
};
use bytemuck::{Pod, Zeroable};
use voxcell_core::DotProfile;

/// One halftone dot instance for the GPU SDF dot/corona render node.
///
/// `#[repr(C)]` layout (20 bytes, 4-byte align):
/// `pos_xy` @0 (8B), `radius` @8 (4B), `tier` @12 (1B), `_pad` @13 (3B), `alpha` @16 (4B).
/// The explicit `_pad` keeps the struct free of *implicit* padding so `bytemuck::Pod`
/// can derive safely; the render node reads `pos_xy/radius/tier/alpha` and ignores `_pad`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct DotInstance {
    /// Dot center in device-pixel space (x right, y down).
    pub pos_xy: [f32; 2],
    /// Analytic SDF circle radius in device pixels.
    pub radius: f32,
    /// Tonal tier index 0..=4 (selects palette color/bloom on the render node).
    pub tier: u8,
    /// Explicit alignment padding (must stay zero; not part of the shading data).
    pub _pad: [u8; 3],
    /// Dot core opacity 0..1 from the tonal palette.
    pub alpha: f32,
}

// Compile-time layout guarantees for the GPU vertex/instance buffer stride.
const _: () = {
    assert!(std::mem::size_of::<DotInstance>() == 20);
    assert!(std::mem::align_of::<DotInstance>() == 4);
    assert!(std::mem::offset_of!(DotInstance, pos_xy) == 0);
    assert!(std::mem::offset_of!(DotInstance, radius) == 8);
    assert!(std::mem::offset_of!(DotInstance, tier) == 12);
    assert!(std::mem::offset_of!(DotInstance, alpha) == 16);
};

impl DotInstance {
    pub fn new(pos_xy: [f32; 2], radius: f32, tier: u8, alpha: f32) -> Self {
        Self { pos_xy, radius, tier, _pad: [0; 3], alpha }
    }
}

/// Zero-copy view of an instance slice as raw bytes for GPU buffer upload.
pub fn dot_instances_as_bytes(instances: &[DotInstance]) -> &[u8] {
    bytemuck::cast_slice(instances)
}

/// Pure emitter: raster grid + palette -> GPU-ready instance buffer.
/// Geometry/thresholds are explicit so portrait, horizon and shape callers share one path.
pub fn emit_dot_instances(
    grid: &BrailleFrameGrid,
    palette: &[TonalTier; 5],
    geom: &DotGeometry,
    dot_profile: DotProfile,
    thresholds: &[f32; 4],
) -> Vec<DotInstance> {
    let dots = grid_to_tonal_dots(grid, palette, geom, dot_profile, thresholds);
    dots.into_iter()
        .map(|d| DotInstance::new([d.cx, d.cy], d.radius, d.tier as u8, palette[d.tier].alpha))
        .collect()
}

/// Portrait instance buffer (AsciiPortraitCanvas geometry + tier cutoffs).
pub fn emit_portrait_dot_instances(
    grid: &BrailleFrameGrid,
    palette: &[TonalTier; 5],
    pixel_w: u32,
    pixel_h: u32,
    dot_profile: DotProfile,
) -> Vec<DotInstance> {
    let geom = DotGeometry::portrait(pixel_w as f32, pixel_h as f32, grid.cols, grid.rows);
    emit_dot_instances(grid, palette, &geom, dot_profile, &PORTRAIT_TIER_THRESHOLDS)
}

/// Horizon instance buffer (BrailleHorizonCanvas geometry + tier cutoffs).
pub fn emit_horizon_dot_instances(
    grid: &BrailleFrameGrid,
    palette: &[TonalTier; 5],
    pixel_w: u32,
    pixel_h: u32,
    dot_profile: DotProfile,
) -> Vec<DotInstance> {
    let geom = DotGeometry::horizon(pixel_w as f32, pixel_h as f32, grid.cols, grid.rows);
    emit_dot_instances(grid, palette, &geom, dot_profile, &HORIZON_TIER_THRESHOLDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raster::{parse_hex_rgb, resolve_portrait_tone_palette};
    use voxcell_core::ExpressionMood;

    fn sample_grid() -> BrailleFrameGrid {
        // 2x2 cells with a spread of bit densities and luminances across all 5 tiers.
        BrailleFrameGrid {
            cols: 2,
            rows: 2,
            cell_bits: vec![0x01, 0xFF, 0x0F, 0x00],
            cell_lums: vec![20, 250, 130, 0],
        }
    }

    #[test]
    fn dot_instance_layout_is_pod_and_zeroable() {
        assert_eq!(std::mem::size_of::<DotInstance>(), 20);
        let zero = DotInstance::zeroed();
        assert_eq!(zero.pos_xy, [0.0, 0.0]);
        assert_eq!(zero.radius, 0.0);
        assert_eq!(zero.tier, 0);
        assert_eq!(zero.alpha, 0.0);
        assert_eq!(zero._pad, [0; 3]);

        let inst = DotInstance::new([3.5, -2.25], 1.75, 4, 0.99);
        let one = [inst];
        let bytes = dot_instances_as_bytes(&one);
        assert_eq!(bytes.len(), 20);
        let back: &[DotInstance] = bytemuck::cast_slice(bytes);
        assert_eq!(back[0], inst);
    }

    #[test]
    fn emitter_produces_one_instance_per_set_subpixel_bit() {
        let grid = sample_grid();
        let palette = resolve_portrait_tone_palette(
            ExpressionMood::Calm, false, false, false, false, 0.1, None,
        );
        let inst = emit_portrait_dot_instances(&grid, &palette, 64, 64, DotProfile::Balanced);
        let expected_bits: usize = [0x01u8, 0xFF, 0x0F, 0x00].iter().map(|b| b.count_ones() as usize).sum();
        assert_eq!(inst.len(), expected_bits);
        // Empty cell contributes nothing.
        assert!(inst.iter().all(|d| d.radius > 0.0 && d.alpha > 0.0));
        // Tier indices stay in range and alpha matches the palette tier.
        for d in &inst {
            assert!(d.tier <= 4);
            assert_eq!(d.alpha, palette[d.tier as usize].alpha);
            assert_eq!(d._pad, [0; 3]);
        }
    }

    #[test]
    fn emitter_positions_and_radii_match_cpu_raster_geometry() {
        let grid = sample_grid();
        let palette = resolve_portrait_tone_palette(
            ExpressionMood::Calm, false, false, false, false, 0.1, None,
        );
        let geom = DotGeometry::portrait(64.0, 64.0, grid.cols, grid.rows);
        let dots = grid_to_tonal_dots(&grid, &palette, &geom, DotProfile::Balanced, &PORTRAIT_TIER_THRESHOLDS);
        let inst = emit_dot_instances(&grid, &palette, &geom, DotProfile::Balanced, &PORTRAIT_TIER_THRESHOLDS);
        assert_eq!(dots.len(), inst.len());
        for (d, i) in dots.iter().zip(inst.iter()) {
            assert_eq!(i.pos_xy, [d.cx, d.cy]);
            assert_eq!(i.radius, d.radius);
            assert_eq!(i.tier as usize, d.tier);
        }
        // All centers inside the surface for this fixture.
        assert!(inst.iter().all(|d| d.pos_xy[0] >= 0.0 && d.pos_xy[0] <= 64.0));
        assert!(inst.iter().all(|d| d.pos_xy[1] >= 0.0 && d.pos_xy[1] <= 64.0));
    }

    #[test]
    fn higher_tiers_get_larger_radii_and_phosphor_profile_scales_up() {
        let grid = BrailleFrameGrid {
            cols: 2,
            rows: 1,
            cell_bits: vec![0xFF, 0xFF],
            cell_lums: vec![30, 250], // tier 0 vs tier 4
        };
        let palette = resolve_portrait_tone_palette(
            ExpressionMood::Calm, false, false, false, false, 0.1, None,
        );
        let balanced = emit_portrait_dot_instances(&grid, &palette, 64, 32, DotProfile::Balanced);
        let phosphor = emit_portrait_dot_instances(&grid, &palette, 64, 32, DotProfile::Phosphor);
        let r0 = balanced.iter().find(|d| d.tier == 0).unwrap().radius;
        let r4 = balanced.iter().find(|d| d.tier == 4).unwrap().radius;
        assert!(r4 > r0, "highlight tier must render larger dots");
        let p4 = phosphor.iter().find(|d| d.tier == 4).unwrap().radius;
        assert!(p4 > r4, "phosphor profile must scale dots up vs balanced");
        assert_eq!(palette[0].rgb, parse_hex_rgb("#94A3B8"));
    }
}