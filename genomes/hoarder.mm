; hoarder.mm — the ancestor, with somewhere to put things.
;
; `ancestor.mm` with two changes and nothing else, so that a race between them measures one
; strategy rather than two genomes:
;
;   * it builds a vacuole in slot 4
;   * it does not dump its surplus sugar
;
; The point of the pairing is that neither change pays on its own. A vacuole with nothing in it
; is upkeep; kept sugar with no vacuole is free solute, and free solute is charged for
; (`MetabolicRates::osmotic_upkeep`) because a dissolved particle pulls water in and a
; polymerised one does not. Together they are a battery made of matter: fix sugar while the sun
; is up, hold it out of solution for nothing, burn it when the sun goes down.
;
; It is a strategy for a world that has a night longer than a cell's energy reserve will cover.
; Under uniform light it should simply lose, and if it does not, the reserve is too small or the
; vacuole is too cheap. That is the measurement, not the hope.
;
; Chemical indices as `ancestor.mm`:
;   4  carbon      structural
;   8  sugar       the energy substrate, and what this one banks
;   11 carbon_dioxide   the waste, and photosynthesis's input
;   14 brine       standing in for dissolved oxygen

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     40              ; param
        IMM     1               ; nucleus
        IMM     1               ; slot 1
        BUILD
        IMM     60
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     50
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        IMM     200             ; and the granule, which is the whole of the difference
        IMM     4               ; vacuole
        IMM     4               ; slot 4
        BUILD
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, the input to photosynthesis
        EAT
        DROP
        IMM     20
        IMM     14              ; and its oxidant
        EAT
        DROP
        IMM     16
        IMM     4               ; carbon, to build a body out of
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house
;
; Peroxide still goes out — that is poison, not stores, and holding it is how a cell ages. The
; sugar dump of `ancestor.mm` is gone, which is the second half of the strategy.

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
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
