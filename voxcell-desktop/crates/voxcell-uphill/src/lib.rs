//! Headless uphill harness (spec §9, ticket P4-04).
//!
//! Runs the Voxcell scenes (idle, working, attention, speaking, canvas,
//! thread split/overlay, phone) through rune's headless Runtime and asserts:
//! AccessKit node completeness, zero clipped text, edge-safe canvas popovers,
//! inter-frame character motion deltas, and the <4 ms/frame 120 Hz budget.
//! Owned by the uphill workstream — see docs/RUNE_DESKTOP_SPEC.md.
