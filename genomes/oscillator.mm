; oscillator.mm — the ancestor, with a clock.
;
; Catalogue slot 13 is the one M3 organelle no shipped genome ever built, so nothing had ever
; priced it and nothing had ever read it. This is the genome that does both. It is the ancestor
; with two changes: it builds an oscillator, and it divides only on the beat.
;
; Written as `hoarder.mm` and `mutator.mm` are — one gene different from `ancestor.mm`, so a
; race between them measures the clock rather than two unrelated cells.
;
; ---------------------------------------------------------------------------------------------
; What the organelle actually reports, which is not what SPEC §6.2 says
;
; §6.2 gives the oscillator `period` in and `phase` out, and `sensing::oscillator_phase` does
; compute a proper triangle wave: it rises from 0 to `Q10_ONE` over half the period and falls
; back. But the reading is handed to `sensing.rs:224`'s `visible`, which is
; `sat_i16(q / Q10_ONE)` — and a value that is *already* normalised to 0..Q10_ONE comes out of
; that as 0 for every value except the single tick where it is exactly Q10_ONE.
;
; So what a genome can actually read is **a one-tick pulse once per period**, and on reading 1
; its inverse: 1 for all but one tick, 0 on the beat. That is a usable clock and it is not a
; phase. A genome cannot ask how far through its cycle it is, which is what the spec offers.
;
; This genome is written against the behaviour rather than the specification, deliberately —
; a fixture that assumes the documented reading would fail for a reason that has nothing to do
; with the clock. When `visible` is corrected for this reading, `#divide`'s guard below is the
; line that has to change, and it should become a threshold rather than a test against zero.
;
; ---------------------------------------------------------------------------------------------
; What the clock drives, and what it deliberately does not
;
; It gates the surplus-sugar dump: a batch purge once per period instead of a continuous bleed.
; Peroxide is still dumped every tick and division is not gated at all, and both of those are
; deliberate — the first because a cell that excretes its peroxide once per period poisons
; itself, and the second because the first version of this genome did gate division and it cost
; exactly what it should have.
;
; That is worth recording rather than quietly fixing. Gating `#divide` on the pulse took the
; cell to **3 cells in 71 of its own expression cycles** against a bar of 4, because the VM's
; instruction pointer persists across ticks: the driver reaches `#divide` once per expression
; cycle, not once per tick, so the pulse has to be high at that instant. The chance of that is
; roughly one in `period` however the period is chosen, so gating anything on the beat costs a
; factor of the period. A clock in this engine is cheap to read and expensive to obey.
;
; Whether batch purging pays is not claimed and has not been measured. `oscillator_phase`
; offsets by the cell's own key — "so a clonal population does not beat in lockstep for reasons
; that have nothing to do with coordination" — so clones purge out of step with one another,
; which is the interesting property if any of this has one.
;
; Chemical indices as `ancestor.mm`:
;   4  carbon      structural, what a body is built out of
;   8  sugar       the energy substrate
;   11 carbon_dioxide   the waste
;   14 oxygen      the oxidant every pathway breathes
;
; Organelle slots:
;   0  membrane   (always)
;   1  nucleus
;   2  mitochondrion
;   3  chloroplast
;   4  oscillator, period 64

        EXPRESS #build
        EXPRESS #tune
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     40              ; param
        IMM     1               ; nucleus
        IMM     1               ; slot 1
        BUILD
        IMM     60
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        IMM     32              ; param
        IMM     13              ; oscillator
        IMM     4               ; slot 4
        BUILD
        RET

; ---------------------------------------------------------------- set the period
;
; Control input 0 is the period in ticks, and `oscillator_phase` floors it at 2. Sixty-four is
; chosen against the division cost rather than arbitrarily: the ancestor's own copy loop takes
; roughly twenty-eight ticks for its 227 bytes, so a period much shorter than that would fire
; again before the previous division finished and the gate would do nothing.

        GENE    #tune
        IMM     64              ; period, in ticks
        ZERO                    ; control input 0
        IMM     4               ; slot 4
        OSET
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, the input to photosynthesis
        EAT
        DROP
        IMM     20
        IMM     14              ; and its oxidant
        EAT
        DROP
        IMM     16
        IMM     4               ; carbon, to build a body out of
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house
;
; Peroxide out every tick, exactly as `ancestor.mm` — not gated, because a cell that excretes
; once per period poisons itself and this would become a test of the peroxide economy.
;
; The sugar is the part the clock owns. `ancestor.mm` bleeds eight units every tick to keep the
; cytoplasm open; this one holds it and purges on the beat. The read is the whole point of the
; genome: an `OGET` against slot 4, every expression cycle, which is the cost the benchmark
; needs priced.

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out — every tick, ungated
        EMIT
        DROP
        ZERO                    ; reading 0: the pulse, 1 on the beat and 0 otherwise
        IMM     4               ; slot 4, the oscillator
        OGET
        JMPZ    hold            ; not on the beat — keep the sugar
        IMM     64              ; a period's worth at once, rather than eight every tick
        IMM     8               ; surplus sugar back to the water
        EMIT
        DROP
hold:
        RET

; ---------------------------------------------------------------- divide
;
; Ungated, and identical to `ancestor.mm`. See the header for what happened when it was not.

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     100
        CMP                     ; -1 if poor, 0 or 1 if it can afford to divide
        ONE
        ADD                     ; 0 if poor, non-zero if not
        JMPZ    lean
        GLEN
        SETLN
        GLEN
        BUD
        DROP
        ZERO
        SETPA
        ZERO
        SETPB
loop:
        COPYB
        LOOPLN  loop
        SPLIT
lean:
        RET
