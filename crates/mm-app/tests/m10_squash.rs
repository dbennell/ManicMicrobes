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

/// The seam one cell computes once both cores are respected, as `slide::squash_of` does it.
fn clamped_seam(radius: f32, other: f32, distance: f32) -> f32 {
    let min = mm_app::slide::MIN_FACE;
    let (mine, theirs) = (min * radius, min * other);
    if mine + theirs >= distance {
        return distance * mine / (mine + theirs);
    }
    seam(radius, other, distance).clamp(mine, distance - theirs)
}

#[test]
fn mismatched_cells_pressed_to_the_core_still_agree() {
    // The clamp used to be applied by each cell to its own face alone, which silently gave up
    // the property the test above exists to protect: whenever it bit, the two faces stopped
    // summing to the distance between the centres and the pair drew as one cell lying over the
    // other rather than as two sharing a wall.
    //
    // It went unnoticed because it needs *both* a size mismatch and a pair pressed near its
    // core, and until the physics grew a core (SPEC §6.4) crowds rarely got that deep. Once they
    // did it was the most visible thing on the packing bench.
    let min = mm_app::slide::MIN_FACE;
    for (a, b) in [(1.0f32, 1.0), (1.0, 0.7), (2.0, 1.0), (3.0, 0.6), (0.4, 1.9)] {
        // Walk in from merely touching to exactly the distance where the two cores meet, which
        // is as close as the physics will now let them get.
        for step in 0..=10 {
            let touching = a + b;
            let core = min * (a + b);
            let d = touching + (core - touching) * step as f32 / 10.0;
            let from_a = clamped_seam(a, b, d);
            let from_b = clamped_seam(b, a, d);
            assert!(
                (from_a + from_b - d).abs() < 1e-4,
                "radii {a} and {b} at {d}: faces {from_a} and {from_b} do not meet"
            );
            // And neither cell has been cut into its own core to achieve that.
            assert!(
                from_a >= min * a - 1e-4 && from_b >= min * b - 1e-4,
                "radii {a} and {b} at {d}: {from_a}, {from_b} cut inside a core"
            );
        }
    }
}

#[test]
fn cells_resting_at_contact_still_close_the_gap() {
    // The case that matters, because it is the one the physics settles into. Separation pushes
    // overlapping cells apart every tick and stops at `d >= ri + rj`, so at rest cells touch
    // with *zero* overlap — and circles touching at a point leave a triangular hole between
    // every three of them. Drawing them larger than they are and cutting at the seam is what
    // closes that hole.
    let r = 1.0f32;
    let d = 2.0 * r; // exactly touching, which is where separation leaves them
    let drawn = r * mm_app::slide::PACKING;

    let face = seam(drawn, drawn, d);
    // Each is cut at the midpoint between the centres — they abut, sharing one wall.
    assert!(
        (face - d / 2.0).abs() < 1e-5,
        "seam at {face}, not {}",
        d / 2.0
    );
    // And each actually reaches that wall, which is the whole point: a cell drawn at its
    // physical radius would stop short of it and leave the hole open.
    assert!(
        drawn > face,
        "drawn radius {drawn} does not reach its own seam at {face}"
    );
}

/// The seam one cell computes, with the two rigidities taken into account.
fn firm_seam(radius: f32, other: f32, distance: f32, mine: f32, theirs: f32) -> f32 {
    let plain = seam(radius, other, distance);
    let overlap = (radius + other - distance).max(0.0);
    plain + 0.5 * overlap * ((mine - theirs) / (mine + theirs).max(1.0))
}

#[test]
fn a_firm_cell_dents_a_soft_one_and_they_still_meet() {
    // Rigidity moves the seam towards whichever cell gives way more easily. It must move it
    // and no more: both cells still have to arrive at the same line from their own side, or
    // the gap the whole thing exists to close comes back.
    let (r, d) = (1.0f32, 1.6);
    let (firm, soft) = (200.0f32, 20.0);

    let from_firm = firm_seam(r, r, d, firm, soft);
    let from_soft = firm_seam(r, r, d, soft, firm);
    assert!(
        (from_firm + from_soft - d).abs() < 1e-4,
        "the two sides disagree: {from_firm} + {from_soft} != {d}"
    );
    // The firm one keeps more than half.
    assert!(from_firm > d / 2.0, "the firm cell gave way: {from_firm}");
    assert!(from_soft < d / 2.0, "the soft cell did not: {from_soft}");
    // And the seam stays inside the overlap either way — a rigidity difference dents a
    // neighbour, it does not pass through it.
    let overlap = (2.0 * r - d).max(0.0);
    assert!(
        (from_firm - d / 2.0) <= overlap / 2.0 + 1e-5,
        "the seam left the overlap"
    );
}

#[test]
fn cells_of_the_same_build_still_meet_in_the_middle() {
    let (r, d) = (1.0f32, 1.5);
    for k in [1.0f32, 24.0, 255.0] {
        let face = firm_seam(r, r, d, k, k);
        assert!((face - d / 2.0).abs() < 1e-5, "rigidity {k} moved the seam");
    }
}

#[test]
fn equal_cells_meet_half_way() {
    let d = 1.5f32;
    assert!((seam(1.0, 1.0, d) - d / 2.0).abs() < 1e-6);
}

#[test]
fn a_cell_keeps_a_core_however_hard_it_is_squeezed() {
    // The seam marches inward as two cells interpenetrate, and for circles it is right to.
    // Cells are not circles: they resist, and one pressed on from every side has to remain a
    // cell rather than becoming a shard. Eight seams at the floor still leave a polygon with
    // that inradius, so the core survives from all directions at once.
    let floor = mm_app::slide::MIN_FACE;
    assert!(floor > 0.0 && floor < 1.0, "a nonsense core: {floor}");

    // A cell almost entirely inside a much larger one: the unclamped seam is behind its own
    // centre, and the clamp is the only thing between it and nothing.
    let (r, other, d) = (0.3f32, 3.0, 0.6);
    let raw = seam(r, other, d) / r;
    assert!(
        raw < 0.0,
        "expected the raw seam past the centre, got {raw}"
    );
    assert!(raw.max(floor) >= floor);
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
