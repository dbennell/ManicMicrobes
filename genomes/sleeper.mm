; sleeper.mm — the ancestor, plus the sense to stop paying for machinery it cannot use.
;
; This is `ancestor.mm` with one gene added and one number changed, deliberately, so that any
; difference between them is the gene. Same body, same feeding, same division threshold.
;
; ---------------------------------------------------------------- read this first: it loses
;
; **It is beaten by the plain ancestor in every world it has been run in**, and it is kept for the
; same reason `hoarder.mm` is: that is the measurement, not the hope. It is here as the first
; genome to use ISA 14's throttled upkeep at all, and as the record of what happened when it did.
;
;   world                    ancestor   sleeper   first dormancy
;   the arena, head to head        567       497   —
;   the_thicket                  1,156     1,053   tick 1,000
;   the_lean_water               2,193     1,950   tick 500
;
; The middle column is not the interesting one. **The gate fires** — `Occurrence::Dormancy` is
; recorded at tick 1,000 and 500, and never for the ancestor — so this is not a genome paying for
; a branch it never takes. It idles, it saves what it should save, and it still loses.
;
; What it is losing to is **chatter.** The gate is instantaneous and carbon dioxide is
; intermittent: the cell reads zero on a tick the water has not refilled yet, shuts the
; chloroplast, and is still shut on the tick the CO2 arrives. Three quarters of a chloroplast's
; upkeep is 0.064 a tick and one tick of missed fixation is worth more than that.
;
; So the finding, which is worth more than the genome: **idling needs hysteresis, or a signal that
; persists.** A cell should sleep through something long — a night, a season — and the only
; persistent scarcity in the library is the day/night cycle, which the next section shows no
; genome can see. Between those two facts, dormancy currently has no world to pay in. That is a
; statement about the library and the sensor, not about the mechanism, which `tests/dormancy.rs`
; measures at +39% on lifespan when the dark actually lasts.
;
; The obvious next version, for whoever picks this up: hold the throttle shut for N ticks once
; closed, or gate on an `OSCILLATOR` rather than on a reading. Neither needs an engine change.
;
; ---------------------------------------------------------------- what it is for
;
; `OrganelleSpec::upkeep_throttled` (ISA 14) made three quarters of a chloroplast's and a
; mitochondrion's upkeep follow `control[0]`. Before it, `OSET`ting an engine to zero lowered
; what the organelle *produced* and not a unit of what it cost, so the only reason a genome ever
; had to close a throttle was to stop consuming a substrate — and none of the shipped genomes
; ever did. Now closing one is worth something on its own.
;
; `docs/ECONOMY.md` §17 has the numbers. An idle cell's bill is 96% organelle upkeep, of which
; about a third is reachable by a genome; the rest is the nucleus, the membrane and a basal
; quarter, none of which a cell can surrender and live.
;
; ---------------------------------------------------------------- what it watches, and what it
;                                                                  cannot
;
; **Not the light**, and that is a finding rather than a choice. A photosensor's ambient reading
; is `sat_i16(light / Q10_ONE)`, and `the_short_night.ron` — the one shipped day/night world —
; runs between 128 and 1024. Measured in `tests/light_resolution.rs`: twenty distinct values of
; the field over a full cycle, and **one** distinct reading of it, which is 0. Not zero except at
; noon; the triangle peaks at 1023, one short of the divisor, so the reading never reaches 1 at
; all. A genome on that slide has no bits of information about the light whatsoever.
;
; That is the same defect the pH sensor's own note in `sensing.rs` describes and fixes for itself
; — "divided down, the whole interesting range of a slide would be the integer 7" — and nothing
; has fixed it for light. Until something does, a light-gated sleeper cannot be written, only
; intended, and the fix is genome-observable and so another ISA bump.
;
; So this one watches its **inputs**, which are reported at a useful resolution, and the strategy
; that falls out is better anyway because it is not about the sky:
;
;   * a chloroplast with no carbon dioxide in the cell can fix nothing, whatever the light;
;   * a mitochondrion with no sugar in the cell can burn nothing, whatever it is asked for.
;
; **The second one was written, measured and removed, and that is the most useful thing in this
; file.** Gating the mitochondrion on held sugar killed the lineage outright — extinct at 14,649
; ticks against an ancestor still at 746 — because `#grow` below dumps eight units of surplus sugar
; into the water every tick, so the cell's holding rounds to zero most ticks and the engine spends
; its life shut. The controls, run head to head against `ancestor.mm` over 20,000 ticks:
;
;   both genes          extinct at 17,965
;   #burn alone         extinct at 14,649
;   #sun alone          501 against 551 — a tie
;   nucleus 48 alone    530 against 509 — a tie, so the bigger nucleus is not the cost
;
; It is not a bug in the mechanism; it is the mechanism working. The word that stops a
; mitochondrion's bill stops its respiration by the same fraction, so a genome that idles an engine
; it actually needed has idled its income, and no guard in the engine will save it from that. What
; the measurement says is narrower and more useful: **a cell cannot gate an engine on a substrate
; it is simultaneously excreting.** A version that keeps a tick's fuel back — `#grow` is where that
; would go — could carry this gene. This one does not, so it does not carry it.
;
; What is left is the first gate, and it is a state a crowd puts itself into. `docs/ECONOMY.md` §16 built `the_thicket` precisely so
; that a pack can deplete the carbon inside itself faster than the water refills it, and this is
; the genome that has something to do about it. Dormancy here is a response to *local scarcity*,
; not to nightfall, and a slide of these should show cells idling in the middle of a crowd while
; the ones at its edge keep working.
;
; A closed organelle is still owned, still built, and still costs its basal quarter, so this is
; not a way to carry machinery for free — it is a way to stop paying the working rate for
; machinery that is not working.
;
; ---------------------------------------------------------------- the readings it uses
;
; `OGET ( idx slot -- v )` on slot 0, the membrane, is the cell's self-sensor. Readings 0–4 are
; mass, energy, age, radius and damage; from 5 they are the interior chemicals, so reading `5 + c`
; is how much of chemical `c` this cell is holding.
;
;   13   5 + 8    sugar, what the mitochondrion burns
;   16   5 + 11   carbon dioxide, what the chloroplast fixes
;
; Both are `interior / Q10_ONE`, so a cell holding less than one whole unit reads zero — which is
; the right threshold for this and is the reason the light reading is the wrong one for it.
;
; ---------------------------------------------------------------- the number that changed
;
; The nucleus is 48 rather than the ancestor's 40. Nucleus `param` is genome capacity at 8 bytes a
; unit, so 40 holds 320 bytes and this genome is longer than that. A daughter truncated at 320
; would lose its last gene silently — no error, no death, just a lineage that quietly stops being
; this organism. `engulfer.mm` learned that one the same way.
;
; It was left at 48 after `#burn` was removed rather than trimmed back, because the control above
; measured what it costs and the answer is nothing: an ancestor with nothing changed but a
; 48-unit nucleus ties with one at 40. Headroom for a descendant is worth more than a number that
; does not show up.

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #sun
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     48              ; nucleus: 384 bytes, enough to carry the two extra genes
        IMM     1
        IMM     1
        BUILD
        IMM     60
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
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

; ---------------------------------------------------------------- the chloroplast's throttle
;
; Shut when the cell holds no carbon dioxide. Note the order this runs in: `#feed` has already
; had its turn this tick, so a cell reading zero here is one that tried to draw CO2 and found
; none in its square — a crowded cell, or one in lean water. It is not a cell that merely has
; not asked yet.

        GENE    #sun
        IMM     16              ; reading 5 + 11 — carbon dioxide held
        ZERO                    ; slot 0, the membrane
        OGET
        JMPZ    shut
        IMM     255             ; there is something to fix: run it
        IMM     2
        SHL                     ; 1020, just under the Q10 clamp
        ZERO                    ; control 0 — the throttle
        IMM     3               ; the chloroplast
        OSET
        RET
shut:
        ZERO                    ; nothing to fix, so nothing to pay the working rate for
        ZERO
        IMM     3
        OSET
        RET

; ---------------------------------------------------------------- keep house
;
; Verbatim from the ancestor, including the name: respiration exhales peroxide and a cell that
; lets it build up ages and dies.
;
; **The `EMIT 8` of sugar on the end is what killed `#burn`.** A cell that has just dumped its
; surplus holds less than one whole unit, reads zero, and would idle the engine that was about to
; make more. Anything that wants to gate on held fuel has to change this line first.

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out
        EMIT
        DROP
        IMM     8
        IMM     8               ; surplus sugar back to the water
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     100
        CMP
        ONE
        ADD
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
