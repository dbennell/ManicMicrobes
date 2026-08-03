; ancestor.mm — the first thing that is alive.
;
; A photo-autotroph. It eats waste and its oxidant from the water, photosynthesises them into
; substrate, burns the substrate for energy, excretes the waste back, and divides when it has
; the mass to. Nothing here is clever and nothing here is optimal; the point is that it closes
; the loop and persists, so that everything after M2 has something to select on.
;
; Read it as four genes and a driver. The driver runs every tick and expresses whichever gene
; the cell's situation calls for — which is the whole reason `EXPRESS` binds by similarity
; rather than by name: a mutation to a promoter changes *when* a gene runs, not whether the
; genome still parses.
;
; Chemical indices, matching the default table of SPEC §7.1 and the metabolic chemistry of
; the organelle catalogue:
;   4  carbon      structural, what a body is built out of
;   8  sugar       the energy substrate
;   11 carbon_dioxide   the waste
;   14 brine       standing in for dissolved oxygen
;
; Organelle slots this ancestor uses:
;   0  membrane   (always)
;   1  nucleus    so it can copy itself, and so its copy fidelity is a trait
;   2  mitochondrion
;   3  chloroplast

        EXPRESS #build
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
