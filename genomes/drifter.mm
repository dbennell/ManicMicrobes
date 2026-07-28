; drifter.mm — everything needed to chase food, and no idea that it could.
;
; This is the ancestor for M3's chemotaxis experiment, and the point of it is what it does not
; contain. It builds a chemosensor and two cilia, it beats the cilia, and it never once reads
; the sensor. The sensor sits there reporting a gradient to nobody.
;
; So the cell swims — in whatever direction its cilia happen to be mounted, forever, regardless
; of where the food is. Everything the behaviour needs is present and paid for: a sensor tuned
; to the right chemical, thrusters that can be steered by writing to a control input, and an
; instruction budget with room to spare. What is missing is four instructions connecting them.
;
; Mutation has to find those four instructions. Nothing scores it for doing so; a cell that
; happens to steer toward food eats more, divides more, and leaves more descendants, and that
; is the entire mechanism.
;
; The control condition it is measured against is `drifter_blind.mm`, which is this genome with
; its chemosensor replaced by an inert organelle. Both swim, both cost the same to run, and one
; has information the other does not. If the experiment works, the sighted line ends up closer
; to its food than the blind one; if it does not, the difference tells you which parameter is
; starving it.
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
        IMM     7:4             ; chemosensor (width pinned, so the blind control is the same length)
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
        IMM     200
        CMP                     ; -1 if poor, 0 or 1 if it can afford to divide
        ONE
        ADD                     ; 0 if poor, non-zero if not
        JMPNZ   enough
        HALT                    ; not yet; sleeping is cheap
        HALT
enough:
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
        RET
