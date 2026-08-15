; evader.mm — an autotroph that will not sit still next to anybody.
;
; `drifter.mm` is the M3 experiment: a cell carrying a chemosensor it never reads, waiting for
; mutation to find the four instructions that would connect it to the cilia. This genome is what
; those four instructions look like when they are written by hand, pointed the other way.
;
; It watches **peroxide**, chemical 13 — respiration's byproduct, which every living cell has to
; dump into the water or take damage from. That makes a peroxide gradient a map of where cells
; are, drawn by the cells themselves and paid for by nobody. This one swims *down* it.
;
; # What it is for
;
; Not to be a clever cell. To be *prey worth hunting for*.
;
; On a mat, a predator is always already touching something, so no sense has anything to add and
; the ecology collapses to whoever eats cheapest. `scenarios/the_scattering.ron` supplies the
; other half — a slide lean enough in structural carbon that the population is scarce rather than
; packed — and this genome keeps that scarce population *apart*, so that a hunter arriving in
; empty water has to choose a direction rather than simply open its mouth.
;
; The two halves are separate on purpose. Scarcity is the scenario's business and can be dialled
; without touching a genome; keeping apart is the cell's, and can be swapped for a different rule
; without rebuilding the world.
;
; # Why fleeing is not the same as being repelled
;
; The gradient is written straight to the cilia as *signed* power, negated. A cell in clean water
; reads nothing and drifts; a cell with company reads the direction of that company and pushes
; the opposite way, harder the closer it is. There is no threshold and no state — the behaviour
; is a single subtraction away from `drifter.mm`'s, which is the point. Whatever this does, a
; mutation can undo in one byte.
;
; The gradient needs no amplifying. `sensing::visible_gradient` divides a `Q10` difference by
; four, so a peroxide gradient of four thousand across two squares already saturates a cilium.
;
; Slots, as `drifter.mm`:
;   0  membrane    (always)
;   1  nucleus
;   2  mitochondrion
;   3  chloroplast
;   6  cilium, mounted +x
;   7  chemosensor, tuned to chemical 13 — the peroxide everybody exhales
;   8  cilium, mounted +y

        EXPRESS #build
        EXPRESS #keep
        EXPRESS #feed
        EXPRESS #flee
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body

        GENE    #build
        IMM     64              ; nucleus: 8 bytes of genome per unit
        IMM     1
        IMM     1
        BUILD
        IMM     50
        IMM     3               ; chloroplast
        IMM     3
        BUILD
        IMM     40
        IMM     2               ; mitochondrion
        IMM     2
        BUILD
        IMM     60
        IMM     7:4             ; chemosensor
        IMM     7
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     6
        BUILD
        IMM     80
        IMM     6               ; cilium
        IMM     8
        BUILD
        RET

; ---------------------------------------------------------------- tune and mount
;
; The sensor's chemical and the two mount angles are constants, so they are set once a tick and
; never thought about again. Perpendicular mounts are what make steering two independent scalars
; rather than an arctangent this instruction set cannot compute — `stalker.mm` says why.

        GENE    #keep
        IMM     13              ; watch peroxide
        ZERO
        IMM     7
        OSET
        ZERO
        ONE
        IMM     6
        OSET                    ; cilium 6, mounted +x
        IMM     12
        ONE
        IMM     8
        OSET                    ; cilium 8, mounted +y
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
        IMM     255
        IMM     13              ; peroxide out — which is also the signal everyone else reads
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- flee
;
; The four instructions, twice, with a sign on them.
;
; `OGET 1 7` is the sensor's gradient along x and `OGET 2 7` along y. Negated, each becomes the
; power of the cilium mounted on that axis. Away from peroxide is away from whoever made it.

        GENE    #flee
        ONE
        IMM     7
        OGET                    ; peroxide gradient along x
        NEG
        ZERO
        IMM     6
        OSET                    ; cilium 6, signed power
        IMM     2
        IMM     7
        OGET                    ; peroxide gradient along y
        NEG
        ZERO
        IMM     8
        OSET                    ; cilium 8, signed power
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
        JMPZ    lean
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
