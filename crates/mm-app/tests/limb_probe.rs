//! What a limb is actually drawn as, in numbers.
//!
//! `shader_probe` for the things cells grow outside their membranes. Same discipline and same
//! reason: the only way to ask whether a spike is the length it was told to be, without this, is
//! to photograph one and look — and a picture cannot be differenced, so "it looks about right"
//! is where the argument stops.
//!
//! Two layers are checked and they fail differently.
//!
//! * **The geometry**, through `limbmesh`: where the quad is, how big, which way round. A fault
//!   here draws the right shape in the wrong place.
//! * **The field**, through `phantom::limb`, which is `limb.wgsl` in Rust. A fault here draws the
//!   wrong shape in the right place.
//!
//! What is *not* checked here is that the Rust copy and the WGSL agree. That needs a photograph
//! of a real frame, which is what `tools/check_outline.py` does for the body — see the note at the
//! bottom of `docs/MORPHOLOGY.md` §8.

use mm_app::limbmesh::{self, form, Buffers, Placed};
use mm_app::phantom::limb;

/// One spike, four units long and one wide, pointing along `+x` from the origin.
fn spike(half_len: f32, half_wid: f32) -> Placed {
    Placed {
        cx: 0.0,
        cy: 0.0,
        ux: 1.0,
        uy: 0.0,
        half_len,
        half_wid,
        rgba: [1.0; 4],
        form: form::SPIKE,
        extent: 1.0,
        phase: 0.0,
        count: 1.0,
        inner: 0.0,
        taper: 0.0,
        seed: 0.0,
    }
}

// --- the geometry ---------------------------------------------------------------------------

#[test]
fn a_quad_reaches_exactly_as_far_as_it_was_told_to_and_no_further() {
    // The claim the whole feature rests on: a limb is drawn at the length the data asked for.
    // Off by a factor anywhere between `LimbDot` and the vertex buffer and every spike on the
    // slide is a lie about how far a cell can reach.
    let mut buf = Buffers::default();
    buf.begin(1);
    buf.push(spike(4.0, 1.0));
    let xs: Vec<f32> = buf.positions.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = buf.positions.iter().map(|p| p[1]).collect();
    assert_eq!(xs.iter().cloned().fold(f32::MIN, f32::max), 4.0);
    assert_eq!(xs.iter().cloned().fold(f32::MAX, f32::min), -4.0);
    assert_eq!(ys.iter().cloned().fold(f32::MIN, f32::max), 1.0);
    assert_eq!(ys.iter().cloned().fold(f32::MAX, f32::min), -1.0);
}

#[test]
fn a_limb_is_rotated_into_place_and_not_merely_positioned() {
    // Every mount angle, against the arithmetic done longhand. The CPU rotates so that `uv` is
    // the limb's own frame whatever the angle — which is what lets the shader do no trigonometry
    // and lets a long thin flagellum have a long thin quad.
    for n in 0..32 {
        let a = std::f32::consts::TAU * n as f32 / 32.0;
        let (ux, uy) = (a.cos(), a.sin());
        let mut buf = Buffers::default();
        buf.begin(1);
        buf.push(Placed {
            ux,
            uy,
            ..spike(4.0, 1.0)
        });
        // The tip is the midpoint of the two `uv.x == 1` corners, and it must sit at `4 * u`.
        let tip: Vec<[f32; 3]> = buf
            .positions
            .iter()
            .zip(&buf.uvs)
            .filter(|(_, uv)| uv[0] > 0.0)
            .map(|(p, _)| *p)
            .collect();
        assert_eq!(tip.len(), 2);
        let (mx, my) = ((tip[0][0] + tip[1][0]) / 2.0, (tip[0][1] + tip[1][1]) / 2.0);
        assert!(
            (mx - ux * 4.0).abs() < 1e-4 && (my - uy * 4.0).abs() < 1e-4,
            "mount {n}: tip at ({mx}, {my}) rather than ({}, {})",
            ux * 4.0,
            uy * 4.0
        );
    }
}

#[test]
fn the_aspect_makes_the_shaders_frame_isotropic() {
    // `uv` is `-1..1` however long and thin the quad, so a circle in `uv` is an ellipse on screen.
    // The shader multiplies `uv.x` by this, and if it is wrong every field is stretched — a spike
    // comes out either stubby or needle-thin with no other symptom.
    for (len, wid, want) in [(4.0, 1.0, 4.0), (1.0, 1.0, 1.0), (20.0, 0.5, 40.0)] {
        assert!((spike(len, wid).aspect() - want).abs() < 1e-4);
    }
}

// --- the field ------------------------------------------------------------------------------

#[test]
fn a_spike_is_solid_at_the_root_and_ends_at_the_tip() {
    let aspect = 6.0;
    // Down the axis: inside all the way, and the outline crossed exactly once, at the tip.
    assert!(limb::spike(-aspect, 0.0, aspect, 0.0) < 0.0, "hollow root");
    assert!(limb::spike(0.0, 0.0, aspect, 0.0) < 0.0, "hollow middle");
    assert!(
        limb::spike(aspect - 0.01, 0.0, aspect, 0.0) < 0.0,
        "the last hundredth of the spike is missing"
    );
    assert!(
        limb::spike(aspect + 0.01, 0.0, aspect, 0.0) > 0.0,
        "the spike runs on past its own tip"
    );
    // And well outside it across the width, at the root where the limb is at its widest.
    assert!(limb::spike(-aspect, 1.4, aspect, 0.0) > 0.0);
}

#[test]
fn a_spike_is_symmetric_across_its_own_axis() {
    // Asymmetry here would be a limb that leans, and a limb that leans is one whose drawn
    // direction is not the direction the data gave it.
    let aspect = 5.0;
    for i in 0..40 {
        let qx = -aspect + 2.0 * aspect * i as f32 / 39.0;
        for qy in [0.1f32, 0.4, 0.9, 1.3] {
            let (a, b) = (
                limb::spike(qx, qy, aspect, 0.0),
                limb::spike(qx, -qy, aspect, 0.0),
            );
            assert!((a - b).abs() < 1e-6, "at ({qx}, ±{qy}): {a} against {b}");
        }
    }
}

#[test]
fn a_spike_is_a_barb_and_not_a_triangle() {
    // The profile is concave: at every point along it the spike is *narrower* than a straight
    // cone would be. A linear taper reads as an arrow drawn on the cell; concave reads as
    // something that grew, because anything that has to be both anchored and sharp is.
    let aspect = 6.0;
    let mut ever_narrower = false;
    for i in 1..40 {
        let t = i as f32 / 40.0;
        let qx = -aspect + 2.0 * aspect * t;
        // The half-width here, found by walking out until the field changes sign.
        let mut w = 0.0f32;
        while w < 2.0 && limb::spike(qx, w, aspect, 0.0) < 0.0 {
            w += 0.001;
        }
        let cone = 1.0 - t;
        assert!(
            w <= cone + 1e-3,
            "at t={t} the spike is {w} wide against a cone's {cone}: it bulges"
        );
        if w < cone - 0.02 {
            ever_narrower = true;
        }
    }
    assert!(
        ever_narrower,
        "the profile is a straight cone; the taper is not concave at all"
    );
}

#[test]
fn a_tapered_limb_keeps_a_tip_of_the_width_it_asked_for() {
    // What `taper` is for, and what the flagellum and the holdfast will need: a limb that ends in
    // a stub rather than a point.
    let aspect = 6.0;
    let taper = 0.4;
    assert!(
        limb::spike(aspect - 0.01, taper * 0.9, aspect, taper) < 0.0,
        "the tip is thinner than the taper asked for"
    );
    assert!(
        limb::spike(aspect - 0.01, taper * 1.1, aspect, taper) > 0.0,
        "the tip is fatter than the taper asked for"
    );
}

// --- the two together -------------------------------------------------------------------------

#[test]
fn a_spike_stays_inside_its_own_quad() {
    // The failure this catches is the one with no other symptom: a field that reaches past the
    // corner is silently clipped to the rectangle, so the limb comes out with a straight edge
    // where its outline should be and looks like a shape somebody meant.
    let p = spike(8.0, 1.5);
    let aspect = p.aspect();
    // Every point on the quad's boundary must be outside the field, except along the root edge,
    // which is deliberately open because the body is drawn over it.
    for i in 0..=200 {
        let t = i as f32 / 200.0;
        // The two long sides, at the extreme of `uv.y`.
        let qx = -aspect + 2.0 * aspect * t;
        assert!(
            limb::spike(qx, 1.0, aspect, 0.0) >= 0.0,
            "the spike touches the quad's long edge at t={t}"
        );
        // The tip edge.
        let qy = -1.0 + 2.0 * t;
        assert!(
            limb::spike(aspect, qy, aspect, 0.0) >= 0.0,
            "the spike touches the quad's tip edge at t={t}"
        );
    }
}

#[test]
fn exactly_the_organelles_that_reach_outside_are_drawn_outside() {
    // The catalogue is append-only, so this is the test that fires the next time an organ arrives
    // that ought to change a cell's silhouette and does not.
    let outside: Vec<&str> = mm_core::OrganelleType::all()
        .iter()
        .filter(|k| limbmesh::form_of(**k).is_some())
        .map(|k| k.name())
        .collect();
    assert_eq!(
        outside,
        vec!["cilium", "spike", "holdfast", "flagellum", "exoenzyme vesicle"],
        "the list of organelles with an outside has changed"
    );
}
