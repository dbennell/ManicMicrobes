; scavenger.mm — the ancestor, with a lysosome.
;
; The other half of M8's food web. It builds a lysosome and holds the throttle open, so a cell
; standing in carrion turns it back into substrate. It has no spike: it does not kill, it
; waits.
;
; Together with `hunter.mm` this makes a three-level web out of three organelles and no
; special cases — producers photosynthesise, hunters make carrion out of producers, and
; scavengers make substrate out of carrion. Nothing in the engine knows any of those words.

        EXPRESS #build
        EXPRESS #digest
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

        GENE    #build
        IMM     40
        IMM     1
        IMM     1
        BUILD
        IMM     60
        IMM     3
        IMM     3
        BUILD
        IMM     50
        IMM     2
        IMM     2
        BUILD
        RET

; ---------------------------------------------------------------- the lysosome
;
; Slot 6, throttle wide open. A lysosome's control 0 is its digestion rate.

        GENE    #digest
        IMM     90
        IMM     11              ; lysosome
        IMM     6
        BUILD
        IMM     255
        IMM     2
        SHL                     ; 1020, near full throttle
        ZERO
        IMM     6
        OSET
        RET

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

        GENE    #grow
        IMM     255
        IMM     13
        EMIT
        DROP
        IMM     8
        IMM     8
        EMIT
        DROP
        RET

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
