; hoarder.mm — the ancestor, with somewhere to put things.
;
; `ancestor.mm` with two changes and nothing else, so that a race between them measures one
; strategy rather than two genomes:
;
;   * it builds granules — vacuoles — in slots 4 and 5
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
; ---------------------------------------------------------------- two granules, not one
;
; It built one, of 200, and **could not survive anywhere** — not in the dark it was written for
; and not in the soup either. It starved, every time, and never once poisoned itself.
;
; The arithmetic, from `balance::what_actually_kills_the_hoarder`. It holds about 837 units of
; free solute against an osmotic threshold of 256, and `turgor_cost` is quadratic in the excess:
;
;     excess 581 units  ->  2,632 `Q10` a tick
;     gross income      ->  2,400 `Q10` a tick   (a mitochondrion at param 50)
;
; The tax on what it was storing was larger than everything it earned, before a single organelle
; was paid for. Setting the two equal locates the cliff: this engine can hold about **810 units**
; before storage costs it everything, and the hoarder sat at 837 — over the edge by three per
; cent, which is why it died on every slide rather than only the dark ones.
;
; A vacuole exempts `param` units from the reckoning and `param` is a `u8`, so **one granule can
; hide at most 255 and this cell needs 510.** The strategy was not mispriced; it was out of reach
; in one organelle, and no amount of tuning a single number could have found it.
;
; Two and not three, measured on `soup.ron` over twelve thousand ticks:
;
;     granules   cells   exempt   free solute
;            1       0        -             -
;            2     591      510           656
;            3      33      765           393
;
; The third granule is cheaper turgor and a worse cell. Each costs 36 units of structural carbon
; to build and 271 `Q10` a tick to carry whether or not there is anything in it — and upkeep is
; charged on the container while the saving is only on the contents. Two is where that trade
; turns.
;
; **It still dies in `seasons.ron` at any granule count**, which is a finding this genome cannot
; fix and `docs/ECONOMY.md` owns: in a world where the light comes and goes the income is lower,
; so the fixed upkeep of the granules is what becomes unaffordable. The container costing more
; than the contents are worth is the thing to look at, and it is a price in the catalogue rather
; than a number in a genome.
;
; Chemical indices as `ancestor.mm`:
;   4  carbon      structural
;   8  sugar       the energy substrate, and what this one banks
;   11 carbon_dioxide   the waste, and photosynthesis's input
;   14 oxygen      the oxidant every pathway breathes

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
        IMM     255             ; the granules, which are the whole of the difference
        IMM     4               ; vacuole
        IMM     4               ; slot 4
        BUILD
        IMM     255
        IMM     4               ; vacuole
        IMM     5               ; slot 5 — see the header on why one is not enough
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
