; expression.mm — gene blocks and promoter binding (SPEC §4.4).
;
; EXPRESS does not name a gene, it describes one: it finds the GENE header whose promoter is
; closest in Hamming distance and calls it. Deleting a gene therefore does not orphan its
; callers — they bind the next-best match — and duplicating a gene and mutating its promoter
; yields a paralog expressed under different conditions, which is the actual mechanism by
; which biological novelty arises.
;
; Under M0's null host EAT and EMIT report nothing, so this genome is exercised for its
; control flow rather than for its metabolism.

        EXPRESS #forage
        EXPRESS #excrete
        HALT

        GENE    #forage
        IMM     20              ; amount
        IMM     3               ; chemical 3
        EAT                     ; ( amount chem -- got )
        DROP
        RET

        GENE    #excrete
        IMM     4               ; amount
        IMM     9               ; chemical 9
        EMIT                    ; ( amount chem -- sent )
        DROP
        RET
