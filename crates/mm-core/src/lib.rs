//! Manic Microbes — simulation core.
//!
//! Cells execute byte-encoded genomes in a per-cell virtual machine. This crate is that
//! machine and, from M1 onward, the world it runs in. It has no rendering, no wall clock and
//! no global state.
//!
//! Read `docs/SPEC.md` before changing anything here; it is normative. The hard rules in
//! `CLAUDE.md` are each enforced by a test in `tests/`:
//!
//! | Rule | Test |
//! |------|------|
//! | No Bevy in `mm-core` | `tests/no_bevy.rs` |
//! | No floats in `mm-core` | `tests/no_floats.rs` |
//! | No panics on any path a genome can reach | `tests/no_panic_paths.rs`, `tests/totality_fuzz.rs` |
//! | Addressing wraps, magnitudes saturate | `tests/saturation.rs` |
//! | No sequential RNG | [`rng`], `tests/no_panic_paths.rs` |
//! | Determinism | `tests/determinism.rs` |
//!
//! # M0 scope
//!
//! A total, deterministic VM and the genome representation it runs on. Nothing here has a
//! position. Opcodes `0x30`–`0x3F` reach the world through [`host::Host`], whose M0
//! implementation is a world that does not exist.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented,
    clippy::exit
)]
#![warn(missing_debug_implementations)]

pub mod arena;
pub mod biology;
pub mod cell;
pub mod chem;
pub mod config;
pub mod ecology;
pub mod events;
pub mod fixed;
pub mod fluid;
pub mod genome;
pub mod genome_file;
pub mod host;
pub mod intent;
pub mod isa;
pub mod junction;
pub mod ledger;
pub mod light;
pub mod metabolism;
pub mod metrics;
pub mod mutation;
pub mod names;
pub mod neighbours;
pub mod organelle;
pub mod phylogeny;
pub mod rng;
pub mod scenario;
pub mod sensing;
pub mod snapshot;
pub mod state_hash;
pub mod substrate;
pub mod vm;
pub mod world;

pub use biology::{BiologyConfig, BiologyReport, CellHost};
pub use cell::{CellArena, CellId, CellSeed};
pub use chem::{ChemTable, ChemicalDef, CHEM_COUNT};
pub use config::VmConfig;
pub use fixed::{POS_ONE, Q10_ONE};
pub use genome::{Genome, GenomeError, GenomePool, Promoter, MAX_GENOME_LEN};
pub use host::{Host, NullHost, RecordingHost, INJECT_SELF};
pub use intent::{Intent, IntentBuffer, Pending, PendingBirth, SenseView, SlotIntents};
pub use isa::{Op, Template, ISA_VERSION, MAX_TEMPLATE_LEN, OPCODE_COUNT};
pub use ledger::{Ledger, LedgerBreach, TrophicSource};
pub use light::{CurrentField, Edge, LightRegime};
pub use metabolism::{MetabolicRates, Metabolism};
pub use mutation::{MutationRates, Operator};
pub use neighbours::NeighbourIndex;
pub use organelle::{
    MembraneReading, Organelle, OrganelleCatalogue, OrganelleType, MEMBRANE_SLOT, SLOT_COUNT,
};
pub use rng::{mix64, Purpose, RandCtx};
pub use scenario::{Barrier, Scenario, ScenarioError, Seeding};
pub use sensing::{ChemReading, PhysicsReport, TouchReading};
pub use snapshot::{Snapshot, SnapshotError};
pub use state_hash::{StateHash, StateHasher};
pub use substrate::{Substrate, SubstrateError};
pub use vm::{Vm, CALL_STACK_LEN, DATA_STACK_LEN, RAM_WORDS, REGISTER_COUNT};
pub use world::World;
