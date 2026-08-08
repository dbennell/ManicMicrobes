; marble.mm — the ancestor, with a wall.
;
; Everything on a slide is drawn as a tessellation: cells flattened into polygons, sharing walls,
; no gaps. That is `slide::area_swell` and it is right for a moss leaf. A smear of yeast pressed
; just as hard stays obstinately round, and this is the genome that produces the second picture.
;
; It is `ancestor.mm` with eight extra instructions. Nothing else changes, which is the point —
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
; ---------------------------------------------------------------- why it needs a vacuole
;
; A membrane costs `8 + param/4` units of structural matter, so the catalogue's maximum of 255
; costs 71.75 — and `BASE_INTERIOR_CAPACITY` is 64. **A cell cannot hold enough carbon at once to
; build its own maximum wall.** Without help the ceiling is a param of about 224.
;
; The way past it is a vacuole, which raises interior capacity by its own `param`. This is the
; first thing in the engine that has ever *needed* one: `gradient_probe` reports zero vacuoles in
; every population it has ever measured, because until now a vacuole's only effect was to reduce
; a bill.
;
; It cuts both ways and the arithmetic is close. A vacuole also takes its own `param` back out of
; `osmotic_load`, so it lowers the turgor at the same time as it raises the ceiling — and turgor
; is the other half of rigidity. At 40 the trade comes out ahead because the extra capacity lets
; the cell hold more solute overall than the vacuole hides.
;
; Measured, that is the difference between most of the way and all of it:
;
;   variant                       membrane   rigidity   swell
;   no vacuole, wall at 200          200       0.68     1.095
;   vacuole at 40, wall at 255       255       1.00     1.015
;
; against `ancestor.mm`'s 1.229 and a perfect sphere's 1.000. The second is the picture.
;
; ---------------------------------------------------------------- what it costs
;
; On `soup.ron` from sixteen founders, at twenty thousand ticks:
;
;   genome         population   membrane   rigidity   swell   radius
;   ancestor.mm         1 113         24       0.09    1.229     1.12
;   marble.mm             436        255       1.00    1.015     1.00
;
; **Under half the carrying capacity**, which is the price of the picture and is paid every tick
; forever: 71.75 units of structural matter in the wall against the ancestor's 14, 0.53 energy a
; tick to carry it against 0.078, and a vacuole on top of that.
;
; One thing that did *not* happen, and it was the thing to watch for. `membrane.param` is also
; the growth target — a cell grows towards `q10(param)` — so the obvious worry was that a marble
; would be a giant, which is backwards from life, where yeast are small and round and moss cells
; are large and tessellated. Measured, the median radius is **1.00 against the ancestor's 1.12**:
; the marbles are *smaller*, because they divide long before they reach a target of 255 and are
; matter-limited well below it. The conflation is real and is still worth separating one day, but
; it does not bite here.
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
        IMM     40
        IMM     4               ; a vacuole, and the first thing in this engine that ever needed one
        IMM     4
        BUILD
        IMM     255             ; the wall, at the catalogue's maximum
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
        IMM     100
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
