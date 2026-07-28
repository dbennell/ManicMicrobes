; dormant.mm — sleeping is cheap, so dormancy is evolvable (SPEC §5).
;
; HALT yields the rest of the instruction budget for the tick and refunds a fraction of its
; cost. A cell that halts immediately spends almost nothing, which makes waiting out a bad
; season a strategy a lineage can find rather than a special case the engine grants.
;
; The RAND draw before each HALT is deliberate: it proves the draw is a pure function of
; (seed, tick, cell_id, purpose, counter) and not of a stream, since this genome consumes one
; value per tick forever without any state accumulating anywhere but the counter.

wake:
        RAND
        IMM     15
        AND
        JMPZ    act             ; roughly one tick in sixteen
        HALT
        JMPB    wake
        HALT                    ; unreachable, and it keeps the jump's template from fusing
                                ; with the label below: a template is the *maximal* run of
                                ; NOP letters, so two adjacent ones become one
act:
        IMM     1
        IMM     6
        EAT
        DROP
        HALT
        JMPB    wake
