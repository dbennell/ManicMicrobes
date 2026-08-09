; drifter_blind.mm — the control: the same cell with nothing to see with.
;
; It held `RESERVED_A` — catalogue slot 14 — until that slot was filled by the holdfast, at
; which point the control quietly stopped being blind: it was building a working organelle,
; paying a third more upkeep for it than the reserved slot cost, and would have gripped a wall
; on any slide that had one. A control that acquires a capability is not a control. Moved to
; slot 15, which is the reserved one now, and this is what an ISA bump costs when a genome was
; using a slot *for* being empty.
;
; The motile-but-blind control for M3's chemotaxis experiment. Identical to `drifter.mm` byte
; for byte except that slot 7 holds a `RESERVED_B` organelle instead of a chemosensor: same
; length, same instruction count, same build cost, same upkeep, same everything — and no
; information about where the food is.
;
; That control is the whole experiment. A sighted population that ends up nearer its food than
; a blind one has evolved chemotaxis. A sighted population that ends up nearer its food than
; *nothing* has merely swum about, and half of swimming about is ending up somewhere.
;
; Slots:
;   0  membrane    (always)
;   1  nucleus
;   2  mitochondrion
;   3  chloroplast
;   7  chemosensor, tuned to chemical 11 — the same carbon dioxide it eats
;   6  cilium, mounted +x
;   8  cilium, mounted +y

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #swim
        EXPRESS #keep
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     64              ; nucleus: 8 bytes of genome per unit, and this genome is 344,
        IMM     1               ; so anything under 42 cannot copy itself at all
        IMM     1
        BUILD
        IMM     50
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     40
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        IMM     60
        IMM     15:4             ; RESERVED_B — built, paid for, and blind
        IMM     7
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     6
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     8
        BUILD
        RET

; ---------------------------------------------------------------- tune the sensor
;
; The chemosensor's first control input says which chemical it watches. Setting it is the one
; concession this ancestor makes to having a sensor at all — it is switched on and pointed at
; something, and then ignored.

        GENE    #keep
        IMM     11              ; watch carbon dioxide
        ZERO
        IMM     7
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
        IMM     255
        IMM     13              ; peroxide out, or it ages and dies
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- swim
;
; Full power on both cilia, every tick, on perpendicular mounts. The power is written to control
; input 0 and the mount angle to control input 1; a mutation that made either of them depend on
; `OGET 1 7` — the sensor's x gradient — would be chemotaxis.
;
; It does not. It writes constants.
;
; Both constants used to be wrong, and each was wrong in a way that made the experiment above it
; unwinnable rather than merely unwon.
;
; The power was `IMM 255`, and a control input is a `Q10` fraction of 1024, so this genome ran
; its engines at a quarter throttle under a comment claiming full power. A template value is a
; `u8` and `IMM` cannot push more than 255, so full power needs arithmetic — the idiom is
; `stalker.mm`'s, which shifts for its spike. At a quarter throttle two cilia made 158 `Q10`
; against the 192 a cell needed to out-push its own wake, so this genome travelled *backwards*:
; measured, 8.5 squares the wrong way in 600 ticks where it now goes 121 the right way. The
; engine side of that is fixed too — see `ECONOMY.md` §14.7 — but a quarter throttle is still a
; quarter throttle, and in a crowd it is the difference between half a square a tick and a
; fiftieth.
;
; The mounts were never written at all. `Organelle::finished` leaves `control[1]` at zero and
; zero is due east, so both cilia pushed the same way and the slot comments above — one +x, one
; +y — described a cell this genome did not build. A cell with one axis cannot steer, whatever
; it reads, so the four instructions M3 is waiting for had nowhere to go. `stalker.mm` says why
; the pair has to be perpendicular: it makes steering two independent scalars rather than an
; arctangent nobody can compute in this instruction set.

        GENE    #swim
        ONE
        IMM     10
        SHL                     ; 1024 — full power, which IMM alone cannot reach
        ZERO
        IMM     6
        OSET                    ; cilium 6, power
        ONE
        IMM     10
        SHL
        ZERO
        IMM     8
        OSET                    ; cilium 8, power
        ZERO
        ONE
        IMM     6
        OSET                    ; cilium 6, mounted +x
        IMM     12
        ONE
        IMM     8
        OSET                    ; cilium 8, mounted +y
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     100
        CMP                     ; -1 if poor, 0 or 1 if it can afford to divide
        ONE
        ADD                     ; 0 if poor, non-zero if not
        JMPZ    lean            ; too poor — skip the whole copy, do not sleep through it
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
