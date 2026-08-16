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

// --- the beating forms --------------------------------------------------------------------

#[test]
fn a_tuft_has_as_many_hairs_as_it_was_told_to() {
    // A cilium organelle is many small ones where a flagellum is one large one, which the
    // catalogue and `docs/FEEDING.md` §7 both say before the physics is consulted. Drawing a tuft
    // as one hair would make the pair indistinguishable at exactly the zoom the pair matters.
    let aspect = 3.0;
    for count in [2.0f32, 3.0, 4.0, 5.0] {
        // Count the runs of "inside" across the tuft, a third of the way along where the hairs
        // have separated but not yet swung far.
        let qx = -aspect + 2.0 * aspect * 0.33;
        let mut runs = 0;
        let mut inside = false;
        for i in 0..4000 {
            let qy = -1.2 + 2.4 * i as f32 / 3999.0;
            let now = limb::cilium(qx, qy, aspect, 0.0, 0.0, count) < 0.0;
            if now && !inside {
                runs += 1;
            }
            inside = now;
        }
        assert_eq!(runs, count as i32, "asked for {count} hairs and got {runs}");
    }
}

#[test]
fn a_hair_is_anchored_at_the_root_and_swings_at_the_tip() {
    // The swing goes as `t * t`. A hair that slid sideways as a rigid rod would read as a
    // twitching whisker rather than as something beating, and the root would come away from the
    // membrane it grows out of.
    let aspect = 3.0;
    let at = |t: f32, phase: f32| -> f32 {
        let qx = -aspect + 2.0 * aspect * t;
        // Walk across to find the hair's centre: the deepest point of the field.
        let (mut best, mut best_y) = (1e6f32, 0.0f32);
        for i in 0..4000 {
            let qy = -1.2 + 2.4 * i as f32 / 3999.0;
            let d = limb::cilium(qx, qy, aspect, 1.0, phase, 1.0);
            if d < best {
                best = d;
                best_y = qy;
            }
        }
        best_y
    };
    // A quarter turn apart, which is where a sine differs most.
    let (root_a, root_b) = (at(0.02, 0.0), at(0.02, 0.25));
    let (tip_a, tip_b) = (at(0.98, 0.0), at(0.98, 0.25));
    assert!(
        (root_a - root_b).abs() < 0.02,
        "the root moved by {} between two phases",
        (root_a - root_b).abs()
    );
    assert!(
        (tip_a - tip_b).abs() > 0.15,
        "the tip only moved by {} between two phases",
        (tip_a - tip_b).abs()
    );
}

#[test]
fn a_beat_reverses_when_the_power_does() {
    // The one thing a genome can do with a propulsor that nothing in the picture could say: a
    // cilium beating backwards pushes its cell backwards. `cilium_power` is signed for it, and
    // the wave has to travel the other way or the sign is carried and then thrown away.
    let aspect = 6.0;
    let sample = |extent: f32, phase: f32| -> f32 {
        let qx = -aspect + 2.0 * aspect * 0.6;
        let (mut best, mut best_y) = (1e6f32, 0.0f32);
        for i in 0..4000 {
            let qy = -1.0 + 2.0 * i as f32 / 3999.0;
            let d = limb::flagellum(qx, qy, aspect, extent, phase, 0.22);
            if d < best {
                best = d;
                best_y = qy;
            }
        }
        best_y
    };
    // Forwards and backwards at the same non-zero phase must put the whip in different places.
    let f = sample(1.0, 0.2);
    let b = sample(-1.0, 0.2);
    assert!(
        (f - b).abs() > 0.05,
        "a reversed beat is drawn identically: {f} against {b}"
    );
    // And at phase zero they coincide, because the wave has not travelled yet — which is what
    // says the sign is on the *travel* and not on the shape.
    assert!((sample(1.0, 0.0) - sample(-1.0, 0.0)).abs() < 1e-4);
}

#[test]
fn a_flagellums_wave_grows_towards_its_free_end() {
    // What makes a whip a whip rather than a wiggly line: the base is held by the body and the
    // far end is free, so the amplitude grows along the length.
    let aspect = 8.0;
    let envelope = |t: f32| -> f32 {
        let qx = -aspect + 2.0 * aspect * t;
        let mut worst = 0.0f32;
        for phase in 0..24 {
            let p = phase as f32 / 24.0;
            let (mut best, mut best_y) = (1e6f32, 0.0f32);
            for i in 0..2000 {
                let qy = -1.0 + 2.0 * i as f32 / 1999.0;
                let d = limb::flagellum(qx, qy, aspect, 1.0, p, 0.22);
                if d < best {
                    best = d;
                    best_y = qy;
                }
            }
            worst = worst.max(best_y.abs());
        }
        worst
    };
    let (near, far) = (envelope(0.15), envelope(0.95));
    assert!(
        far > near * 2.0,
        "the wave is {near} at the root and {far} at the tip: it is not growing"
    );
    assert!(near < 0.2, "the root is already swinging by {near}");
}

#[test]
fn an_idle_propulsor_is_straight_and_still_there() {
    // A cilium is not a weapon. One a cell has built is one it is paying for whether or not it is
    // beating, so an idle propulsor is drawn — straight, and at the length its `param` bought.
    let aspect = 6.0;
    for phase in [0.0f32, 0.25, 0.5, 0.75] {
        assert!(
            limb::flagellum(0.0, 0.0, aspect, 0.0, phase, 0.22) < 0.0,
            "an idle flagellum has a hole down its middle at phase {phase}"
        );
        assert!(
            limb::flagellum(0.0, 0.6, aspect, 0.0, phase, 0.22) > 0.0,
            "an idle flagellum is swinging at phase {phase}"
        );
    }
}

// --- gripping and letting go ------------------------------------------------------------------

#[test]
fn a_holdfast_splays_when_it_grips_and_closes_when_it_lets_go() {
    // The readable half of the holdfast, and a decision a genome makes every tick that nothing on
    // the slide could show: gripping cement is spread against what it is gripping, and cement that
    // has let go hangs together.
    let aspect = 4.0;
    let spread = |extent: f32| -> f32 {
        // The widest the foot gets, measured at the tip.
        let mut widest = 0.0f32;
        for i in 0..2000 {
            let qy = -1.0 + 2.0 * i as f32 / 1999.0;
            if limb::holdfast(aspect - 0.02, qy, aspect, extent, 0.45, 3.0) < 0.0 {
                widest = widest.max(qy.abs());
            }
        }
        widest
    };
    let (loose, tight) = (spread(0.0), spread(1.0));
    assert!(
        tight > loose + 0.2,
        "a gripping foot spans {tight} and a loose one {loose}"
    );
}

#[test]
fn a_holdfast_that_has_let_go_hangs_off_its_own_axis() {
    // Limp, and seeded so two feet on one cell do not slump identically — which would read as a
    // symmetry the cell does not have.
    let aspect = 4.0;
    let lean = |extent: f32, seed: f32| -> f32 {
        let qx = -aspect + 2.0 * aspect * 0.5;
        let (mut best, mut best_y) = (1e6f32, 0.0f32);
        for i in 0..2000 {
            let qy = -1.0 + 2.0 * i as f32 / 1999.0;
            let d = limb::holdfast(qx, qy, aspect, extent, 0.45, seed);
            if d < best {
                best = d;
                best_y = qy;
            }
        }
        best_y
    };
    // Straight to within the resolution of the scan, which is a thousandth of the quad's width.
    assert!(
        lean(1.0, 3.0).abs() < 0.01,
        "a gripping holdfast leans by {}",
        lean(1.0, 3.0)
    );
    // Across a spread of seeds, some slump one way and some the other.
    let leans: Vec<f32> = (0..12).map(|s| lean(0.0, s as f32)).collect();
    assert!(
        leans.iter().any(|l| *l > 0.05) && leans.iter().any(|l| *l < -0.05),
        "every slack holdfast slumps the same way: {leans:?}"
    );
}

// --- the cloud ---------------------------------------------------------------------------------

#[test]
fn the_halo_is_densest_against_the_cell_and_reaches_zero_on_its_own() {
    // A hard edge on it would be a claim about a boundary that does not exist: an exoenzyme puts
    // what it dissolves in the *square*, and the square has no rim.
    let inner = 0.6;
    let at = |r: f32| -> f32 {
        // Averaged around the ring, because it is curdled and one sample is noise.
        let n = 64;
        (0..n)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / n as f32;
                limb::halo(r * a.cos(), r * a.sin(), 1.0, inner, 2.0)
            })
            .sum::<f32>()
            / n as f32
    };
    let near = at(inner + 0.01);
    let mid = at((inner + 1.0) / 2.0);
    assert!(near > mid, "the cloud is thicker away from the cell");
    assert!(mid > at(0.99), "the cloud does not thin towards its edge");
    assert!(at(0.999) < 0.002, "the cloud has a visible rim: {}", at(0.999));
    assert_eq!(at(1.2), 0.0, "the cloud reaches outside its own quad");
    // Inside the membrane there is nothing to draw: the body covers it and the enzyme is outside.
    assert_eq!(at(inner - 0.05), 0.0);
}

#[test]
fn the_halo_thickens_with_the_throttle_and_vanishes_when_shut() {
    let inner = 0.5;
    let total = |throttle: f32| -> f32 {
        (0..200)
            .map(|i| {
                let r = inner + (1.0 - inner) * i as f32 / 199.0;
                limb::halo(r, 0.0, throttle, inner, 5.0)
            })
            .sum::<f32>()
    };
    assert!(total(1.0) > total(0.5) * 1.6);
    assert!(total(0.5) > total(0.1) * 3.0);
    assert_eq!(total(0.0), 0.0, "a shut vesicle still dissolves the water");
}

// --- the junctions ------------------------------------------------------------------------

#[test]
fn a_junction_thins_as_it_approaches_breaking() {
    // The most useful new thing on the slide. A hard junction breaks a fixed distance past its own
    // rest length, and until this existed a colony came apart between one frame and the next with
    // nothing having said it was about to.
    let aspect = 6.0;
    let taper = 0.33;
    let half_width = |strain: f32| -> f32 {
        let mut w = 0.0f32;
        while w < 2.0 && limb::band(0.0, w, aspect, strain, taper) < 0.0 {
            w += 0.001;
        }
        w
    };
    let slack = half_width(0.0);
    let taut = half_width(1.0);
    assert!(slack > 0.99, "an unstrained junction is already thin: {slack}");
    assert!(
        taut < slack * 0.4,
        "at breaking point it is still {taut} of {slack} wide"
    );
    // And it does not vanish before it goes: a warning you cannot see is not a warning.
    assert!(taut > 0.2, "it is a hairline before it breaks: {taut}");
    // Monotone, so the thinning reads as a continuous approach rather than a jump.
    let widths: Vec<f32> = (0..=10).map(|i| half_width(i as f32 / 10.0)).collect();
    assert!(
        widths.windows(2).all(|w| w[1] <= w[0] + 1e-4),
        "the thinning is not monotone: {widths:?}"
    );
}

#[test]
fn a_junction_has_square_ends_and_stays_in_its_quad() {
    // A desmosome is a patch of wall. A rounded end reads as a rod lying on top of the pair rather
    // than as the thing holding them together — and a field that reached past the corner would be
    // clipped to the rectangle and get a straight edge somebody would read as meant.
    let aspect = 5.0;
    assert!(limb::band(aspect - 0.01, 0.0, aspect, 0.0, 0.33) < 0.0);
    assert!(limb::band(aspect + 0.01, 0.0, aspect, 0.0, 0.33) > 0.0);
    // Square: the full width is present right up to the end, which a rounded cap would not be.
    assert!(limb::band(aspect - 0.01, 0.95, aspect, 0.0, 0.33) < 0.0);
    for i in 0..=200 {
        let t = i as f32 / 200.0;
        assert!(limb::band(-aspect + 2.0 * aspect * t, 1.0, aspect, 0.0, 0.33) >= 0.0);
    }
}

#[test]
fn a_channel_is_pores_and_a_band_is_a_bar() {
    // One is structure and the other is a conversation, and they must not be the same mark: a
    // colony wired for transfer should read differently from one merely held together, which is a
    // distinction SPEC §8.1 makes and the picture never did.
    let aspect = 6.0;
    for count in [2.0f32, 3.0, 5.0] {
        let mut runs = 0;
        let mut inside = false;
        for i in 0..8000 {
            let qx = -aspect + 2.0 * aspect * i as f32 / 7999.0;
            let now = limb::channel(qx, 0.0, aspect, count) < 0.0;
            if now && !inside {
                runs += 1;
            }
            inside = now;
        }
        assert_eq!(runs, count as i32, "asked for {count} pores and got {runs}");
    }
    // A band is one unbroken run over the same span, or the two read alike.
    let mut runs = 0;
    let mut inside = false;
    for i in 0..8000 {
        let qx = -aspect + 2.0 * aspect * i as f32 / 7999.0;
        let now = limb::band(qx, 0.0, aspect, 0.0, 0.33) < 0.0;
        if now && !inside {
            runs += 1;
        }
        inside = now;
    }
    assert_eq!(runs, 1, "a hard junction is drawn with holes in it");
}

#[test]
fn the_junctions_are_the_only_forms_drawn_over_the_cells() {
    // A form in the wrong layer is invisible or is drawn over a body it should be behind, and
    // neither says which line was wrong. The junctions have to be over — under them, a hard
    // junction between two packed cells is entirely inside the two bodies, which is the bug this
    // whole commit is about.
    assert!(limbmesh::over_cells(form::BAND));
    assert!(limbmesh::over_cells(form::CHANNEL));
    for f in [form::SPIKE, form::CILIUM, form::FLAGELLUM, form::HOLDFAST, form::HALO] {
        assert!(!limbmesh::over_cells(f), "form {f} is drawn over the cells");
    }
}

/// And the two layers are on either side of the cell mesh, which sits at 1.0.
///
/// A `const` assertion rather than one inside a test, because both sides are constants and this
/// way it is a compile error rather than a failure somebody has to run something to see.
const _: () = {
    assert!(limbmesh::LIMB_Z < 1.0, "limbs are drawn over the cells");
    assert!(limbmesh::OVER_Z > 1.0, "junctions are drawn under the cells");
};

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
