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
        instr_per_tick: 16,
        template_search_range: 512,
        promoter_bind_threshold: 2,
    };
}

impl Default for VmConfig {
    fn default() -> Self {
        Self::DEFAULT
    }
}
