//! Cells flatten where they are pressed into each other (M10.5).
//!
//! The seam between two overlapping cells is drawn by both of them, independently, in two
//! different draw calls with no communication. So the only thing that makes them meet along one
//! line rather than leaving a gap or a doubled edge is that the arithmetic agrees from either
//! side — which is a property worth a test, because nothing about the picture says which of the
//! two got it wrong when it does not.

use mm_app::slide::Squash;

/// The seam one cell computes, from its side.
fn seam(radius: f32, other: f32, distance: f32) -> f32 {
    (distance * distance + radius * radius - other * other) / (2.0 * distance)
}

#[test]
fn two_cells_agree_where_their_shared_face_is() {
    // Each measures from its own centre, so the two answers must add up to the distance
    // between them. Anything else is a gap or an overlap along the seam.
    for (a, b, d) in [
        (1.0f32, 1.0, 1.5),
        (1.0, 0.5, 1.2),
        (2.0, 0.4, 2.0),
        (0.3, 0.3, 0.5),
        (1.0, 3.0, 2.5),
    ] {
        let from_a = seam(a, b, d);
        let from_b = seam(b, a, d);
        assert!(
            (from_a + from_b - d).abs() < 1e-4,
            "radii {a} and {b} at {d}: faces {from_a} and {from_b} do not meet"
        );
    }
}

#[test]
fn equal_cells_meet_half_way() {
    let d = 1.5f32;
    assert!((seam(1.0, 1.0, d) - d / 2.0).abs() < 1e-6);
}

#[test]
fn a_big_neighbour_cuts_past_the_centre() {
    // A cell wholly inside a much larger one has its seam behind its own centre, which is what
    // being engulfed looks like: the small one is cut away to nothing rather than drawn as a
    // disc sitting on top.
    let face = seam(0.3, 3.0, 1.0);
    assert!(face < 0.0, "expected a negative face, got {face}");
}

#[test]
fn cells_that_only_touch_are_not_cut() {
    // Exactly touching: the seam sits on the outline, so the cell is drawn whole.
    let (a, b) = (1.0f32, 1.0);
    let face = seam(a, b, a + b);
    assert!(
        (face - a).abs() < 1e-6,
        "touching should not squash: {face}"
    );
}

#[test]
fn the_packed_direction_survives_the_round_trip() {
    // Directions travel to the shader as two 16-bit snorms in one f32. A cell outline cannot
    // show more precision than that, but it can certainly show a direction that came back
    // pointing somewhere else.
    for k in 0..64 {
        let angle = k as f32 * std::f32::consts::TAU / 64.0;
        let (nx, ny) = (angle.cos(), angle.sin());
        let bits = mm_app::cellmesh::pack_normal(nx, ny).to_bits();
        let ux = ((bits & 0xFFFF) as u16 as i16) as f32 / 32767.0;
        let uy = ((bits >> 16) as u16 as i16) as f32 / 32767.0;
        assert!((ux - nx).abs() < 1e-3, "x at {angle}: {ux} vs {nx}");
        assert!((uy - ny).abs() < 1e-3, "y at {angle}: {uy} vs {ny}");
    }
}

#[test]
fn the_empty_seam_is_out_of_reach_but_not_out_of_precision() {
    // Both halves matter. Too near and it cuts a cell that has no neighbour there; too far and
    // the smooth intersection that combines the four seams loses the field to rounding —
    // `mix(b, a, h)` is `b + h*(a - b)`, and once `b` is large enough that `a - b` rounds to
    // `-b` in `f32`, the field value is gone and the cell draws as a translucent square.
    let far = mm_app::cellmesh::NO_SQUASH;
    // The furthest any pixel of the quad can be from its centre, in field units.
    let corner = 2.0f32.sqrt();
    assert!(far > corner + 1.0, "{far} is close enough to cut a corner");
    // Survives being added to and subtracted from a field value of ordinary size.
    let field = 0.125f32;
    assert!(
        (-far + (field + far) - field).abs() < 1e-4,
        "{far} loses a field value of {field} to rounding"
    );
}

#[test]
fn a_cell_with_no_neighbours_carries_no_seams() {
    // The default has to be a seam nothing can reach, because the shader applies all four
    // unconditionally rather than branching on a count.
    let s = mm_app::cellmesh::Squash::default();
    assert!(s.face >= mm_app::cellmesh::NO_SQUASH);
    let _ = Squash {
        nx: 1.0,
        ny: 0.0,
        face: 1.0,
    };
}
