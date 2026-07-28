; replicator.mm — the minimal viable replicator of SPEC §5.2.
;
; Ten instructions. COPYB plus LOOPLN makes the inner copy loop two instructions long, which
; is what keeps a self-replicator inside the reach of random bytes: the primary path is
; seeding this by hand, but de-novo emergence stays a scenario option rather than a fantasy.
;
; Nothing here has a position. Under M0's null host BUD and SPLIT do nothing and COPYB writes
; into a daughter buffer that is only observed by the tests; the copy loop itself is real, and
; the tests check that the daughter bytes come out identical to the parent.

        GENE    #replicate
        GLEN
        SETLN                   ; LN = own genome length
        GLEN
        BUD                     ; allocate the daughter buffer, and PB = 0
        DROP                    ; ignore the result: a failed BUD wastes a tick, not a cell
        ZERO
        SETPA
        ZERO
        SETPB
loop:                           ; the label is four template letters, emitted here
        COPYB                   ; daughter[PB] = genome[PA]; PA++, PB++, LN--
        LOOPLN  loop            ; base-pairs back to the letters above while LN != 0
        SPLIT
