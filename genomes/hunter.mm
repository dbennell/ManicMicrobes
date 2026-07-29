; hunter.mm — the ancestor, with a spike.
;
; Identical to `ancestor.mm` except for one gene: it builds a spike and holds it out. That is
; the whole of predation. There is no attack instruction, no target selection and no
; "eat" — a spike wounds whatever the cell is touching, a wounded cell eventually dies, a
; dead cell leaves carrion, and anything with a lysosome standing in carrion gets substrate
; back out of it.
;
; So this genome is not a predator because it says it is. It is a predator because of what it
; is built out of, which is the same reason the analysis layer calls it one.
;
; It is deliberately *not* a good predator. It has no lysosome, so it kills without eating —
; it makes carrion for whatever comes along next. A lineage that pairs the spike with a
; lysosome is strictly better, and the point of shipping this one is to give that lineage
; something to improve on rather than to hand it the answer.

        EXPRESS #build
        EXPRESS #arm
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     40
        IMM     1               ; nucleus
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

; ---------------------------------------------------------------- the spike
;
; Built in slot 5 and extended to full. `OSET ( value control slot -- )`, slot on top, and a
; spike's control 0 is its signed extension — so a mutation to the immediate below retracts
; it, which is how a lineage stops being a predator without losing the organelle.

        GENE    #arm
        IMM     80
        IMM     12              ; spike
        IMM     5               ; slot 5
        BUILD
        IMM     128
        IMM     2
        SHL                     ; 512, half extension
        ZERO                    ; control 0 — extension
        IMM     5               ; slot 5
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
        IMM     13              ; peroxide, out
        EMIT
        DROP
        IMM     8
        IMM     8
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET
        IMM     200
        CMP
        ONE
        ADD
        JMPNZ   enough
        HALT
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
