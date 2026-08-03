//! What a cell costs in bytes, and what a bare point particle would.
//!
//! Answers one question and is kept for it: whether "particulate" can be cells. Run with
//! `cargo run -p mm-cli --example cellsize --release`.
fn main() {
    use mm_core::junction::Junction;
    use mm_core::vm::Vm;
    use mm_core::Organelle;
    use std::mem::size_of;

    let vm = size_of::<Vm>();
    let per_cell = 1
        + 4
        + 4 * size_of::<i32>()
        + 2 * size_of::<i32>()
        + size_of::<u32>()
        + size_of::<i32>()
        + 16 * size_of::<i32>()
        + 16 * size_of::<Organelle>()
        + 4 * size_of::<Junction>()
        + vm
        + 8
        + size_of::<Option<Vec<u8>>>()
        + 1
        + 4
        + 8
        + 8;
    println!(
        "Organelle {:>4} B   Junction {:>4} B   Vm {:>4} B",
        size_of::<Organelle>(),
        size_of::<Junction>(),
        vm
    );
    println!(
        "per cell ~{per_cell} B, of which the VM is {vm} B ({:.0}%)",
        100.0 * vm as f64 / per_cell as f64
    );
    let per_particle = 2 * size_of::<i32>() + 2;
    println!(
        "per point particle {per_particle} B  ({:.0}x smaller)",
        per_cell as f64 / per_particle as f64
    );
    for n in [50_000usize, 1_000_000] {
        println!(
            "{n:>9}:  as cells {:>7.1} MB   as points {:>6.1} MB",
            (n * per_cell) as f64 / 1e6,
            (n * per_particle) as f64 / 1e6
        );
    }
}
