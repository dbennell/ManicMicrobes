; marble.mm — the ancestor, with a wall.
;
; Everything on a slide is drawn as a tessellation: cells flattened into polygons, sharing walls,
; no gaps. That is `slide::area_swell` and it is right for a moss leaf. A smear of yeast pressed
; just as hard stays obstinately round, and this is the genome that produces the second picture.
;
; It is `ancestor.mm` with four extra instructions. Nothing else changes, which is the point —
; being a marble is not a different way of living, it is the same way of living with a different
; investment.
;
; ---------------------------------------------------------------- what makes a marble
;
; Two things, and this genome only has to do the first because it already does the second by
; accident.
;
; **It must not be glued to anything.** A cell joined to its neighbours is part of a body, and a
; body shares its walls however rigid its cells are. This genome never calls `JOIN`, which is
; also true of `ancestor.mm` — so every slide ever run has been a heap of unjoined cells drawn as
; though it were a tissue, which was the thing worth fixing.
;
; **It must be firm**, which is wall times turgor. The turgor is already there: a working cell
; carries several interior capacities of solute and the term saturates at one, so every ancestor
; has been fully turgid all along. What none of them has is a wall — and until the membrane
; became something `BUILD` could reach, none of them could have had one, because a daughter is
; born with her mother's membrane parameter and nothing anywhere could change that number.
;
; ---------------------------------------------------------------- why 200, and not 255
;
; Because a marble you cannot fill a slide with is not the picture.
;
; The obvious version of this genome takes the wall to the catalogue's maximum. It cannot: a
; membrane costs `8 + param/4` units of structural matter, so 255 costs 71.75, and
; `BASE_INTERIOR_CAPACITY` is 64. **A cell cannot hold enough carbon at once to build its own
; maximum wall.** The way past that is a vacuole, which raises interior capacity by its own
; `param` — and would be the first thing in this engine ever to need one, since `gradient_probe`
; reports zero vacuoles in every population it has ever measured.
;
; It works, and it is the wrong trade. Measured on `soup.ron` from sixteen founders at twenty
; thousand ticks:
;
;   genome                     pop   rigidity   swell   coverage
;   ancestor.mm              1 113       0.09   1.229       105%
;   this, wall 200             666       0.68   1.095       105%
;   wall 255 + a vacuole       436       1.00   1.015        55%
;
; where 1.000 is a perfect sphere and 1.237 is a cell inflated until its clipped outline keeps
; the area it has. The third row is **rounder and half as dense**: the extra wall costs enough
; upkeep that the population settles at 436, which covers barely half the slide, and a scatter of
; very round cells is not what a smear of yeast looks like. The second row is wall to wall *and*
; visibly separate, which is.
;
; So the wall stops at 200, a little under the ceiling `BASE_INTERIOR_CAPACITY` sets, and there is
; no vacuole. The rounder strain is worth knowing about and is four instructions away — add a
; vacuole at 40 in slot 4 and take the wall to 255 — but it is a different picture and a worse one.
;
; ---------------------------------------------------------------- what it costs
;
; **Four cells in ten**, against the ancestor, and paid every tick forever:
;
;   structural matter in the wall   14 units -> 58, a little over four times
;   membrane upkeep                 0.078 -> 0.42 energy a tick, five and a half times
;   total loadout upkeep            ~0.41 -> ~0.75 energy a tick
;
; The slide still fills — coverage is the ancestor's 105% — because the cells are *larger*, at a
; median radius of 1.38 against 1.12. `membrane.param` is also the growth target, so a thicker
; wall is also a bigger cell, and here that happens to compensate for there being fewer of them.
;
; That conflation is worth knowing rather than relying on. Real yeast are small and round and
; moss cells are large and tessellated, which is the other way round; separating wall thickness
; from target size would need a second membrane control, and `control[1]` is already the growth
; target. See the growth block in `metabolism.rs`.
;
; One thing this genome is *not* robust to: brighter light. At `Uniform(intensity: 2048)` and
; above it goes extinct, where the ancestor does not. Unexplained, and left as a finding rather
; than tuned away — the likeliest suspect is that faster photosynthesis fills the cytoplasm with
; substrate it cannot burn or excrete fast enough, and the quadratic turgor charge does the rest.
; `#grow` returns eight units of sugar a cycle, which was calibrated for the ancestor's income.
;
; Chemical indices and organelle slots are `ancestor.mm`'s, unchanged.

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body
;
; The wall goes **last**, deliberately. `BUILD` is refused when the cytoplasm cannot pay, and a
; membrane at 200 wants 58 of the 64 units a cell can hold — so a genome that asks for the wall
; first spends everything on it and has nothing left for the nucleus it needs to reproduce. Asked
; for last, the cell equips itself, and then thickens once it can afford to.
;
; Slot 0 ignores the type operand. It is always the membrane and cannot be retyped; only its size
; is a question, and `TEAR` on it is still refused.

        GENE    #build
        IMM     40              ; nucleus
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
        IMM     200             ; the wall. 58 units of carbon, once, and 0.42 a tick to carry
        ZERO                    ; type ignored on slot 0
        ZERO                    ; slot 0 — the membrane
        BUILD
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide
        EAT
        DROP
        IMM     20
        IMM     14              ; oxygen
        EAT
        DROP
        IMM     64
        IMM     4               ; carbon, and more of it: there is a wall to pay for
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house

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
;
; The ancestor's, with a higher bar. A daughter inherits her mother's membrane parameter, so she
; is born owing 100 units of growth before she is the size her own wall says she should be, and
; she has to build nothing to get there — the wall is already hers. What she cannot do is divide
; while poor, and 100 was calibrated for a cell paying a fifth of this one's upkeep.

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     240
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
