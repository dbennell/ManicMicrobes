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
; Slot 5, held out a sixtyfourth. This number was 512 — half extension — and at 512 this genome
; **kills its own daughters**, which took a long time to see because every plausible explanation
; was wrong first.
;
; A spike damages what it touches. It has no idea what it is touching: `ecology::step` filters
; the victims by "not me", "occupied" and "within reach", and that is the whole list. There is no
; kin check and there must not be one — a special case for "my own offspring" is exactly the kind
; of flag `CLAUDE.md` forbids. A daughter is born inside her mother's reach, and this genome has
; no cilia, so neither of them can leave.
;
; `spike_damage` is `Q10_ONE/16`, so damage a tick is `64 * extension / 1024`. A newborn dies of
; two a tick in about twelve ticks. That is the cliff, and it is sharp — one founder in
; `soup.ron`, 2400 ticks:
;
;   extension     4      16      32      64     128     512
;   population  183      64       2       2       1       1
;
; What is *not* the cause, each ruled out by measurement rather than by argument: it is not the
; divide guard (sweeping the threshold from 200 down to 60 changes nothing at all), not the
; energy (a bigger engine puts it at 131 against its own bar of 100 and it still will not breed),
; not the copy (it buds thirteen times in twelve hundred ticks and completes all 339 bytes every
; time), not the nucleus, not the chloroplast, and not the want of prey — among a thousand prey
; it reaches two. It divides perfectly well. The daughters die.
;
; Sheathing the spike for the duration of the copy does not save them either: it is back out on
; the next pass, and she is still there. Cilia do not save them: two of them and the power to beat
; them gets to two cells. Only a spike small enough to be survivable does.
;
; So this is the finding the genome exists to carry: **an armed cell that cannot tell kin from
; prey and cannot move away from either has to carry a weapon its own children can survive.**
; Sixteen is one point of damage a tick — a prey cell with a membrane of twenty-four takes some
; twenty-four ticks of unbroken contact to kill, which is a real weapon and a slow one. Half
; extension is not a weapon at all. It is a sterility switch.

        GENE    #arm
        IMM     80
        IMM     12              ; spike
        IMM     5
        BUILD
        IMM     4
        IMM     2
        SHL                     ; 16 — see above; 512 sterilises the lineage
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
