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

    let before: std::collections::BTreeMap<u64, (f32, f32, usize)> = slide
        .frame()
        .cells
        .iter()
        .map(|d| {
            (
                d.id.ordering_key(),
                (d.area_swell, d.radius, d.squash.len()),
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
            jumps.push((
                (d.area_swell - was).abs(),
                d.radius,
                d.squash.len() as i32 - *seams as i32,
                d.area_swell,
            ));
        }
    }
    assert!(jumps.len() > 100, "not enough cells to measure");
    jumps.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    let lurchers: Vec<&(f32, f32, i32, f32)> = jumps.iter().filter(|j| j.0 > 0.02).collect();
    let with_change = lurchers.iter().filter(|j| j.2 != 0).count();
    eprintln!(
        "\n  of {} cells whose swell moved by >0.02, {with_change} also gained or lost a seam",
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
            "    swell moved {jump:.3} on radius {radius:.2}, seams {dseams:+}, now at {swell:.3}"
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
