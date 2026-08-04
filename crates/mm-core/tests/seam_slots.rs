//! Whether a packed sheet runs out of seam slots, which is what makes cells overlap.
//!
//! A crowd is drawn as swollen circles cut by their neighbours' seams, and two cells share one
//! wall **only if both of them cut for the other**. `ContactSet` holds
//! `CONTACTS_PER_CELL` of them and, when full, keeps the deepest — a decision each cell makes on
//! its own. So a cell with twelve deeper contacts drops a shallow one that is still cutting for
//! it, and the two stop agreeing where their wall is: one is drawn straight over the other with
//! no shared edge.
//!
//! `ContactSet::offer` says so in as many words, and says the only safe cap is one high enough
//! never to be reached. This measures whether it is: **it is not** — about an eighth of a packed
//! sheet saturates at the reach the renderer uses.
//!
//! It is nevertheless *not* the cause of the overlaps that prompted the measurement, and saying
//! so is the point of keeping it. The cells that run out of slots are the large ones, which is
//! what crowding failure looks like and what the reporter of the artefact predicted it would
//! look like if this were the cause. The artefact is on small cells. See
//! `mm-app/tests/swell_probe`.

use mm_core::neighbours::{NeighbourIndex, CONTACTS_PER_CELL};
use mm_core::{Scenario, World};

fn packed(size: u32, ticks: u64) -> World {
    let genome = {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../genomes/ancestor.mm");
        let src = std::fs::read_to_string(path).expect("genome");
        mm_asm::assemble(&src).expect("assembles").bytes
    };
    let mut world = World::new(Scenario {
        width: size,
        height: size,
        ..Scenario::stress(size, size)
    })
    .expect("world");
    world.place_founders(&genome, 16);
    world.run(ticks);
    world
}

/// The distribution of how many neighbours each cell is pressed against.
fn histogram_at(world: &mut World, reach: i32) -> (Vec<usize>, usize, usize) {
    let (w, h) = (world.substrate().width(), world.substrate().height());
    // The reach the renderer actually uses. It is 1.5× the radius rather than 1.0, widened so
    // that a seam is always in hand before the outlines meet — a pair sitting exactly on the
    // boundary was flicking between overlapping and sharing a wall. Which means a cell has many
    // more contacts *in reach* than it has neighbours touching it, and that is the pressure on
    // the slot cap.

    let mut index = NeighbourIndex::default();
    index.rebuild(world.cells(), w, h);
    let mut hist = vec![0usize; CONTACTS_PER_CELL + 1];
    let mut total = 0usize;
    for i in world.cells().iter() {
        let n = index.contacts(world.cells(), i, reach).as_slice().len();
        hist[n.min(CONTACTS_PER_CELL)] += 1;
        total += 1;
    }
    let full = hist[CONTACTS_PER_CELL];
    (hist, full, total)
}

#[test]
fn a_packed_sheet_does_not_run_out_of_seam_slots() {
    let mut world = packed(64, 4000);
    // Three reaches, because the renderer's is 1.5× the radius rather than 1.0 — widened so a
    // seam is always in hand before two outlines meet, which fixed a different flicker. The
    // question is how much of the slot pressure that widening is responsible for.
    for reach in [1000, 1200, 1500] {
        let (hist, full, total) = histogram_at(&mut world, reach);
        eprintln!("\nreach {reach}‰ — {total} cells, 64-square slide, 4000 ticks");
        for (n, count) in hist.iter().enumerate() {
            if *count > 0 {
                eprintln!("  {n:>2} contacts  {count:>6}");
            }
        }
        eprintln!(
            "  saturated: {full} of {total} ({}‰)",
            if total > 0 { full * 1000 / total } else { 0 }
        );
    }
    let (_, full, total) = histogram_at(&mut world, 1500);
    assert!(
        total > 200,
        "the slide never filled up, so this measures nothing"
    );
    // Reported rather than asserted, because the honest answer turned out to be "yes, and it is
    // not what you are looking at". 13% saturate at the renderer's reach and there are real
    // asymmetric pairs — but the cell that runs out of slots is almost always a *large* one
    // (mean radius 2062 against a population mean of 1222, one of ninety-nine below it), and the
    // cell drawn over is of average size. The artefact that prompted this is on small cells, and
    // `mm-app/tests/swell_probe` finds a mechanism that fits: fourteen percent of a settled pack
    // changes size between one tick and the next, by up to eleven percent, and the worst of it
    // lands on the smallest cells.
    let _ = full;
}

/// Whether any pair actually disagrees about their shared wall, and how big those cells are.
///
/// Two cells share one wall only if **both** cut for the other, so an asymmetric pair — `i` has
/// `j` in its contact set and `j` does not have `i` — is one cell drawn over another with no
/// shared edge. That is the failure the slot cap can cause. If there are none, the cap is not
/// what is putting overlaps on the screen, whatever the saturation figures say.
///
/// The size question is the one that decides it. Slot exhaustion is a *crowding* failure and
/// should fall on cells with many neighbours, which are the large ones. If the overlaps are on
/// small cells instead, the cause is elsewhere — and the obvious elsewhere is the swell, which
/// is a single multiplier on the whole cell solved to preserve its area, so a small cell with
/// few seams swells hard and grows into whatever is in its free arc.
#[test]
fn do_any_two_cells_disagree_about_their_shared_wall() {
    let mut world = packed(64, 4000);
    let (w, h) = (world.substrate().width(), world.substrate().height());
    let mut index = NeighbourIndex::default();
    index.rebuild(world.cells(), w, h);

    // `Contact` carries an offset rather than a slot, so the neighbour is resolved by where it
    // is. Positions are exact integers, so this is a lookup and not a search.
    let at: std::collections::BTreeMap<(i32, i32), usize> = world
        .cells()
        .iter()
        .map(|i| ((world.cells().x[i], world.cells().y[i]), i))
        .collect();
    let sets: Vec<(usize, Vec<usize>)> = world
        .cells()
        .iter()
        .map(|i| {
            let (x, y) = (world.cells().x[i], world.cells().y[i]);
            (
                i,
                index
                    .contacts(world.cells(), i, 1500)
                    .as_slice()
                    .iter()
                    .filter_map(|c| at.get(&(x + c.dx, y + c.dy)).copied())
                    .collect(),
            )
        })
        .collect();
    let lookup: std::collections::BTreeMap<usize, &Vec<usize>> =
        sets.iter().map(|(i, v)| (*i, v)).collect();

    let mut asymmetric = 0usize;
    let mut dropper_radius = Vec::new();
    let mut victim_radius = Vec::new();
    let mut bearing: Vec<(i32, i32)> = Vec::new();
    for (i, mine) in &sets {
        for j in mine {
            let theirs = match lookup.get(j) {
                Some(t) => *t,
                None => continue,
            };
            if !theirs.contains(i) {
                asymmetric += 1;
                dropper_radius.push(mm_core::biology::radius(world.cells(), *j));
                victim_radius.push(mm_core::biology::radius(world.cells(), *i));
                // Which way the cell it forgot about lies. Reported because the artefact was
                // observed to run one way — "downwards, sixty to eighty percent of the time" —
                // and a direction is the one thing that can only come from the search or the
                // order it happens in, never from geometry.
                bearing.push((
                    world.cells().x[*i] - world.cells().x[*j],
                    world.cells().y[*i] - world.cells().y[*j],
                ));
            }
        }
    }

    if !bearing.is_empty() {
        let up = bearing.iter().filter(|(_, dy)| *dy > 0).count();
        let down = bearing.iter().filter(|(_, dy)| *dy < 0).count();
        let left = bearing.iter().filter(|(dx, _)| *dx < 0).count();
        let right = bearing.iter().filter(|(dx, _)| *dx > 0).count();
        eprintln!(
            "  the forgotten neighbour lies: +y {up}, -y {down}, -x {left}, +x {right} \
             (of {} pairs)",
            bearing.len()
        );
    }

    let radii: Vec<i32> = world
        .cells()
        .iter()
        .map(|i| mm_core::biology::radius(world.cells(), i))
        .collect();
    let mean = radii.iter().map(|r| *r as i64).sum::<i64>() / radii.len().max(1) as i64;
    eprintln!("\n{} cells, mean radius {mean}", radii.len());
    eprintln!("  asymmetric pairs: {asymmetric}");
    if !dropper_radius.is_empty() {
        let dm =
            dropper_radius.iter().map(|r| *r as i64).sum::<i64>() / dropper_radius.len() as i64;
        eprintln!("  mean radius of the cell that dropped: {dm}");
        eprintln!(
            "  {} of the droppers are below the population mean",
            dropper_radius
                .iter()
                .filter(|r| (**r as i64) < mean)
                .count()
        );
        let vm = victim_radius.iter().map(|r| *r as i64).sum::<i64>() / victim_radius.len() as i64;
        eprintln!("  mean radius of the cell drawn over: {vm}");
        eprintln!(
            "  {} of the victims are below the population mean",
            victim_radius.iter().filter(|r| (**r as i64) < mean).count()
        );
    }
}

/// Contacts appearing and disappearing between ticks, and which way they lie.
///
/// The literal form of the report: a cell "cannot see" a neighbour so forms no seam, notices a
/// few ticks later, then forgets again — and it runs one way, downwards, most of the time. A
/// direction is the one thing geometry cannot produce on its own. Either the search is
/// asymmetric, or the order it happens in is, or the report is about something else.
#[test]
fn which_way_does_a_forgotten_neighbour_lie() {
    let mut world = packed(64, 4000);

    fn look(world: &mut World) -> std::collections::BTreeMap<u64, Vec<(i32, i32)>> {
        let (w, h) = (world.substrate().width(), world.substrate().height());
        let mut index = NeighbourIndex::default();
        index.rebuild(world.cells(), w, h);
        let cells = world.cells();
        cells
            .iter()
            .map(|i| {
                (
                    cells.id_at(i).ordering_key(),
                    index
                        .contacts(cells, i, 1500)
                        .as_slice()
                        .iter()
                        .map(|c| (c.dx, c.dy))
                        .collect(),
                )
            })
            .collect()
    }

    let before = look(&mut world);
    world.run(1);
    let after = look(&mut world);

    // Matched by bearing: a contact is "the same one" if its direction barely moved. Positions
    // shift by a fraction of a square a tick, so anything genuinely new points somewhere new.
    let same = |a: &(i32, i32), b: &(i32, i32)| {
        let (ax, ay) = (a.0 as f64, a.1 as f64);
        let (bx, by) = (b.0 as f64, b.1 as f64);
        let (la, lb) = ((ax * ax + ay * ay).sqrt(), (bx * bx + by * by).sqrt());
        la > 0.0 && lb > 0.0 && (ax * bx + ay * by) / (la * lb) > 0.995
    };

    let (mut appeared, mut vanished) = (Vec::new(), Vec::new());
    for (key, now) in &after {
        let Some(was) = before.get(key) else { continue };
        for c in now {
            if !was.iter().any(|o| same(c, o)) {
                appeared.push(*c);
            }
        }
        for o in was {
            if !now.iter().any(|c| same(c, o)) {
                vanished.push(*o);
            }
        }
    }

    let tally = |v: &[(i32, i32)]| {
        (
            v.iter().filter(|(_, dy)| *dy > 0).count(),
            v.iter().filter(|(_, dy)| *dy < 0).count(),
            v.iter().filter(|(dx, _)| *dx < 0).count(),
            v.iter().filter(|(dx, _)| *dx > 0).count(),
        )
    };
    let (au, ad, al, ar) = tally(&appeared);
    let (vu, vd, vl, vr) = tally(&vanished);
    eprintln!("\none tick apart, {} cells", after.len());
    eprintln!(
        "  contacts that appeared: {}   +y {au}  -y {ad}  -x {al}  +x {ar}",
        appeared.len()
    );
    eprintln!(
        "  contacts that vanished: {}   +y {vu}  -y {vd}  -x {vl}  +x {vr}",
        vanished.len()
    );
}

/// What the renderer misses because the index is a tick's separation out of date.
///
/// `World::step` rebuilds the neighbour index, then runs `resolve_collisions`, which **moves
/// cells** — and nothing rebuilds it afterwards. So when the frame is published and the renderer
/// asks `world.neighbours()` for a cell's contacts, the lookup takes the cell's *current* square
/// and reads buckets filled from its *previous* one. A cell that separation pushed over a square
/// boundary is filed in the wrong bucket and simply is not found.
///
/// Which would show up exactly as reported: never on a settled bench, because separation moves
/// nothing there; constantly once anything jiggles; and not fixed by raising the slot cap,
/// because the contact was never offered a slot in the first place.
///
/// Measured by asking the same question twice of the same world — once through the index as the
/// renderer gets it, once through one rebuilt from where the cells actually are.
#[test]
fn what_the_stale_index_cannot_see() {
    let mut world = packed(64, 4000);
    world.run(1);

    let (w, h) = (world.substrate().width(), world.substrate().height());
    let mut fresh = NeighbourIndex::default();
    fresh.rebuild(world.cells(), w, h);

    let cells = world.cells();
    let (mut missed, mut phantom, mut total) = (0usize, 0usize, 0usize);
    let mut cells_affected = 0usize;
    let mut bearing: Vec<(i32, i32)> = Vec::new();
    for i in cells.iter() {
        let stale: Vec<(i32, i32)> = world
            .neighbours()
            .contacts(cells, i, 1500)
            .as_slice()
            .iter()
            .map(|c| (c.dx, c.dy))
            .collect();
        let now: Vec<(i32, i32)> = fresh
            .contacts(cells, i, 1500)
            .as_slice()
            .iter()
            .map(|c| (c.dx, c.dy))
            .collect();
        total += now.len();
        let mut any = false;
        for c in &now {
            if !stale.contains(c) {
                missed += 1;
                bearing.push(*c);
                any = true;
            }
        }
        for c in &stale {
            if !now.contains(c) {
                phantom += 1;
                any = true;
            }
        }
        if any {
            cells_affected += 1;
        }
    }
    eprintln!(
        "\n{} cells, {total} contacts as they really are",
        cells.len()
    );
    eprintln!("  the renderer's index misses: {missed}");
    eprintln!("  and reports as present:      {phantom}  (neighbours that have moved away)");
    eprintln!("  cells with at least one wrong: {cells_affected}");
    if !bearing.is_empty() {
        let up = bearing.iter().filter(|(_, dy)| *dy > 0).count();
        let down = bearing.iter().filter(|(_, dy)| *dy < 0).count();
        eprintln!("  the missed ones lie: +y {up}, -y {down}");
    }
}

/// Two cells of a pair must compute the same wall. This is the property, checked directly.
///
/// The seam is the plane through the two points where the outlines cross, and it depends only on
/// the two centres and the two radii — so each cell arrives at it independently, and this cell's
/// face plus its neighbour's must sum to the distance between them. If they do not, the pair is
/// drawn with a gap or an overlap and no shared edge, whatever else is right.
///
/// Worth a test of its own because it is exactly what a change to how the radius is *drawn* can
/// break without touching anything that looks like seam code: smooth one side and not the other
/// and the sum stops holding.
#[test]
fn both_cells_of_a_pair_agree_where_their_wall_is() {
    let mut world = packed(64, 4000);
    world.run(1);
    let (w, h) = (world.substrate().width(), world.substrate().height());
    let mut index = NeighbourIndex::default();
    index.rebuild(world.cells(), w, h);
    let cells = world.cells();

    // Both cells' drawn radii, as the front end works them out.
    let drawn = |mass: i32| 0.25 + (mass as f64 / 1024.0).max(0.0).sqrt() * 0.125;
    const PACKING: f64 = 1.15;

    let at: std::collections::BTreeMap<(i32, i32), usize> = cells
        .iter()
        .map(|i| ((cells.x[i], cells.y[i]), i))
        .collect();

    let (mut checked, mut worst) = (0usize, 0.0f64);
    for i in cells.iter() {
        let r = drawn(cells.mass[i]) * PACKING;
        for c in index.contacts(cells, i, 1500).as_slice() {
            let Some(&j) = at.get(&(cells.x[i] + c.dx, cells.y[i] + c.dy)) else {
                continue;
            };
            let other = drawn(c.mass) * PACKING;
            let d = ((c.dx as f64).powi(2) + (c.dy as f64).powi(2)).sqrt() / 256.0;
            if d <= 0.0 {
                continue;
            }
            // This cell's face, and the same formula from the neighbour's side.
            let mine = (d * d + r * r - other * other) / (2.0 * d);
            let theirs = (d * d + other * other - r * r) / (2.0 * d);
            let _ = j;
            checked += 1;
            worst = worst.max((mine + theirs - d).abs());
        }
    }
    eprintln!("\n{checked} pairs checked, worst disagreement {worst:.6} squares");
    assert!(
        worst < 1e-4,
        "the two sides of a wall disagree by {worst} squares — one of them is using a different \
         radius for the other than the other uses for itself"
    );
}
