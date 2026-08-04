//! Complete world state, serialised (invariant I7).
//!
//! > Complete world state round-trips through serialisation with bit-identical resumption.
//! > This is a hard requirement now even though networking is deferred, because retrofitting
//! > it is infeasible.
//!
//! # Why a hand-rolled binary format
//!
//! The scenario is `.ron` because a human edits it. A snapshot is not: it is four million
//! integers on a 512×512 grid, and as text it would be tens of megabytes of digits that
//! nobody reads. So this is a compact binary format, written by hand.
//!
//! Hand-rolled rather than reached for from a crate, for two reasons. The first is that
//! "bit-identical resumption" is the whole requirement, and a format whose exact byte
//! layout is under our control is one where that is checkable by reading it. The second is
//! versioning: every save carries the ISA version, and a genome archived under a different
//! opcode table means something different now than it did when it evolved (SPEC §16). That
//! refusal has to be explicit, not a deserialisation error about an unexpected field.
//!
//! # Adding state
//!
//! Hard rule 7: *if you add state, extend the serialisation and its test in the same
//! commit.* [`Snapshot::write`] and [`Snapshot::read`] must stay mirror images, and
//! `world_survives_a_round_trip` is the test that catches it when they do not.

use crate::cell::{CellId, RestoredCell};
use crate::chem::CHEM_COUNT;
use crate::isa::ISA_VERSION;
use crate::organelle::{Organelle, OrganelleType, SLOT_COUNT};
use crate::scenario::Scenario;
use crate::vm::{Vm, CALL_STACK_LEN, DATA_STACK_LEN, RAM_WORDS, REGISTER_COUNT};
use crate::world::World;

/// Magic bytes at the head of every snapshot: "MMSNAP\0\x01".
pub const MAGIC: [u8; 8] = *b"MMSNAP\0\x01";
/// Snapshot format version, distinct from the ISA version. The format may change without
/// the meaning of a genome changing, and vice versa.
pub const FORMAT_VERSION: u16 = 10;

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SnapshotError {
    NotASnapshot,
    /// A format this build does not know how to read.
    FormatVersion {
        found: u16,
        expected: u16,
    },
    /// Genomes in this save mean something different under this engine's opcode table.
    IsaMismatch {
        found: u16,
        expected: u16,
    },
    /// The file ended in the middle of a field.
    Truncated {
        at: usize,
    },
    /// The embedded scenario did not parse.
    Scenario(String),
    /// A length field described more data than the file contains, or than is legal.
    Corrupt(String),
}

impl std::fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SnapshotError::NotASnapshot => write!(f, "not a Manic Microbes snapshot"),
            SnapshotError::FormatVersion { found, expected } => write!(
                f,
                "snapshot format version {found}, this build reads {expected}"
            ),
            SnapshotError::IsaMismatch { found, expected } => write!(
                f,
                "snapshot was made under ISA version {found}, this engine is version \
                 {expected}; every stored genome means something different under a \
                 different opcode table, so it will not be resumed"
            ),
            SnapshotError::Truncated { at } => {
                write!(f, "snapshot ends unexpectedly at byte {at}")
            }
            SnapshotError::Scenario(e) => write!(f, "embedded scenario: {e}"),
            SnapshotError::Corrupt(e) => write!(f, "corrupt snapshot: {e}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

/// Append-only cursor over a byte buffer.
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn new() -> Writer {
        Writer { bytes: Vec::new() }
    }
    fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
    fn i64(&mut self, v: i64) {
        self.u64(v as u64);
    }
    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }
    fn i32_slice(&mut self, v: &[i32]) {
        self.u64(v.len() as u64);
        for x in v {
            self.i32(*x);
        }
    }
    fn bool_slice(&mut self, v: &[bool]) {
        // One byte per flag. A bitset would be eight times smaller, but barriers are sparse
        // and a snapshot is not the memory budget; clarity wins over a factor of eight on
        // the one field that is easiest to get subtly wrong.
        self.u64(v.len() as u64);
        for x in v {
            self.bool(*x);
        }
    }
    fn string(&mut self, v: &str) {
        self.u64(v.len() as u64);
        self.bytes.extend_from_slice(v.as_bytes());
    }
    /// A length-prefixed byte string.
    fn blob(&mut self, v: &[u8]) {
        self.u64(v.len() as u64);
        self.bytes.extend_from_slice(v);
    }
}

/// Reading cursor. Every read is bounds-checked and returns an error rather than panicking:
/// a snapshot may be truncated, corrupt or hostile, and none of those may take the process
/// down.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Reader<'a> {
        Reader { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], SnapshotError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(SnapshotError::Truncated { at: self.at })?;
        let slice = self
            .bytes
            .get(self.at..end)
            .ok_or(SnapshotError::Truncated { at: self.at })?;
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, SnapshotError> {
        Ok(self.take(1)?.first().copied().unwrap_or(0))
    }

    fn u16(&mut self) -> Result<u16, SnapshotError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([
            b.first().copied().unwrap_or(0),
            b.get(1).copied().unwrap_or(0),
        ]))
    }

    fn u32(&mut self) -> Result<u32, SnapshotError> {
        let b = self.take(4)?;
        let mut a = [0u8; 4];
        for (slot, byte) in a.iter_mut().zip(b) {
            *slot = *byte;
        }
        Ok(u32::from_le_bytes(a))
    }

    fn u64(&mut self) -> Result<u64, SnapshotError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        for (slot, byte) in a.iter_mut().zip(b) {
            *slot = *byte;
        }
        Ok(u64::from_le_bytes(a))
    }

    fn i32(&mut self) -> Result<i32, SnapshotError> {
        Ok(self.u32()? as i32)
    }

    fn i64(&mut self) -> Result<i64, SnapshotError> {
        Ok(self.u64()? as i64)
    }

    fn bool(&mut self) -> Result<bool, SnapshotError> {
        Ok(self.u8()? != 0)
    }

    /// A length-prefixed slice. The length is checked against what remains *before*
    /// allocating, so a corrupt header claiming four billion elements fails immediately
    /// rather than trying to reserve sixteen gigabytes.
    fn i32_vec(&mut self) -> Result<Vec<i32>, SnapshotError> {
        let n = self.u64()? as usize;
        let bytes = n.checked_mul(4).ok_or_else(|| {
            SnapshotError::Corrupt(format!("{n} elements is not a plausible length"))
        })?;
        if self.bytes.len().saturating_sub(self.at) < bytes {
            return Err(SnapshotError::Corrupt(format!(
                "claims {n} values but only {} bytes remain",
                self.bytes.len().saturating_sub(self.at)
            )));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.i32()?);
        }
        Ok(out)
    }

    fn bool_vec(&mut self) -> Result<Vec<bool>, SnapshotError> {
        let n = self.u64()? as usize;
        if self.bytes.len().saturating_sub(self.at) < n {
            return Err(SnapshotError::Corrupt(format!(
                "claims {n} flags but only {} bytes remain",
                self.bytes.len().saturating_sub(self.at)
            )));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(self.bool()?);
        }
        Ok(out)
    }

    fn byte_vec(&mut self) -> Result<Vec<u8>, SnapshotError> {
        let n = self.u64()? as usize;
        if self.bytes.len().saturating_sub(self.at) < n {
            return Err(SnapshotError::Corrupt(format!(
                "claims {n} bytes but only {} remain",
                self.bytes.len().saturating_sub(self.at)
            )));
        }
        Ok(self.take(n)?.to_vec())
    }

    fn string(&mut self) -> Result<String, SnapshotError> {
        let n = self.u64()? as usize;
        let b = self.take(n)?;
        String::from_utf8(b.to_vec())
            .map_err(|e| SnapshotError::Corrupt(format!("invalid utf-8: {e}")))
    }
}

/// Extinction causes as a tag plus an optional species id.
///
/// Written longhand rather than derived, so that adding a cause is a visible edit to the
/// format in both directions rather than a silent change of meaning.
fn write_extinction(w: &mut Writer, cause: Option<crate::phylogeny::Extinction>) {
    use crate::phylogeny::Extinction;
    let (tag, id) = match cause {
        None => (0u8, 0u32),
        Some(Extinction::SucceededByDescendant(id)) => (1, id),
        Some(Extinction::Outcompeted(id)) => (2, id),
        Some(Extinction::MassExtinction) => (3, 0),
        Some(Extinction::NeverEstablished) => (4, 0),
        Some(Extinction::Unknown) => (5, 0),
    };
    w.u8(tag);
    w.u32(id);
}

fn read_extinction(
    r: &mut Reader<'_>,
) -> Result<Option<crate::phylogeny::Extinction>, SnapshotError> {
    use crate::phylogeny::Extinction;
    let tag = r.u8()?;
    let id = r.u32()?;
    Ok(match tag {
        0 => None,
        1 => Some(Extinction::SucceededByDescendant(id)),
        2 => Some(Extinction::Outcompeted(id)),
        3 => Some(Extinction::MassExtinction),
        4 => Some(Extinction::NeverEstablished),
        5 => Some(Extinction::Unknown),
        // An unknown tag is a file this build does not understand, not a cause to guess at.
        other => {
            return Err(SnapshotError::Corrupt(format!(
                "extinction cause tag {other} is not one this build knows"
            )))
        }
    })
}

fn write_occurrence(w: &mut Writer, what: crate::events::Occurrence) {
    use crate::events::Occurrence as O;
    let (tag, n) = match what {
        O::EndogenousReplication => (0u8, 0u32),
        O::Motility => (1, 0),
        O::ChemotacticMachinery => (2, 0),
        O::PhototacticMachinery => (3, 0),
        O::Generations(n) => (4, n),
        O::NewDominantSpecies => (5, 0),
        O::MassExtinction => (6, 0),
        O::Predation => (7, 0),
        O::ForeignInjection => (8, 0),
        O::SoftJunction => (9, 0),
        O::HardJunction => (10, 0),
        O::Cluster(n) => (11, n),
        O::DifferentiatedCluster => (12, 0),
        O::SignalRelay => (13, 0),
        O::KeyMismatchJunction => (14, 0),
        O::Dormancy => (15, 0),
    };
    w.u8(tag);
    w.u32(n);
}

fn read_occurrence(r: &mut Reader<'_>) -> Result<crate::events::Occurrence, SnapshotError> {
    use crate::events::Occurrence as O;
    let tag = r.u8()?;
    let n = r.u32()?;
    Ok(match tag {
        0 => O::EndogenousReplication,
        1 => O::Motility,
        2 => O::ChemotacticMachinery,
        3 => O::PhototacticMachinery,
        4 => O::Generations(n),
        5 => O::NewDominantSpecies,
        6 => O::MassExtinction,
        7 => O::Predation,
        8 => O::ForeignInjection,
        9 => O::SoftJunction,
        10 => O::HardJunction,
        11 => O::Cluster(n),
        12 => O::DifferentiatedCluster,
        13 => O::SignalRelay,
        14 => O::KeyMismatchJunction,
        15 => O::Dormancy,
        other => {
            return Err(SnapshotError::Corrupt(format!(
                "event tag {other} is not one this build knows"
            )))
        }
    })
}

fn write_vm(w: &mut Writer, vm: &Vm) {
    for v in vm.data {
        w.u16(v as u16);
    }
    for v in vm.call {
        w.u16(v);
    }
    for v in vm.regs {
        w.u16(v as u16);
    }
    for v in vm.ram {
        w.u16(v as u16);
    }
    w.u16(vm.ip);
    w.u16(vm.pa);
    w.u16(vm.pb);
    w.u16(vm.ln);
    w.u32(vm.rand_ctr);
    w.u8(vm.dsp);
    w.u8(vm.dlen);
    w.u8(vm.csp);
    w.u8(vm.clen);
    w.bool(vm.halted);
}

fn read_vm(r: &mut Reader<'_>) -> Result<Vm, SnapshotError> {
    let mut vm = Vm::new();
    for i in 0..DATA_STACK_LEN {
        vm.data[i] = r.u16()? as i16;
    }
    for i in 0..CALL_STACK_LEN {
        vm.call[i] = r.u16()?;
    }
    for i in 0..REGISTER_COUNT {
        vm.regs[i] = r.u16()? as i16;
    }
    for i in 0..RAM_WORDS {
        vm.ram[i] = r.u16()? as i16;
    }
    vm.ip = r.u16()?;
    vm.pa = r.u16()?;
    vm.pb = r.u16()?;
    vm.ln = r.u16()?;
    vm.rand_ctr = r.u32()?;
    vm.dsp = r.u8()?;
    vm.dlen = r.u8()?;
    vm.csp = r.u8()?;
    vm.clen = r.u8()?;
    vm.halted = r.bool()?;
    Ok(vm)
}

const fn mm_income_len() -> usize {
    crate::ledger::TrophicSource::COUNT
}

/// Save and restore of complete world state.
#[derive(Clone, Copy, Debug)]
pub struct Snapshot;

impl Snapshot {
    /// Serialise a world.
    ///
    /// # Errors
    ///
    /// Only if the embedded scenario cannot be rendered to `.ron`.
    pub fn write(world: &World) -> Result<Vec<u8>, SnapshotError> {
        let mut w = Writer::new();
        w.bytes.extend_from_slice(&MAGIC);
        w.u16(FORMAT_VERSION);
        w.u16(ISA_VERSION);

        let scenario_ron = world
            .scenario()
            .to_ron()
            .map_err(|e| SnapshotError::Scenario(e.to_string()))?;
        w.string(&scenario_ron);

        w.u64(world.tick_count());

        let s = world.substrate();
        w.u32(s.width());
        w.u32(s.height());
        for c in 0..CHEM_COUNT {
            w.i32_slice(s.chem_plane(c));
        }
        w.i32_slice(s.light());
        let (vx, vy) = s.velocity();
        w.i32_slice(vx);
        w.i32_slice(vy);
        w.bool_slice(s.blocked());

        let (ix, iy) = world.impulses();
        w.i32_slice(ix);
        w.i32_slice(iy);
        // Per-cell, not per-square, and genuinely state: the physics phase writes it and the
        // *next* tick's division reads it, so a world restored without it lets through the first
        // round of divisions the original refused. Caught by the round-trip test, which is what
        // it is for.
        w.i32_slice(world.pressure());

        // The population, slot for slot including the empty ones: a free slot still has to
        // exist or every id after it would shift, and ids are held across saves.
        let cells = world.cells();
        w.u64(cells.capacity() as u64);
        for i in 0..cells.capacity() {
            // The generation goes out for every slot, occupied or not: a free slot's
            // generation decides the id of whoever moves in next.
            w.u32(cells.generation_at(i));
            match cells.snapshot_slot(i) {
                None => w.bool(false),
                Some(c) => {
                    w.bool(true);
                    w.i32(c.x);
                    w.i32(c.y);
                    w.i32(c.vx);
                    w.i32(c.vy);
                    w.i32(c.mass);
                    w.i32(c.energy);
                    w.u32(c.age);
                    w.i32(c.damage);
                    w.i32_slice(c.interior);
                    w.u64(c.slots.len() as u64);
                    for o in c.slots {
                        w.u8(o.kind as u8);
                        w.u8(o.param);
                        w.u16(o.remaining_build);
                        w.u16(o.control[0] as u16);
                        w.u16(o.control[1] as u16);
                    }
                    // Junctions (SPEC §8). Both ends are written, so a restored world has the
                    // same relationships and not merely the same cells.
                    w.u64(c.junctions.len() as u64);
                    for j in c.junctions {
                        w.u8(match j.kind {
                            crate::junction::JunctionKind::None => 0,
                            crate::junction::JunctionKind::Soft => 1,
                            crate::junction::JunctionKind::Hard => 2,
                        });
                        w.u32(j.other.slot());
                        w.u32(j.other.generation());
                        w.i32(j.rest);
                    }
                    write_vm(&mut w, c.vm);
                    w.blob(c.genome.bytes());
                    match c.daughter {
                        Some(d) => {
                            w.bool(true);
                            w.blob(d);
                        }
                        None => w.bool(false),
                    }
                    w.u8(c.key);
                    w.u16(c.badge);
                    w.u32(c.species);
                    w.u32(c.parent.slot());
                    w.u32(c.parent.generation());
                    w.u64(c.birth_tick);
                }
            }
        }

        // ...and the free list, in order.
        w.u64(cells.free_list().len() as u64);
        for slot in cells.free_list() {
            w.u32(*slot);
        }

        let l = world.ledger();
        for v in l.chem_totals() {
            w.i64(v);
        }
        for v in l.evicted() {
            w.i64(v);
        }
        for v in l.injected() {
            w.i64(v);
        }
        for v in l.drained() {
            w.i64(v);
        }
        w.i64(l.energy_in());
        w.i64(l.energy_out());
        w.i64(l.energy_stored());
        w.i64(l.energy_imported());
        w.i64(l.energy_exported());
        w.i64(l.converted());
        for v in l.income() {
            w.i64(v);
        }

        // --- interventions: parameter changes made while the world was running (M10.2) ---
        //
        // The configuration itself is *not* written here. It is in the embedded scenario, which
        // this format has always carried, and M10.2 moved `BiologyConfig` into `Scenario` so
        // that it would be. What is left is the history: the founding parameters plus these
        // changes, replayed in order, are the parameters in force now.
        //
        // Sixty lines of hand-written field-by-field serialisation used to live here, and it
        // was the reason this format's version moved three times in two milestones for changes
        // that should have been free. It was also a hard-rule-7 bug when it did not exist at
        // all: before M6, a world restored into `BiologyConfig::default()` was a *different
        // world*, and the first thing to notice was an arena match — mutation off when it was
        // saved, back on when it was resumed, diverging twenty ticks later while the state hash
        // at the moment of restore matched perfectly.
        let interventions = world.interventions();
        w.u32(interventions.len() as u32);
        for step in interventions {
            w.u64(step.tick);
            // RON rather than a field-by-field encoding, for exactly the reason above: a
            // parameter added to `BiologyConfig` must not need a matching pair of lines here
            // and a version bump. `serde(default)` on every config struct means an older
            // snapshot's text still loads, with anything it does not name taking its default.
            w.string(&ron::to_string(&step.biology).unwrap_or_default());
        }

        // --- the species archive and the world's newspaper (SPEC §10) ---
        //
        // Founder genomes are written out in full, which is the one place this format stores
        // genome bytes for something that may be long dead. That is deliberate and it is what
        // SPEC §10.3 asks for: full genomes for species founders, aggregates for everyone
        // else. A wiki page whose founder genome could not be loaded into the editor would be
        // a page about nothing.
        let archive = world.archive();
        w.u32(archive.next_id());
        w.u64(archive.pruned());
        w.u64(archive.forks());
        w.u32(archive.speciation_threshold);
        w.u32(archive.genus_threshold);
        w.u64(archive.sample_interval);
        w.u32(archive.len() as u32);
        for s in archive.iter() {
            w.u32(s.id);
            // `u32::MAX` for "no parent": a root. Species ids are handed out from zero and
            // never reused, so the sentinel cannot collide with a real one.
            w.u32(s.parent.unwrap_or(u32::MAX));
            w.string(&s.name.genus);
            w.string(&s.name.epithet);
            w.u32(s.genus);
            w.u64(s.founded_tick);
            w.u64(s.founder_fingerprint);
            w.blob(s.founder_genome.bytes());
            for c in s.traits.counts {
                w.u8(c);
            }
            w.u16(s.traits.genome_len);
            w.u32(s.population);
            w.u32(s.peak_population);
            w.u64(s.peak_tick);
            w.u64(s.births);
            w.u64(s.deaths);
            w.u32(s.depth);
            w.u64(s.extinct_tick.unwrap_or(u64::MAX));
            write_extinction(&mut w, s.extinction);
            w.bool(s.traits_settled);
            w.u64(s.curve.interval());
            w.u32(s.curve.points().len() as u32);
            for p in s.curve.points() {
                w.u64(p.tick);
                w.u32(p.population);
            }
        }

        let log = world.events();
        let (window_pop, window_at, generations, dominant) = log.window_state();
        w.u32(window_pop);
        w.u64(window_at);
        w.u32(generations);
        w.u32(dominant.unwrap_or(u32::MAX));
        w.u32(log.events().len() as u32);
        for e in log.events() {
            w.u64(e.tick);
            write_occurrence(&mut w, e.what);
            w.u32(e.species);
            w.i32(e.x);
            w.i32(e.y);
        }
        w.u64(world.births_total());
        // The junction-era counters. Hard rule 7 again — and added here in the same edit that
        // put them in the hash, which is the only way not to repeat the `BiologyConfig` bug.
        w.u64(world.foreign_injections_total());
        w.u64(world.forced_joins_total());
        w.u64(world.wounds_total());

        Ok(w.bytes)
    }

    /// Restore a world.
    ///
    /// # Errors
    ///
    /// A file that is not a snapshot, is a format or ISA version this build will not honour,
    /// is truncated, or is internally inconsistent.
    pub fn read(bytes: &[u8]) -> Result<World, SnapshotError> {
        let mut r = Reader::new(bytes);
        let magic = r.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(SnapshotError::NotASnapshot);
        }
        let format = r.u16()?;
        if format != FORMAT_VERSION {
            return Err(SnapshotError::FormatVersion {
                found: format,
                expected: FORMAT_VERSION,
            });
        }
        let isa = r.u16()?;
        if isa != ISA_VERSION {
            return Err(SnapshotError::IsaMismatch {
                found: isa,
                expected: ISA_VERSION,
            });
        }

        let scenario_ron = r.string()?;
        let scenario = Scenario::from_ron(&scenario_ron)
            .map_err(|e| SnapshotError::Scenario(e.to_string()))?;
        let tick = r.u64()?;

        let width = r.u32()?;
        let height = r.u32()?;
        if width != scenario.width || height != scenario.height {
            return Err(SnapshotError::Corrupt(format!(
                "grid is {width}x{height} but the scenario says {}x{}",
                scenario.width, scenario.height
            )));
        }

        // Build from the scenario so barriers and derived tables come out consistent, then
        // overwrite every field with the saved values. The scenario's own seeding is
        // discarded by the overwrite, which is correct: the snapshot is the truth.
        let mut world = World::new(scenario).map_err(|e| SnapshotError::Scenario(e.to_string()))?;
        let pool = world.genomes().clone_handle();
        let expected = (width as usize).saturating_mul(height as usize);

        let mut planes = Vec::with_capacity(CHEM_COUNT);
        for c in 0..CHEM_COUNT {
            let plane = r.i32_vec()?;
            if plane.len() != expected {
                return Err(SnapshotError::Corrupt(format!(
                    "chemical {c} has {} values, expected {expected}",
                    plane.len()
                )));
            }
            planes.push(plane);
        }
        let light = r.i32_vec()?;
        let vx = r.i32_vec()?;
        let vy = r.i32_vec()?;
        let blocked = r.bool_vec()?;
        let ix = r.i32_vec()?;
        let iy = r.i32_vec()?;
        let pressure = r.i32_vec()?;
        for (name, len) in [
            ("light", light.len()),
            ("vx", vx.len()),
            ("vy", vy.len()),
            ("blocked", blocked.len()),
            ("impulse_x", ix.len()),
            ("impulse_y", iy.len()),
        ] {
            if len != expected {
                return Err(SnapshotError::Corrupt(format!(
                    "{name} has {len} values, expected {expected}"
                )));
            }
        }

        // The population.
        let slot_count = r.u64()? as usize;
        if slot_count > 1 << 26 {
            return Err(SnapshotError::Corrupt(format!(
                "{slot_count} cell slots is not a plausible population"
            )));
        }
        let mut cells = Vec::with_capacity(slot_count.min(1 << 16));
        for _ in 0..slot_count {
            let generation = r.u32()?;
            if !r.bool()? {
                cells.push((generation, None));
                continue;
            }
            let x = r.i32()?;
            let y = r.i32()?;
            let vx = r.i32()?;
            let vy = r.i32()?;
            let mass = r.i32()?;
            let energy = r.i32()?;
            let age = r.u32()?;
            let damage = r.i32()?;
            let interior = r.i32_vec()?;
            if interior.len() != CHEM_COUNT {
                return Err(SnapshotError::Corrupt(format!(
                    "a cell has {} chemicals, expected {CHEM_COUNT}",
                    interior.len()
                )));
            }
            let n_slots = r.u64()? as usize;
            if n_slots != SLOT_COUNT {
                return Err(SnapshotError::Corrupt(format!(
                    "a cell has {n_slots} organelle slots, expected {SLOT_COUNT}"
                )));
            }
            let mut slots = Vec::with_capacity(SLOT_COUNT);
            for _ in 0..SLOT_COUNT {
                let kind_byte = r.u8()?;
                let kind = if kind_byte == 255 {
                    OrganelleType::Empty
                } else {
                    OrganelleType::from_operand(kind_byte as i16)
                };
                let param = r.u8()?;
                let remaining_build = r.u16()?;
                let c0 = r.u16()? as i16;
                let c1 = r.u16()? as i16;
                slots.push(Organelle {
                    kind,
                    param,
                    remaining_build,
                    control: [c0, c1],
                });
            }
            let n_junctions = r.u64()? as usize;
            if n_junctions != crate::junction::JUNCTIONS_PER_CELL {
                return Err(SnapshotError::Corrupt(format!(
                    "a cell has {n_junctions} junction slots, expected {}",
                    crate::junction::JUNCTIONS_PER_CELL
                )));
            }
            let mut junctions = Vec::with_capacity(crate::junction::JUNCTIONS_PER_CELL);
            for _ in 0..crate::junction::JUNCTIONS_PER_CELL {
                let kind = match r.u8()? {
                    0 => crate::junction::JunctionKind::None,
                    1 => crate::junction::JunctionKind::Soft,
                    2 => crate::junction::JunctionKind::Hard,
                    other => {
                        return Err(SnapshotError::Corrupt(format!(
                            "junction kind {other} is not one this build knows"
                        )))
                    }
                };
                junctions.push(crate::junction::Junction {
                    kind,
                    other: CellId::from_parts(r.u32()?, r.u32()?),
                    rest: r.i32()?,
                });
            }

            let vm = read_vm(&mut r)?;
            let genome_bytes = r.byte_vec()?;
            let genome = pool
                .intern(genome_bytes)
                .map_err(|e| SnapshotError::Corrupt(e.to_string()))?;
            let daughter = if r.bool()? { Some(r.byte_vec()?) } else { None };
            let key = r.u8()?;
            let badge = r.u16()?;
            let species = r.u32()?;
            let parent = CellId::from_parts(r.u32()?, r.u32()?);
            let birth_tick = r.u64()?;
            cells.push((
                generation,
                Some(RestoredCell {
                    x,
                    y,
                    vx,
                    vy,
                    mass,
                    energy,
                    age,
                    damage,
                    interior,
                    slots,
                    junctions,
                    vm,
                    genome,
                    daughter,
                    key,
                    badge,
                    species,
                    parent,
                    birth_tick,
                }),
            ));
        }

        let free_len = r.u64()? as usize;
        if free_len > slot_count {
            return Err(SnapshotError::Corrupt(format!(
                "free list of {free_len} for {slot_count} slots"
            )));
        }
        let mut free = Vec::with_capacity(free_len);
        for _ in 0..free_len {
            free.push(r.u32()?);
        }

        let mut chem_totals = [0i64; CHEM_COUNT];
        for slot in chem_totals.iter_mut() {
            *slot = r.i64()?;
        }
        let mut evicted = [0i64; CHEM_COUNT];
        for slot in evicted.iter_mut() {
            *slot = r.i64()?;
        }
        let mut injected = [0i64; CHEM_COUNT];
        for slot in injected.iter_mut() {
            *slot = r.i64()?;
        }
        let mut drained = [0i64; CHEM_COUNT];
        for slot in drained.iter_mut() {
            *slot = r.i64()?;
        }
        let energy_in = r.i64()?;
        let energy_out = r.i64()?;
        let energy_stored = r.i64()?;
        let energy_imported = r.i64()?;
        let energy_exported = r.i64()?;
        let converted = r.i64()?;
        let mut income = [0i64; mm_income_len()];
        for slot in income.iter_mut() {
            *slot = r.i64()?;
        }

        // --- interventions (M10.2) ---
        //
        // The configuration came back with the embedded scenario. These are the changes made
        // to it since, and `restore_interventions` applies the last of them, which is by
        // definition the one in force.
        let intervention_count = r.u32()? as usize;
        let mut interventions = Vec::with_capacity(intervention_count.min(1 << 16));
        for _ in 0..intervention_count {
            let tick = r.u64()?;
            let text = r.string()?;
            let biology = ron::from_str(&text)
                .map_err(|e| SnapshotError::Corrupt(format!("intervention at tick {tick}: {e}")))?;
            interventions.push(crate::biology::Intervention { tick, biology });
        }
        world.restore_interventions(interventions);

        // --- the species archive and the world's newspaper ---
        let next_species = r.u32()?;
        let pruned = r.u64()?;
        let forks = r.u64()?;
        let speciation_threshold = r.u32()?;
        let genus_threshold = r.u32()?;
        let sample_interval = r.u64()?;
        let species_count = r.u32()? as usize;
        let mut species = Vec::with_capacity(species_count.min(1 << 20));
        for _ in 0..species_count {
            let id = r.u32()?;
            let parent = match r.u32()? {
                u32::MAX => None,
                p => Some(p),
            };
            let genus_name = r.string()?;
            let epithet = r.string()?;
            let genus = r.u32()?;
            let founded_tick = r.u64()?;
            let founder_fingerprint = r.u64()?;
            let genome_len = r.u64()? as usize;
            let genome_bytes = r.take(genome_len)?.to_vec();
            let founder_genome = world
                .genomes()
                .intern(genome_bytes)
                .map_err(|e| SnapshotError::Scenario(e.to_string()))?;
            let mut counts = [0u8; crate::organelle::SLOT_COUNT];
            for c in counts.iter_mut() {
                *c = r.u8()?;
            }
            let trait_genome_len = r.u16()?;
            let population = r.u32()?;
            let peak_population = r.u32()?;
            let peak_tick = r.u64()?;
            let births = r.u64()?;
            let deaths = r.u64()?;
            let depth = r.u32()?;
            let extinct_tick = match r.u64()? {
                u64::MAX => None,
                t => Some(t),
            };
            let extinction = read_extinction(&mut r)?;
            let traits_settled = r.bool()?;
            let interval = r.u64()?;
            let point_count = r.u32()? as usize;
            let mut curve = crate::phylogeny::Curve::new(interval);
            let mut points = Vec::with_capacity(point_count.min(1 << 16));
            for _ in 0..point_count {
                points.push(crate::phylogeny::CurvePoint {
                    tick: r.u64()?,
                    population: r.u32()?,
                });
            }
            curve.restore(points, interval);
            species.push(crate::phylogeny::Species {
                id,
                parent,
                name: crate::names::Binomial {
                    genus: genus_name,
                    epithet,
                },
                genus,
                founded_tick,
                founder_fingerprint,
                founder_genome,
                traits: crate::names::Traits {
                    counts,
                    genome_len: trait_genome_len,
                },
                population,
                peak_population,
                peak_tick,
                births,
                deaths,
                depth,
                extinct_tick,
                extinction,
                curve,
                // Rebuilt from the parent links by `Phylogeny::restore`.
                children: Vec::new(),
                traits_settled,
            });
        }

        let window_pop = r.u32()?;
        let window_at = r.u64()?;
        let generations = r.u32()?;
        let dominant = match r.u32()? {
            u32::MAX => None,
            d => Some(d),
        };
        let event_count = r.u32()? as usize;
        let mut events = Vec::with_capacity(event_count.min(1 << 20));
        for _ in 0..event_count {
            let tick = r.u64()?;
            let what = read_occurrence(&mut r)?;
            events.push(crate::events::Event {
                tick,
                what,
                species: r.u32()?,
                x: r.i32()?,
                y: r.i32()?,
            });
        }
        let births_total = r.u64()?;
        let foreign_injections_total = r.u64()?;
        let forced_joins_total = r.u64()?;
        let wounds_total = r.u64()?;

        world.restore_cells(cells, free);
        world.restore(
            tick,
            planes,
            light,
            vx,
            vy,
            blocked,
            ix,
            iy,
            pressure,
            crate::ledger::LedgerState {
                chem: chem_totals,
                evicted,
                injected,
                drained,
                energy_in,
                energy_out,
                energy_stored,
                energy_imported,
                energy_exported,
                converted,
                income,
            },
        );
        {
            let archive = world.archive_mut();
            archive.speciation_threshold = speciation_threshold;
            archive.genus_threshold = genus_threshold;
            archive.sample_interval = sample_interval;
            archive.restore(species, next_species, pruned, forks);
        }
        world.restore_story(crate::world::RestoredStory {
            events,
            window_population: window_pop,
            window_at,
            generations,
            dominant,
            births_total,
            foreign_injections_total,
            forced_joins_total,
            wounds_total,
        });
        Ok(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::Scenario;

    fn stirred(ticks: u64) -> World {
        let mut w = World::new(Scenario::stress(24, 20)).unwrap();
        w.run(ticks);
        w
    }

    /// The same world, with somebody's hand in it twice.
    fn meddled_with(ticks: u64) -> World {
        let mut w = World::new(Scenario::stress(24, 20)).unwrap();
        w.run(ticks / 3);
        let mut config = w.biology().clone();
        config.division_energy *= 3;
        config.metabolism.rates.background_damage += 7;
        w.set_biology(config);
        w.run(ticks / 3);
        let mut config = w.biology().clone();
        config.mutation.point = 0;
        config.junctions.probe_leaks_distance = true;
        w.set_biology(config);
        w.run(ticks / 3);
        w
    }

    #[test]
    fn a_world_that_was_meddled_with_resumes_as_it_was_left() {
        // M10.2's central claim. Changing a parameter mid-run breaks I1 unless the change is
        // part of the record, so it is part of the record — and this is what makes that more
        // than an assertion in a doc comment.
        let original = meddled_with(300);
        assert_eq!(original.interventions().len(), 2);

        let restored = Snapshot::read(&Snapshot::write(&original).unwrap()).unwrap();
        assert_eq!(restored.state_hash(), original.state_hash());
        assert_eq!(
            restored.biology(),
            original.biology(),
            "the parameters in force were not restored"
        );
        assert_eq!(
            restored.interventions(),
            original.interventions(),
            "the record of who changed what, and when, was lost"
        );

        // And it must carry on identically, not merely look identical at the moment of
        // restore — which is exactly how the pre-M6 version of this bug hid.
        let mut a = original;
        let mut b = restored;
        a.run(200);
        b.run(200);
        assert_eq!(
            a.state_hash(),
            b.state_hash(),
            "they diverged after resuming"
        );
    }

    #[test]
    fn setting_the_parameters_before_the_first_tick_is_not_an_intervention() {
        // Scenario setup, not a hand in a running world. It updates the scenario instead, so
        // that the file describing the world always does describe it.
        let mut w = World::new(Scenario::stress(16, 16)).unwrap();
        let mut config = w.biology().clone();
        config.division_energy += 100;
        w.set_biology(config.clone());

        assert!(w.interventions().is_empty());
        assert_eq!(w.scenario().biology, config);

        // And setting it to what it already is changes nothing either way.
        w.run(5);
        w.set_biology(config);
        assert!(w.interventions().is_empty(), "a no-op was recorded");
    }

    #[test]
    fn a_world_survives_a_round_trip() {
        let original = stirred(500);
        let bytes = Snapshot::write(&original).unwrap();
        let restored = Snapshot::read(&bytes).unwrap();
        assert_eq!(restored.state_hash(), original.state_hash());
        assert_eq!(restored, original, "some field is missing from the format");
    }

    #[test]
    fn a_restored_world_runs_on_identically() {
        // The property that matters: not just that the bytes match, but that the future
        // does. This is what makes networking and long-run checkpointing possible later.
        let mut uninterrupted = stirred(200);
        let mut resumed = Snapshot::read(&Snapshot::write(&uninterrupted).unwrap()).unwrap();
        for tick in 0..500 {
            uninterrupted.step();
            resumed.step();
            assert_eq!(
                resumed.state_hash(),
                uninterrupted.state_hash(),
                "diverged {tick} ticks after resuming"
            );
        }
    }

    #[test]
    fn impulses_and_the_ledger_survive_too() {
        let mut w = World::new(Scenario::stress(16, 16)).unwrap();
        w.inject_impulse(3, 3, 700, -400);
        w.ledger_mut().absorb(12_345);
        w.ledger_mut().dissipate(2_345);
        let restored = Snapshot::read(&Snapshot::write(&w).unwrap()).unwrap();
        let (ix, iy) = restored.impulses();
        let i = restored.substrate().index(3, 3);
        assert_eq!((ix[i], iy[i]), (700, -400));
        // The world starts with a stored-energy baseline of its own, so what matters is that
        // the transactions survived, not that the totals are the numbers just passed in.
        assert_eq!(restored.ledger().energy_in(), w.ledger().energy_in());
        assert_eq!(restored.ledger().energy_out(), 2_345);
        assert_eq!(
            restored.ledger().energy_stored(),
            w.ledger().energy_stored()
        );
        restored.check_invariants().unwrap();
    }

    #[test]
    fn a_foreign_file_is_refused_rather_than_misread() {
        assert_eq!(
            Snapshot::read(b"not a snapshot at all").unwrap_err(),
            SnapshotError::NotASnapshot
        );
        assert!(matches!(
            Snapshot::read(b"").unwrap_err(),
            SnapshotError::Truncated { .. }
        ));
    }

    #[test]
    fn a_foreign_isa_version_is_refused() {
        let mut bytes = Snapshot::write(&stirred(1)).unwrap();
        // The ISA stamp sits right after the magic and the format version.
        let at = MAGIC.len() + 2;
        bytes[at] = 99;
        bytes[at + 1] = 0;
        assert_eq!(
            Snapshot::read(&bytes).unwrap_err(),
            SnapshotError::IsaMismatch {
                found: 99,
                expected: ISA_VERSION
            }
        );
    }

    #[test]
    fn a_foreign_format_version_is_refused() {
        // Derived from the current version rather than written out, because a literal here
        // silently becomes the *current* version the next time the format changes — which is
        // exactly what happened at version 7, where this test started asserting that a
        // perfectly good snapshot was refused.
        let foreign = FORMAT_VERSION.wrapping_add(1);
        let mut bytes = Snapshot::write(&stirred(1)).unwrap();
        bytes[MAGIC.len()..MAGIC.len() + 2].copy_from_slice(&foreign.to_le_bytes());
        assert!(matches!(
            Snapshot::read(&bytes).unwrap_err(),
            SnapshotError::FormatVersion { found, .. } if found == foreign
        ));
    }

    #[test]
    fn truncation_at_any_point_is_an_error_and_never_a_panic() {
        let bytes = Snapshot::write(&stirred(3)).unwrap();
        for cut in 0..bytes.len() {
            // Every prefix must be rejected cleanly. A save interrupted by a full disk is a
            // thing that happens, and it must not take the process down when it is opened.
            let _ = Snapshot::read(&bytes[..cut]);
        }
        assert!(Snapshot::read(&bytes).is_ok());
    }

    #[test]
    fn corruption_is_an_error_and_never_a_panic() {
        let good = Snapshot::write(&stirred(2)).unwrap();
        for byte in 0..good.len().min(4096) {
            let mut bytes = good.clone();
            bytes[byte] ^= 0xFF;
            let _ = Snapshot::read(&bytes);
        }
    }

    #[test]
    fn a_length_field_cannot_make_it_allocate_the_universe() {
        let mut bytes = Snapshot::write(&stirred(1)).unwrap();
        // Find the first chemical plane's length prefix and claim it is enormous.
        let header = MAGIC.len() + 4;
        let mut r = Reader::new(&bytes);
        r.take(header).unwrap();
        let scenario_len = r.u64().unwrap() as usize;
        let at = header + 8 + scenario_len + 8 + 4 + 4;
        bytes[at..at + 8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(matches!(
            Snapshot::read(&bytes).unwrap_err(),
            SnapshotError::Corrupt(_)
        ));
    }
}
