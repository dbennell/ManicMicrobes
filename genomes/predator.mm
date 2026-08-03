; predator.mm — the hunter, with a stomach.
;
; `hunter.mm` builds a spike and nothing else, and its own comments say why that is a bad
; deal: it kills without eating. A spike costs the dearest upkeep in the catalogue and what it
; produces is carrion, which is a public good — anything with a lysosome standing nearby gets
; the meal. A hunter with no lysosome pays for its neighbours' lunch.
;
; This is that lineage's obvious improvement, written out: the same spike, plus the lysosome
; that turns a kill into food. It is the genome the acceptance test for trophic structure uses,
; because a predator that cannot eat what it kills does not make a trophic level, it makes a
; brief and expensive mistake.
;
; The two are shipped side by side deliberately. `hunter.mm` is what predation looks like when
; only half of it has been discovered, and this is what it looks like when both halves have.
; Nothing in the engine knows the difference — there is no predation code path, no target
; selection, no attack. There is a spike, which damages what it touches; damage, which kills;
; death, which makes carrion; and a lysosome, which digests carrion. Predation is what those
; four look like from far enough away.

        EXPRESS #build
        EXPRESS #arm
        EXPRESS #digest
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

; A bigger nucleus than the ancestor's, and the reason is worth writing down: nucleus capacity
; is `param * 8` bytes, and this genome is 342 bytes against the ancestor's 262. At the
; ancestor's `param 40` the nucleus holds 320 bytes, so SPEC §4.1 truncates every daughter at
; division and cuts the tail off `#divide` — the cell divides once into something sterile and
; the lineage stops. Which is the mechanism working: genome bloat costs a bigger nucleus, and
; a bigger nucleus costs upkeep. It just has to be paid rather than ignored.

        GENE    #build
        IMM     56              ; nucleus: 448 bytes, room for 342 and some drift
        IMM     1
        IMM     1
        BUILD
        IMM     55
        IMM     3               ; chloroplast — smaller than the ancestor's, because the
        IMM     3               ; upkeep has to leave room for the spike
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        RET

; ---------------------------------------------------------------- the spike
;
; Slot 5, held out at half extension. Damage and upkeep both scale with extension, so half is
; a compromise a mutation can move in either direction: further out to kill faster, further in
; to survive a lean patch. Which way pays depends on how much prey there is, which is exactly
; the feedback a predator-prey oscillation is made of.

        GENE    #arm
        IMM     80
        IMM     12              ; spike
        IMM     5
        BUILD
        IMM     128
        IMM     2
        SHL                     ; 512, half extension
        ZERO                    ; control 0 — signed extension
        IMM     5
        OSET
        RET

; ---------------------------------------------------------------- the stomach
;
; Slot 6, wide open. This is the whole difference from `hunter.mm`.

        GENE    #digest
        IMM     70
        IMM     11              ; lysosome
        IMM     6
        BUILD
        IMM     255
        IMM     2
        SHL                     ; 1020, near full throttle
        ZERO                    ; control 0 — digestion rate
        IMM     6
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
        IMM     100
        CMP
        ONE
        ADD
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
