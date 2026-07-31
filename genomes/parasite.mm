; parasite.mm — a cell that reproduces by rewriting other cells.
;
; Nothing in the engine knows the word. There is a soft junction, which is a channel, and
; there is INJECT, which writes one byte of your genome into whatever is on the other end of
; it. SPEC §8.3 makes nucleus access symmetric on purpose: reading and writing genome bytes is
; one interface whether the target is self or a neighbour, so `#infect` below is the
; replication loop of §5.2 with COPYB swapped for INJECT and PB pointing somewhere else.
;
; That symmetry is the whole reason there is no virus in the codebase and can be one on the
; slide. Making horizontal transfer a special case would have meant writing the mechanism
; twice and letting the two drift apart.
;
; Organelle slots:
;   0  membrane   (always)
;   1  nucleus
;   2  mitochondrion
;   3  chloroplast   — it has to pay its own upkeep between hosts
;   5  touch sensor  — to notice something worth joining
;   6  junction port — the socket the junction sits in
;
; ---------------------------------------------------------------- what it is worth
;
; Measured rather than assumed, and the result is not the flattering one.
;
;   mm-cli run scenarios/soup.ron --genome genomes/parasite.mm --ticks 2000
;       1,733 cells from 16 seeded. It sustains itself perfectly well on its chloroplast.
;
;   mm-cli match genomes/ancestor.mm genomes/parasite.mm
;       The ancestor eliminates it a little after tick 4,000, having peaked around 16,000
;       against this genome's 130.
;
; The arithmetic is the answer. Converting one host costs GLEN injections — 351 of them here —
; which at `instr_per_tick` of 16 is some seventy ticks of doing nothing else, on top of
; however many key guesses failed first. The ancestor divides in far less. Infection is a
; slower way to make a cell than division, and rewriting a competitor into a copy of yourself
; does not help when the copy is also slow.
;
; Specialising on a fixed key instead of guessing at random was tried and barely moved the
; result, which is the useful part: the join cost is not the binding constraint here, the
; payload length is. A parasite that won would need to carry far less — which is to say it
; would need to stop looking like a cell, which is what a real virus did.
;
; Kept as a demonstration that the mechanism is real and costs what §8.2 says it should. It is
; not a competitive genome and is not meant to be one.

        EXPRESS #build
        EXPRESS #feed
        EXPRESS #infect
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     48              ; nucleus: 384 bytes, room for 351 and some drift
        IMM     1
        IMM     1
        BUILD
        IMM     60              ; a chloroplast, to pay its way while it hunts
        IMM     3
        IMM     3
        BUILD
        IMM     50
        IMM     2
        IMM     2
        BUILD
        IMM     12              ; a touch sensor, to notice something to infect
        IMM     9
        IMM     5
        BUILD
        IMM     12              ; and a port for the junction to sit in
        IMM     10
        IMM     6
        BUILD
        RET

; ---------------------------------------------------------------- feed
;
; The ancestor's, unchanged. A parasite that cannot feed itself dies in the gap between hosts,
; and a lineage that later loses this to reductive evolution is a result rather than a bug.

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

; ---------------------------------------------------------------- infect
;
; Three states, and the cell works out which one it is in by asking its own organelles rather
; than by remembering. Already joined: write. Touching something: try to get in. Neither:
; sleep. Asking is more robust than remembering — a junction broken from the other end does
; not leave this cell believing it still has one.
;
; The DROP after JOIN is load-bearing. §8.2's probe semantics give back one bit and
; deliberately not the distance to the true key: leak the distance and the key is
; hill-climbable in about seven probes, and parasitism stops costing anything.

        GENE    #infect
        ZERO                    ; junction port, output 0 — how many junctions do I have
        IMM     6
        OGET
        JMPNZ   connected
        ZERO                    ; touch sensor, output 0 — how many contacts
        IMM     5
        OGET
        JMPNZ   touching
        HALT                    ; nothing to work with; sleeping is cheap
        HALT
touching:
        RAND
        IMM     127
        AND                     ; one guess at its receptor key, 0-127
        ZERO                    ; kind 0 — soft, a channel rather than a strut
        ONE                     ; touch sensor, output 1 — the handle it reported
        IMM     5
        OGET
        JOIN
        DROP                    ; one bit comes back, and one bit is all it is worth
        RET
connected:
        GLEN
        SETLN                   ; LN = my own length
        ZERO
        SETPA                   ; read from my byte 0
        ZERO
        SETPB                   ; write to its byte 0
pump:
        ZERO
        INJECT                  ; its nucleus[PB] = my genome[PA]; PA++, PB++, LN--
        DROP
        LOOPLN  pump
        ZERO
        LEAVE                   ; it is me now; let go and find another
        RET

; ---------------------------------------------------------------- keep house

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
