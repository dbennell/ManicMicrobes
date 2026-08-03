; drifter_blind.mm — the control: the same cell with nothing to see with.
;
; The motile-but-blind control for M3's chemotaxis experiment. Identical to `drifter.mm` byte
; for byte except that slot 7 holds a `RESERVED_A` organelle instead of a chemosensor: same
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
        IMM     64              ; nucleus: 8 bytes of genome per unit, and this genome is 329,
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
        IMM     14:4             ; RESERVED_A — built, paid for, and blind
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
; Full power on both cilia, every tick, in whatever direction they were mounted. The power is
; written to control input 0 and the mount angle to control input 1; a mutation that made
; either of them depend on `OGET 1 7` — the sensor's x gradient — would be chemotaxis.
;
; It does not. It writes a constant.

        GENE    #swim
        IMM     255
        ZERO
        IMM     6
        OSET                    ; cilium 6, power
        IMM     255
        ZERO
        IMM     8
        OSET                    ; cilium 8, power
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
