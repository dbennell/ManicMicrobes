; mutator.mm — the ancestor, with its copy fidelity written down where evolution can reach it.
;
; Identical to `ancestor.mm` except for one gene. The nucleus's copy-fidelity control (SPEC
; §8, organelle 1) is set every tick from an immediate in the genome, which is what makes the
; mutation rate a *trait* rather than a constant: a point mutation to that one operand byte
; moves the whole lineage's fidelity, and the lineage keeps the consequences.
;
; Without this gene fidelity is whatever `Organelle::finished` happened to default to and
; never changes, so there is nothing for M2's acceptance test 4 to measure. That is not a
; quirk of the test — a trait no genome expresses is not under selection.
;
; The dial: `IMM 128; IMM 2; SHL` is 512, half of Q10_ONE. Half rather than full because the
; test asks whether fidelity *rises*, and a lineage already pinned at the ceiling can only
; fall. A point mutation to the 128 lands anywhere in 0..255, so the reachable fidelity range
; is 0..1020 in steps of 4 — fine-grained enough to climb, coarse enough to climb visibly.
;
; What it is selected on: high fidelity costs energy per byte copied (`copy_energy_per_byte`
; scaled by fidelity), so accuracy is paid for out of the same budget as growth. In a stable
; world a good genome is worth preserving and the cost is worth paying. In a world that keeps
; changing, the genome being preserved is one adapted to a world that has moved on.

        EXPRESS #build
        EXPRESS #nucleus
        EXPRESS #feed
        EXPRESS #grow
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body
;
; Runs every tick and is mostly a no-op after the first few: BUILD on a slot that already
; holds what was asked for still costs the matter, so the driver only reaches here while the
; cell is small. A cheaper ancestor would test OTYPE first; this one is written for legibility.

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
        RET

; ---------------------------------------------------------------- the mutation rate itself
;
; OSET takes ( value control slot -- ), slot on top.

        GENE    #nucleus
        IMM     128
        IMM     2
        SHL                     ; 512: half fidelity, room to evolve in either direction
        ZERO                    ; control 0 — copy fidelity
        ONE                     ; slot 1 — the nucleus
        OSET
        RET

; ---------------------------------------------------------------- feed
;
; Take in what photosynthesis needs and push back what respiration makes. The amounts are
; deliberately larger than one tick's throughput: EAT is clamped to what is there and to what
; the cell can hold, so asking for too much costs nothing but an instruction.

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
; Respiration exhales a reactive byproduct — chemical 13, peroxide — and above a threshold it
; damages the membrane. A cell that lets it build up ages and dies; one that dumps it into the
; water lives, because peroxide is unstable and decomposes back into carbon dioxide out there,
; which is food again. This gene is the whole difference between this ancestor and
; `ancestor_sloppy.mm`, and it is what M2's selection test measures.
;
; It also dumps surplus sugar, which would otherwise fill the cytoplasm and stop it eating.

        GENE    #grow
        IMM     255
        IMM     13              ; peroxide, out
        EMIT
        DROP
        IMM     8
        IMM     8               ; surplus sugar back to the water
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide
;
; The replication loop of SPEC §5.2, guarded by an energy check that now works.
;
; It used to read `JMPNZ enough / HALT / HALT / enough:`, and it guarded nothing. `HALT` yields
; the rest of the *tick* and the instruction pointer has already moved past it, so the two HALTs
; delayed a division by two ticks and then it happened anyway, at any energy at all. Measured on
; `predator.mm`: dividing at 46 energy against its own bar of 200, every fifty-one ticks, each
; one stripping the parent and producing a daughter too poor to build a body. Every shipped
; genome had this, under a comment claiming the guard was worth its instructions.
;
; A forward `JMPZ` over the whole block is the fix. Sleeping is still cheap and still happens —
; the top-level `HALT` after the last `EXPRESS` does it, and did it all along.
;
; The threshold went from 200 to 100 with it, because 200 was never a real number: the branch it
; gated was dead, so nothing ever tested whether the economy could clear it. It cannot. With the
; guard working there is a cliff between 140 and 100 — one founder on `soup.ron` reaches 2 cells
; at 140 and 1,070 at 100.

        GENE    #divide
        ONE
        ZERO
        OGET                    ; membrane slot 0, reading 1: energy
        IMM     100
        CMP                     ; -1 if poor, 0 or 1 if it can afford to divide
        ONE
        ADD                     ; 0 if poor, non-zero if not
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
