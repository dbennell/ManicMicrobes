//! Intents: what a cell asked the world for (SPEC §12).
//!
//! # Why cells do not act directly
//!
//! The tick order runs **execute** in parallel over cells and **resolve** afterwards, in
//! order. During execute no cell writes shared state; it emits a list of intents. Resolve
//! applies them sorted by cell id, so a contested resource — a chemical two cells both ate —
//! is allocated the same way on every machine, at every thread count, forever.
//!
//! That separation is the whole of I1 and I6 on the cell side. If a cell could eat directly,
//! the outcome would depend on which thread reached the square first, and the failure would
//! reproduce only sometimes.
//!
//! # What a cell is told, and what it gets
//!
//! `EAT` has to push a result immediately — a genome branches on it — but what is actually
//! available is not known until resolve. So a cell is told what the square held *at the start
//! of the tick*, bounded by what it asked for and by what it can hold. Resolve then delivers
//! at most that, and less if somebody with a lower id got there first.
//!
//! A cell can therefore be told more than it receives, and only when it is in competition. It
//! is the honest version of the situation: an organism finds out how much food is in front of
//! it, commits, and discovers afterwards that something else was eating too. Nothing about
//! matter conservation depends on the estimate — resolve moves what it moves.
//!
//! # Storage
//!
//! Intents live in one flat buffer with a fixed stride per cell, not a `Vec` per cell. At two
//! hundred thousand cells and an intent per instruction, per-cell allocation would dominate
//! the tick; and a flat buffer indexed by slot is already in the order resolve wants.

use crate::cell::CellId;

/// Most intents one cell can emit in one tick.
///
/// An instruction emits at most one, so this only has to match the largest `instr_per_tick`
/// a scenario will sensibly use. Beyond it a cell's later intents are dropped, which costs it
/// the tail of its tick and cannot corrupt anything.
pub const MAX_INTENTS_PER_TICK: usize = 32;

/// One thing a cell asked to do.
///
/// Deliberately small and `Copy`: there is one of these per instruction per cell per tick.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    /// Take from the local fluid square. `promised` is what the cell was told it would get.
    Eat { chem: u8, promised: i32 },
    /// Give to the local fluid square.
    Emit { chem: u8, amount: i32 },
    /// Begin building an organelle. Replaces whatever is in the slot.
    Build { slot: u8, kind: u8, param: u8 },
    /// Dismantle, recovering part of the matter.
    Tear { slot: u8 },
    /// Write an organelle control input.
    Control { slot: u8, index: u8, value: i16 },
    /// Allocate a daughter genome buffer.
    Bud { size: u16 },
    /// Write one byte into the daughter buffer.
    CopyByte { dst: u16, src: u8 },
    /// Finalise division.
    Split,
    /// Set the receptor key (SPEC §8.2).
    SetKey { key: u8 },
}

/// Per-cell intent lists for one tick, in one flat buffer.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct IntentBuffer {
    intents: Vec<Intent>,
    counts: Vec<u8>,
    /// Intents a cell tried to emit past [`MAX_INTENTS_PER_TICK`]. Instrumentation: a run
    /// where this is routinely non-zero has `instr_per_tick` set higher than the buffer
    /// allows, and cells are silently losing the end of their tick.
    dropped: u64,
}

impl IntentBuffer {
    #[must_use]
    pub fn new() -> IntentBuffer {
        IntentBuffer::default()
    }

    /// Size the buffer for an arena of `slots`, clearing every list.
    pub fn begin_tick(&mut self, slots: usize) {
        let want = slots.saturating_mul(MAX_INTENTS_PER_TICK);
        if self.intents.len() != want {
            self.intents.resize(want, Intent::Split);
        }
        if self.counts.len() != slots {
            self.counts.resize(slots, 0);
        }
        self.counts.fill(0);
    }

    /// Record an intent for the cell in `slot`.
    #[inline]
    pub fn push(&mut self, slot: usize, intent: Intent) {
        let Some(count) = self.counts.get_mut(slot) else {
            return;
        };
        if *count as usize >= MAX_INTENTS_PER_TICK {
            self.dropped = self.dropped.saturating_add(1);
            return;
        }
        let at = slot * MAX_INTENTS_PER_TICK + *count as usize;
        if let Some(cell) = self.intents.get_mut(at) {
            *cell = intent;
            *count = count.saturating_add(1);
        }
    }

    /// What the cell in `slot` asked for, in the order it asked.
    #[inline]
    #[must_use]
    pub fn for_slot(&self, slot: usize) -> &[Intent] {
        let n = self.counts.get(slot).copied().unwrap_or(0) as usize;
        let base = slot * MAX_INTENTS_PER_TICK;
        self.intents.get(base..base + n).unwrap_or(&[])
    }

    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Total intents this tick, for metrics.
    #[must_use]
    pub fn len(&self) -> usize {
        self.counts.iter().map(|c| *c as usize).sum()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// A cell's own view of the world during execute, taken before anything moved.
///
/// Read-only by construction: this is what makes "no cell writes shared state" enforceable
/// rather than a convention. Everything a genome can observe about its surroundings during
/// its own execution comes through here.
#[derive(Clone, Copy, Debug)]
pub struct SenseView {
    /// The square the cell is standing on.
    pub square: usize,
    /// Incident light there, `Q10`.
    pub light: i32,
}

/// Deaths and births decided during resolve, applied during bookkeeping.
///
/// Kept separate so that the arena is not mutated while it is being iterated, and so that a
/// cell which both divides and dies in the same tick is handled once rather than twice.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Pending {
    pub deaths: Vec<CellId>,
    pub births: Vec<PendingBirth>,
}

/// A daughter waiting to be added to the arena.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PendingBirth {
    pub parent: CellId,
    /// The daughter's genome, before interning.
    pub genome: Vec<u8>,
    /// Matter and energy the parent handed over.
    pub mass: i32,
    pub energy: i32,
    /// Half the parent's interior chemistry, moved rather than copied — division splits what
    /// a cell has, it does not duplicate it (I4).
    pub interior: Vec<i32>,
    pub x: i32,
    pub y: i32,
    pub membrane: u8,
    pub key: u8,
    /// Inherited from the parent. M5 replaces this with real speciation.
    pub species: u32,
}

impl Pending {
    pub fn clear(&mut self) {
        self.deaths.clear();
        self.births.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intents_come_back_in_the_order_they_were_asked_for() {
        // Resolve replays a cell's tick, so the order has to be the order the genome
        // executed in — a cell that emits then eats is not the same as one that eats then
        // emits.
        let mut b = IntentBuffer::new();
        b.begin_tick(4);
        b.push(
            1,
            Intent::Emit {
                chem: 3,
                amount: 10,
            },
        );
        b.push(
            1,
            Intent::Eat {
                chem: 3,
                promised: 20,
            },
        );
        b.push(1, Intent::Split);
        assert_eq!(
            b.for_slot(1),
            &[
                Intent::Emit {
                    chem: 3,
                    amount: 10
                },
                Intent::Eat {
                    chem: 3,
                    promised: 20
                },
                Intent::Split,
            ]
        );
        assert!(b.for_slot(0).is_empty());
        assert_eq!(b.len(), 3);
    }

    #[test]
    fn cells_do_not_see_each_others_intents() {
        let mut b = IntentBuffer::new();
        b.begin_tick(3);
        b.push(0, Intent::Split);
        b.push(2, Intent::Tear { slot: 4 });
        assert_eq!(b.for_slot(0), &[Intent::Split]);
        assert!(b.for_slot(1).is_empty());
        assert_eq!(b.for_slot(2), &[Intent::Tear { slot: 4 }]);
    }

    #[test]
    fn a_new_tick_clears_the_old_ones() {
        // Otherwise a cell would re-execute last tick's intents forever.
        let mut b = IntentBuffer::new();
        b.begin_tick(2);
        b.push(0, Intent::Split);
        b.begin_tick(2);
        assert!(b.for_slot(0).is_empty());
    }

    #[test]
    fn overflowing_a_cells_list_costs_it_the_tail_and_nothing_else() {
        let mut b = IntentBuffer::new();
        b.begin_tick(2);
        for i in 0..MAX_INTENTS_PER_TICK + 10 {
            b.push(
                0,
                Intent::Emit {
                    chem: 0,
                    amount: i as i32,
                },
            );
        }
        b.push(1, Intent::Split);
        assert_eq!(b.for_slot(0).len(), MAX_INTENTS_PER_TICK);
        assert_eq!(b.dropped(), 10);
        assert_eq!(
            b.for_slot(1),
            &[Intent::Split],
            "the neighbour is untouched"
        );
    }

    #[test]
    fn an_out_of_range_slot_is_ignored() {
        let mut b = IntentBuffer::new();
        b.begin_tick(1);
        b.push(99, Intent::Split);
        assert!(b.for_slot(99).is_empty());
        assert_eq!(b.len(), 0);
    }

    #[test]
    fn growing_the_arena_keeps_existing_lists_addressable() {
        let mut b = IntentBuffer::new();
        b.begin_tick(2);
        b.push(1, Intent::Split);
        assert_eq!(b.for_slot(1).len(), 1);
        b.begin_tick(64);
        b.push(63, Intent::Split);
        assert_eq!(b.for_slot(63), &[Intent::Split]);
    }
}
