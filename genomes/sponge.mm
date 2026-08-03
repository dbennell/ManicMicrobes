; sponge.mm — the first cell that stays still on purpose.
;
; Everything else in `genomes/` either swims or drifts, because until now there was nothing to
; be gained by holding position: a cell in a current went where the current went, and so did its
; food, so the water past it never moved and there was nothing to strain out of it.
;
; Four things had to exist before this genome could mean anything, and none of them mentions
; filter feeding — which is the point SPEC §17.6 makes about there being no `sessile` flag:
;
;   * a wall that is solid to a body, so there is something to hold on to
;   * a holdfast, so the current can be refused rather than obeyed
;   * a chemical the flow carries at its own speed rather than the water's, so there is
;     something in the water worth catching
;   * capture as a *flux*, so what a cell gets is what goes past it
;
; Put together they make a cell that is better off doing nothing in the right place than
; swimming anywhere, and nothing in the engine knows the word sponge.
;
; It has no cilia. That is the commitment: it cannot go and look for a better spot, so where it
; lands is where it lives, and the whole strategy is a bet that the water will bring it enough.
; `drifter.mm` is the opposite bet with the same chemistry.

        EXPRESS #build
        EXPRESS #anchor
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body
;
; Nucleus at 40 is 320 bytes, and this genome is under that with room to drift. A chloroplast,
; because a sponge that could not photosynthesise would starve waiting for its first meal — the
; filter is a second income and not a replacement for the first, which is the lesson
; `predator.mm` paid for by trying to live on one trade alone.

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

; ---------------------------------------------------------------- hold on, and strain
;
; One organelle, two jobs, one control word. `control[0]` is how hard it grips *and* how hard it
; filters, because it is one surface doing one thing: a holdfast held out hard holds on hard and
; strains hard. Full throttle, because there is no reason to do either by halves — the grip is
; charged on the force it actually resists, so a sponge in still water pays nothing for being
; ready.
;
; Slot 4, and `param` is most of what decides whether this pays: filtering goes with the size of
; the filter and so does the upkeep, so this number is exactly the kind of thing a mutation
; should be able to move in either direction and find out.

        GENE    #anchor
        IMM     200
        IMM     14              ; holdfast
        IMM     4
        BUILD
        IMM     255
        IMM     2
        SHL                     ; 1020, near enough full
        ZERO                    ; control 0 — grip, and filter
        IMM     4
        OSET
        RET

; ---------------------------------------------------------------- feed
;
; The dissolved half. Detritus arrives through the filter without being asked for; this is the
; ordinary business of being a cell, and it is what keeps the lights on between grains.

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, for the chloroplast
        EAT
        DROP
        IMM     20
        IMM     14              ; oxygen, for the mitochondrion
        EAT
        DROP
        IMM     16
        IMM     4               ; carbon, to build a body out of
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out, or respiration poisons it
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
        OGET                    ; membrane slot 0, reading 1: energy
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
