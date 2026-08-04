; stalker.mm — a predator that finds its dinner by the light dinner cannot help giving off.
;
; Three genomes in `genomes/` are the same lineage at three stages of discovery, and this is the
; third. `hunter.mm` carries a spike and no stomach, and pays for its neighbours' lunch.
; `predator.mm` adds the stomach, and killed its own daughters until its weapon was turned down
; to a sixtyfourth. `sentinel.mm` adds a badge and a touch sensor, and can tell its children from
; its dinner — so it carries the weapon at full extension again. All three of them are blind.
;
; This one has eyes. A cell's emission is accumulated from what it actually paid as it paid it,
; so a fed, metabolising cell is a warm one and there is nothing it can do about that. The
; photosensor reads the metabolic band as a magnitude and a gradient, and this genome does the
; one thing `drifter.mm` never does: it connects the sensor to the thrusters.
;
;   #hunt is four instructions. Thrust along x is the glow's gradient along x; thrust along y is
;   its gradient along y. That is the whole of the behaviour.
;
; Measured on the same body with and without those four instructions, starting five squares from
; a crowd of nine over a hundred and twenty ticks: blind ends **eighteen** squares away, steered
; ends **one**. Same cilia, same power, same everything else.
;
; **The signature is a homing sense and not a searching one.** `em_range` is six squares. Started
; twenty squares out, this genome reads a gradient of exactly zero and sits perfectly still while
; the blind version at least wanders — which is the honest answer and the thing to know before
; expecting it to cross a slide. It finds what is already near. To find what is far it would need
; to search first, and nothing here does.

        EXPRESS #build
        EXPRESS #eyes
        EXPRESS #fins
        EXPRESS #dress
        EXPRESS #arm
        EXPRESS #digest
        EXPRESS #watch
        EXPRESS #hunt
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- the body

        GENE    #build
        IMM     80              ; nucleus: 640 bytes
        IMM     1
        IMM     1
        BUILD
        IMM     55
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        RET

; ---------------------------------------------------------------- eyes
;
; Both sensors are small — 24 rather than the 40 they started at — and the cilia are 60 rather
; than 80. Every one of them is upkeep every tick against a lineage that was already the dearest
; in `genomes/`, and at the larger sizes this reached three descendants where four is the bar.
; What a sensor's `param` buys is not range, which is `em_range` and is the scenario's business;
; it is how much of the reading survives, and this genome needs a direction rather than a number.
;
; Slot 4 the photosensor — which reads ambient light on 0..2 and other cells' emission on 3..8,
; because a photosensor detects electromagnetic radiation and a cell's glow is the same physics
; as the sun's. Slot 7 the touch sensor, for the badge, which is `sentinel.mm`'s trick and the
; only reason the spike can be carried out.

        GENE    #eyes
        IMM     24
        IMM     8               ; photosensor
        IMM     4
        BUILD
        IMM     24
        IMM     9               ; touch sensor
        IMM     7
        BUILD
        RET

; ---------------------------------------------------------------- fins
;
; Two cilia on perpendicular mounts. A cilium's direction is `control[1]`, four bits of angle, so
; a mutation turns it a little rather than reversing it. Slot 6 at angle 0 pushes along +x and
; slot 8 at angle 12 along +y — which is what makes steering two independent scalars rather than
; an arctangent nobody can compute in this instruction set.

        GENE    #fins
        IMM     60
        IMM     6               ; cilium
        IMM     6
        BUILD
        IMM     60
        IMM     6               ; cilium
        IMM     8
        BUILD
        ZERO
        ONE                     ; control 1 — mount angle
        IMM     6
        OSET
        IMM     12
        ONE
        IMM     8
        OSET
        RET

; ---------------------------------------------------------------- colours

        GENE    #dress
        IMM     211
        SETBADGE
        RET

; ---------------------------------------------------------------- the weapon

        GENE    #arm
        IMM     80
        IMM     12              ; spike
        IMM     5
        BUILD
        RET

; ---------------------------------------------------------------- the stomach

        GENE    #digest
        IMM     70
        IMM     11              ; lysosome
        IMM     9
        BUILD
        IMM     255
        IMM     2
        SHL
        ZERO
        IMM     9
        OSET
        RET

; ---------------------------------------------------------------- friend or foe
;
; `sentinel.mm`'s, unchanged, including the test that is easy to leave out: a cell touching
; nobody reads a badge of zero, which differs from its own, so without the first branch a
; solitary cell arms permanently and pays the dearest upkeep in the catalogue to menace open
; water.

        GENE    #watch
        IMM     3
        IMM     7               ; the nearest one's badge
        OGET
        DUP
        JMPZ    kin             ; nobody in reach
        IMM     21
        ZERO                    ; my own badge
        OGET
        CMP
        JMPZ    kin
        IMM     128
        IMM     2
        SHL                     ; 512
        ZERO
        IMM     5
        OSET
        RET
kin:
        ZERO
        ZERO
        IMM     5
        OSET
        RET

; ---------------------------------------------------------------- hunt
;
; The four instructions. Everything else in this genome exists to make them affordable.

        GENE    #hunt
        IMM     7               ; metabolic glow, gradient along x
        IMM     4
        OGET
        ZERO                    ; control 0 — signed power
        IMM     6
        OSET
        IMM     8               ; gradient along y
        IMM     4
        OGET
        ZERO
        IMM     8
        OSET
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11
        EAT
        DROP
        IMM     20
        IMM     14
        EAT
        DROP
        IMM     16
        IMM     4
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out, or respiration poisons it
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET
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
