//! Whether a settled pack's cells are changing *size* frame to frame.
//!
//! `slide::area_swell` grows a cell until the shape left after its neighbours' seams have cut it
//! encloses the area the cell actually has. Its own doc says the thing this measures:
//!
//! > Depends on nothing but this frame's seams — no feedback from the previous frame's swell —
//! > so it cannot oscillate on its own account. It does *amplify* a seam appearing or
//! > disappearing, because that **resizes the whole cell** rather than one edge of it.
//!
//! So a seam that comes and goes at the reach boundary does not nudge one edge — it rescales the
//! whole outline, and everywhere the cell is *not* cut it grows into whatever is there. That is a
//! candidate for overlaps appearing and vanishing all over a packed sheet, and it is a different
//! candidate from the seam-slot cap, which `mm-core`'s `seam_slots` measures.
//!
//! The size question separates them. Slot exhaustion is a crowding failure and falls on cells
//! with the most neighbours, which are the large ones. Swell is a *proportional* correction, so
//! a cell with few seams and a big area deficit swells hardest — and losing one seam out of three
//! moves it much further than losing one out of ten.

use mm_app::slide::Slide;
use mm_core::{Scenario, World};

fn packed() -> Slide {
    let genome = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(Scenario::stress(64, 64)).expect("world");
    world.place_founders(&genome, 16);
    world.run(4000);
    let mut slide = Slide::new(Scenario::stress(8, 8)).expect("slide");
    slide.set_world(world);
    slide
}

#[test]
fn how_much_a_settled_pack_changes_size_between_ticks() {
    let mut slide = packed();
    // Every cell needs a `squash` list for this to mean anything, which only exists at the
    // packed level of detail and closer.
    slide.set_camera(32.0, 32.0, 40.0, 40.0);
    slide.set_zoom(64.0);

    let before: std::collections::BTreeMap<u64, (f32, f32, Vec<(f32, f32)>)> = slide
        .frame()
        .cells
        .iter()
        .map(|d| {
            (
                d.id.ordering_key(),
                (
                    d.area_swell,
                    d.radius,
                    d.squash.iter().map(|s| (s.nx, s.ny)).collect::<Vec<_>>(),
                ),
            )
        })
        .collect();
    slide.advance(1);
    let after = slide.frame();

    // The jump, the cell's size, and whether its seam *set* changed size — which is the
    // question. A lurch with the same number of seams is the solve amplifying a small motion; a
    // lurch that coincides with a seam arriving or leaving is membership being a step, and a
    // step in the input is fixable without giving the renderer a memory.
    let mut jumps: Vec<(f32, f32, i32, f32)> = Vec::new();
    for d in after.cells.iter() {
        if let Some((was, _, seams)) = before.get(&d.id.ordering_key()) {
            // Matched by *direction*, not by count. A cell can swap one neighbour for another
            // and keep the same number of seams, and comparing counts calls that "unchanged" —
            // which is how the first version of this ruled membership out by mistake. A seam
            // with no previous seam within about eight degrees is a new one.
            let changed = d
                .squash
                .iter()
                .filter(|s| !seams.iter().any(|(nx, ny)| s.nx * nx + s.ny * ny > 0.99))
                .count()
                + seams
                    .iter()
                    .filter(|(nx, ny)| !d.squash.iter().any(|s| s.nx * nx + s.ny * ny > 0.99))
                    .count();
            jumps.push((
                (d.area_swell - was).abs(),
                d.radius,
                changed as i32,
                d.area_swell,
            ));
        }
    }
    assert!(jumps.len() > 100, "not enough cells to measure");
    jumps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let lurchers: Vec<&(f32, f32, i32, f32)> = jumps.iter().filter(|j| j.0 > 0.02).collect();
    let with_change = lurchers.iter().filter(|j| j.2 != 0).count();
    eprintln!(
        "\n  of {} cells whose swell moved by >0.02, {with_change} had a seam come or go \
         (matched by direction, not by count)",
        lurchers.len()
    );

    let n = jumps.len();
    let mean_radius: f32 = jumps.iter().map(|j| j.1).sum::<f32>() / n as f32;
    let moved = jumps.iter().filter(|j| j.0 > 0.01).count();
    let lurched = jumps.iter().filter(|j| j.0 > 0.05).count();
    eprintln!("\n{n} cells, one tick apart, mean drawn radius {mean_radius:.2}");
    eprintln!(
        "  swell changed at all (>0.01): {moved}  ({}‰)",
        moved * 1000 / n
    );
    eprintln!(
        "  lurched (>0.05):              {lurched}  ({}‰)",
        lurched * 1000 / n
    );
    eprintln!("  worst ten:");
    for (jump, radius, dseams, swell) in jumps.iter().take(10) {
        eprintln!(
            "    swell moved {jump:.3} on radius {radius:.2}, {dseams} seams changed, now at {swell:.3}"
        );
    }
    let all_swell: f32 = jumps.iter().map(|j| j.3).sum::<f32>() / n as f32;
    let lurch_swell: f32 = lurchers.iter().map(|j| j.3).sum::<f32>() / lurchers.len().max(1) as f32;
    eprintln!(
        "  mean swell: {lurch_swell:.3} for the lurchers, {all_swell:.3} for everybody \
         (cap {:.2})",
        1.25
    );
    let big_jumpers: Vec<f32> = jumps.iter().filter(|j| j.0 > 0.05).map(|j| j.1).collect();
    if !big_jumpers.is_empty() {
        let m: f32 = big_jumpers.iter().sum::<f32>() / big_jumpers.len() as f32;
        eprintln!("  mean radius of the lurchers: {m:.2}, against {mean_radius:.2} for everybody");
    }
}

/// *Why* the solve is so sensitive — the input to it, on both ticks, for the worst cell.
///
/// Three explanations are already ruled out: the seam-slot cap (falls on large cells, the
/// artefact is on small), seam membership (18 of 21 lurches had an unchanged seam set), and
/// stiffness near enclosure (the lurchers are *less* swollen than average). Rebuilding the solve
/// without knowing which input moved would be fixing whatever is easiest to reach.
#[test]
fn what_moved_under_the_worst_lurch() {
    let mut slide = packed();
    slide.set_camera(32.0, 32.0, 40.0, 40.0);
    slide.set_zoom(64.0);

    let before: std::collections::BTreeMap<u64, (f32, Vec<(f32, f32, f32)>)> = slide
        .frame()
        .cells
        .iter()
        .map(|d| {
            (
                d.id.ordering_key(),
                (
                    d.area_swell,
                    d.squash.iter().map(|s| (s.nx, s.ny, s.face)).collect(),
                ),
            )
        })
        .collect();
    slide.advance(1);

    let mut worst: Option<(f32, u64)> = None;
    for d in slide.frame().cells.iter() {
        if let Some((was, _)) = before.get(&d.id.ordering_key()) {
            let jump = (d.area_swell - was).abs();
            if worst.is_none_or(|(w, _)| jump > w) {
                worst = Some((jump, d.id.ordering_key()));
            }
        }
    }
    let Some((jump, key)) = worst else { return };
    let (was, old) = before.get(&key).expect("it was there");
    let after = slide.frame();
    let now = after
        .cells
        .iter()
        .find(|d| d.id.ordering_key() == key)
        .expect("still there");

    eprintln!(
        "\nworst lurch: swell {was:.4} -> {:.4}  ({jump:.4})",
        now.area_swell
    );
    eprintln!(
        "  radius {:.3}, {} seams -> {}",
        now.radius,
        old.len(),
        now.squash.len()
    );
    eprintln!("  seam        was                     now                    moved");
    // Seams are in contact order, which is spatial, so they can be compared pairwise only when
    // the count is unchanged — which for a lurch it usually is.
    for (k, s) in now.squash.iter().enumerate() {
        match old.get(k) {
            Some((nx, ny, face)) => eprintln!(
                "  {k:>2}   n({nx:+.3},{ny:+.3}) f{face:.4}   n({:+.3},{:+.3}) f{:.4}   \
                 dn {:.4} df {:+.4}",
                s.nx,
                s.ny,
                s.face,
                ((s.nx - nx).powi(2) + (s.ny - ny).powi(2)).sqrt(),
                s.face - face,
            ),
            None => eprintln!("  {k:>2}   (new)"),
        }
    }
}

/// Whether the thing that steps is a *neighbour's radius*.
///
/// `face = (d² + r² - other²) / 2d`, then clamped into `[MIN_FACE·r, d - MIN_FACE·other]`. Both
/// `other` and the clamp's upper bound are the **neighbour's** radius, so a neighbour that
/// changes size moves this cell's seam without either of them going anywhere. A cell that
/// divides loses about a third of its radius in one tick, and a growing sheet is dividing
/// somewhere all the time.
///
/// Measured against the alternative that the pair simply moved: if the seams that jump belong to
/// neighbours whose radius jumped, it is size; if they belong to neighbours that stayed the same
/// size, it is not.
#[test]
fn is_it_a_neighbours_radius_that_steps() {
    let mut slide = packed();
    slide.set_camera(32.0, 32.0, 40.0, 40.0);
    slide.set_zoom(64.0);

    // Each cell's neighbours, as direction and the neighbour's radius. `1500` is the renderer's
    // `PACKING_PERMILLE`.
    fn look(slide: &Slide) -> std::collections::BTreeMap<u64, Vec<(f32, f32, f32)>> {
        let world = slide.world();
        let cells = world.cells();
        cells
            .iter()
            .map(|i| {
                let list = world
                    .neighbours()
                    .contacts(cells, i, 1500, &world.biology().metabolism.rates)
                    .as_slice()
                    .iter()
                    .map(|c| {
                        let d = ((c.dx as f32).powi(2) + (c.dy as f32).powi(2))
                            .sqrt()
                            .max(1e-6);
                        (c.dx as f32 / d, c.dy as f32 / d, c.radius as f32)
                    })
                    .collect();
                (cells.id_at(i).ordering_key(), list)
            })
            .collect()
    }

    let swell_before: std::collections::BTreeMap<u64, f32> = slide
        .frame()
        .cells
        .iter()
        .map(|d| (d.id.ordering_key(), d.area_swell))
        .collect();
    let nbrs_before = look(&slide);
    slide.advance(1);
    let nbrs_after = look(&slide);

    let mut lurched = 0usize;
    let mut with_resized_neighbour = 0usize;
    let mut steady = 0usize;
    let mut steady_with_resized = 0usize;
    let mut worst: Option<(f32, u64)> = None;

    for d in slide.frame().cells.iter() {
        let key = d.id.ordering_key();
        let (Some(was), Some(before), Some(after)) = (
            swell_before.get(&key),
            nbrs_before.get(&key),
            nbrs_after.get(&key),
        ) else {
            continue;
        };
        // Biggest relative radius change among neighbours matched by direction.
        let mut biggest = 0.0f32;
        for (nx, ny, r_now) in after {
            if let Some((_, _, r_was)) = before.iter().find(|(bx, by, _)| bx * nx + by * ny > 0.99)
            {
                if *r_was > 0.0 {
                    biggest = biggest.max((r_now - r_was).abs() / r_was);
                }
            }
        }
        let jump = (d.area_swell - was).abs();
        if jump > 0.02 {
            lurched += 1;
            if biggest > 0.05 {
                with_resized_neighbour += 1;
            }
            if worst.is_none_or(|(w, _)| jump > w) {
                worst = Some((jump, key));
            }
        } else {
            steady += 1;
            if biggest > 0.05 {
                steady_with_resized += 1;
            }
        }
    }

    eprintln!("\nof {lurched} cells whose swell moved by >0.02:");
    eprintln!("  {with_resized_neighbour} had a neighbour change radius by >5%");
    eprintln!("of {steady} cells that held still:");
    eprintln!("  {steady_with_resized} had a neighbour change radius by >5%");

    if let Some((jump, key)) = worst {
        eprintln!("\nworst lurch ({jump:.3}) — its neighbours, matched by direction:");
        let (before, after) = (&nbrs_before[&key], &nbrs_after[&key]);
        for (nx, ny, r_now) in after {
            match before.iter().find(|(bx, by, _)| bx * nx + by * ny > 0.99) {
                Some((_, _, r_was)) => eprintln!(
                    "  n({nx:+.3},{ny:+.3})  radius {r_was:.0} -> {r_now:.0}  ({:+.1}%)",
                    (r_now - r_was) / r_was * 100.0
                ),
                None => eprintln!("  n({nx:+.3},{ny:+.3})  (new neighbour)"),
            }
        }
    }
}

/// Does a cell's *own* drawn radius step from one tick to the next?
///
/// `biology::radius` is `0.25 + isqrt(mass in whole units) * 0.125` squares. Both the truncation
/// and the integer square root are staircases, and the tread is a *fixed* 0.125 squares — so it
/// is 17% of a 0.75-square cell and 6% of a 2.0-square one. That is the size asymmetry the
/// artefact was reported with, arriving from the one place nobody was looking: not the seams, not
/// the solve, but the radius itself.
///
/// `mm-core` is right to have it — hard rule 2 forbids floats there, and the physics wants a
/// cheap monotone radius. The *drawn* radius is the front end's business and has no such
/// constraint.
#[test]
fn does_a_cells_own_radius_step() {
    let mut slide = packed();
    slide.set_camera(32.0, 32.0, 40.0, 40.0);
    slide.set_zoom(64.0);

    let before: std::collections::BTreeMap<u64, (f32, f32)> = slide
        .frame()
        .cells
        .iter()
        .map(|d| (d.id.ordering_key(), (d.radius, d.area_swell)))
        .collect();
    slide.advance(1);

    let (mut stepped, mut total) = (0usize, 0usize);
    let mut worst = 0.0f32;
    let mut small_stepped = 0usize;
    let mut lurched_and_stepped = 0usize;
    let mut lurched = 0usize;
    for d in slide.frame().cells.iter() {
        let Some((was_r, was_s)) = before.get(&d.id.ordering_key()) else {
            continue;
        };
        total += 1;
        let step = (d.radius - was_r).abs() / was_r.max(0.0001);
        if step > 0.001 {
            stepped += 1;
            worst = worst.max(step);
            if d.radius < 1.0 {
                small_stepped += 1;
            }
        }
        if (d.area_swell - was_s).abs() > 0.02 {
            lurched += 1;
            if step > 0.001 {
                lurched_and_stepped += 1;
            }
        }
    }
    eprintln!("\n{total} cells, one tick apart");
    eprintln!(
        "  own drawn radius stepped: {stepped}  ({}‰)",
        stepped * 1000 / total.max(1)
    );
    eprintln!(
        "  worst single step:        {:.1}% of the cell's radius",
        worst * 100.0
    );
    eprintln!("  of those, below one square: {small_stepped}");
    eprintln!("  swell lurched: {lurched}, of which {lurched_and_stepped} also stepped radius");
}

/// The staircase against the smooth curve, on the same world, so nothing else can differ.
#[test]
fn the_staircase_against_the_smooth_curve() {
    let mut slide = packed();
    slide.set_camera(32.0, 32.0, 40.0, 40.0);
    slide.set_zoom(64.0);

    fn radii(slide: &Slide) -> std::collections::BTreeMap<u64, (f32, f32)> {
        let cells = slide.world().cells();
        cells
            .iter()
            .map(|i| {
                let m = (cells.mass[i] as f32 / 1024.0).max(0.0);
                (
                    cells.id_at(i).ordering_key(),
                    (
                        // What `mm-core` reports, in squares.
                        mm_core::biology::radius(cells, i) as f32 / 1024.0,
                        // The same curve without the two truncations.
                        0.25 + m.sqrt() * 0.125,
                    ),
                )
            })
            .collect()
    }

    let before = radii(&slide);
    slide.advance(1);
    let after = radii(&slide);

    let (mut step_big, mut smooth_big, mut n) = (0usize, 0usize, 0usize);
    let (mut step_worst, mut smooth_worst) = (0.0f32, 0.0f32);
    for (key, (sa, ma)) in &after {
        let Some((sb, mb)) = before.get(key) else {
            continue;
        };
        // A division halves a cell's mass in one tick, which is the world happening rather than
        // the curve. Excluded so the comparison is about the curve.
        if (ma - mb).abs() / mb.max(0.0001) > 0.15 {
            continue;
        }
        n += 1;
        let s = (sa - sb).abs() / sb.max(0.0001);
        let m = (ma - mb).abs() / mb.max(0.0001);
        if s > 0.02 {
            step_big += 1;
        }
        if m > 0.02 {
            smooth_big += 1;
        }
        step_worst = step_worst.max(s);
        smooth_worst = smooth_worst.max(m);
    }
    eprintln!("\n{n} cells that did not divide, one tick apart");
    eprintln!(
        "  staircase: {step_big} changed by >2%, worst {:.1}%",
        step_worst * 100.0
    );
    eprintln!(
        "  smooth:    {smooth_big} changed by >2%, worst {:.1}%",
        smooth_worst * 100.0
    );
}
