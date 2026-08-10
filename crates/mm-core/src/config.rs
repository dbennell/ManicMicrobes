//! VM tuning parameters (SPEC §16).
//!
//! These are scenario-level knobs. At M0 they are constructed in code; M1 loads them from
//! `.ron` alongside the substrate and chemistry tables.

/// Parameters that change how a genome executes. Part of the reproducibility contract: two
/// runs with different `VmConfig` are not comparable, so the values belong in the scenario
/// file and in any saved state.
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct VmConfig {
    /// Instructions a cell may execute per tick (SPEC §5). `HALT` yields the remainder.
    pub instr_per_tick: u16,
    /// How far a jump scans outward from the instruction pointer looking for the
    /// complementary template, in bytes (SPEC §4.3). Also bounded by the genome length, so a
    /// short genome is never scanned more than once around.
    pub template_search_range: u16,
    /// Maximum Hamming distance at which `EXPRESS` binds a promoter (SPEC §4.4). A larger
    /// value makes transcription-factor binding more promiscuous.
    pub promoter_bind_threshold: u16,
}

impl VmConfig {
    /// The spec defaults.
    pub const DEFAULT: VmConfig = VmConfig {
        // Halved with the metabolic rates, so behaviour keeps step with the body. Leaving it at
        // sixteen while growth halved would have doubled how many times a cell runs its genome
        // per division: the copy loop is one byte per `COPYB`, so the 227-byte ancestor already
        // spends about 28 ticks of a generation doing nothing but copying itself, and that ratio
        // is worth holding still while the tempo moves.
        //
        // `template_search_range` and `promoter_bind_threshold` are not rates — one is a distance
        // in bytes and the other a Hamming distance — so neither moves.
        instr_per_tick: 8,
        template_search_range: 512,
        promoter_bind_threshold: 2,
    };
}

impl Default for VmConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}
