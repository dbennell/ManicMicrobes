; engulfer.mm — the predator that eats by being bigger.
;
; `docs/FEEDING.md` §4 measured why predation does not pay and drew two conclusions that
; together rule out most of the obvious fixes:
;
;   > A new acquisition route that delivers a burnable substrate delivers nothing, because
;   > conversion and not supply is the limit. A route pays only if it delivers *structural
;   > matter* … **Ownership of the kill matters more than the yield of the kill.** The largest
;   > term in the loss is spatial: the food lands somewhere else.
;
; `predator.mm` fails both. Half of its victim becomes carrion, digestion recovers two thirds of
; that, the deposit lands where the *victim* died, and what does arrive is a burnable substrate
; the mitochondrion was already the binding term on. Measured over 20,000 ticks on
; `predator_introduction.ron`, seeded 24 ancestors / 6 predators / 6 scavengers, its lineage
; peaks at 79 cells around tick 2,500 and ends at three:
;
;   with a scavenger present   79  54  39  25  17   6   4   3     collapsed
;   with no scavenger          95 186 271 330 305 259 207 180     held
;
; The scavenger is not competing for space — its presence costs the ancestor 12% and the
; predator sixty times that. It is eating the predator's kills. A spike makes a public good.
;
; Engulfment satisfies both of §4's conditions by construction: the body arrives **inside** the
; predator, where nothing else can reach it, and it arrives as **structure**, which is built
; with rather than burnt. This is the genome for it, and it is the first in `genomes/` to open a
; vacuole's mouth at all.
;
; ---------------------------------------------------------------- what makes a kill
;
; There is no predator flag and there must not be one. `ecology::step` gates engulfment on a
; size comparison: the eater's raw mass against the victim's *bulk*, where
;
;     bulk = mass + mass * shell_cover        and        needed = bulk * engulf_ratio
;
; with `engulf_ratio` at two. So being twice the bulk of the thing next to you is the whole of
; the qualification, and a shell counts towards what has to be got round — armour is what a cell
; grows when it does not intend to be swallowed.
;
; Measured on `predator_introduction.ron` with 24 ancestors and mutation on, mass in whole units:
;
;   tick     cells   min   p50   p90   max
;    500       170     7    49   113   144
;  2,000     1,826     4    61   121   399
;  5,000     3,090     4    59   108   399
; 10,000     3,381     4    60   101   398
;
; The median prey weighs 60, so 120 buys the middle of the population and 220 buys nine tenths
; of it. `max_mass` is 400, which is also where the fattest ancestors sit — those are not food
; for anybody, because eating one would take 800 and the ceiling forbids it.
;
; This genome targets a little over 200, which reaches p90 and leaves headroom under the cap.
;
; ---------------------------------------------------------------- mass, and the shell that went
;
; Mass comes from building organelles: `matter_cost = build_matter + build_matter_per_param *
; param`, and it lands in `cells.mass`. This carried a `param 160` shell for a while purely as
; ballast, on the arithmetic that a shell is the cheapest mass in the catalogue — 76.8 units for
; 2.07 of upkeep at param 255, against a vacuole's 35.9 for 2.12 — with `control[0] = 0` so that
; it covered nothing and shaded nothing while still weighing everything.
;
; **It is gone, and dropping it cost nothing measurable.** Once the vacuole grew to 120 and a
; lysosome arrived, the body reached mass 125 on its own, which already clears twice the median
; prey of 60. The shell was buying bulk this cell no longer had to buy, at the price of a seventh
; organelle in a catalogue where `ECONOMY.md` §12.1 measures eight as fatal.
;
; The trick is left written down because it is real and a descendant may want it: a shell opened
; to nothing is mass without shade, which is the trade `control[0]` exists to express. What it
; costs is being edible — an open shell adds nothing to the bulk an eater has to get round.
;
; Measured on `predator_introduction.ron` with 24 ancestors and mutation on, mass in whole units,
; which is where the targets above come from:
;
;   tick     cells   min   p50   p90   max
;    500       170     7    49   113   144
;  2,000     1,826     4    61   121   399
;  5,000     3,090     4    59   108   399
; 10,000     3,381     4    60   101   398
;
; The median prey weighs 60, so 120 buys the middle of the population and 220 buys nine tenths of
; it. `max_mass` is 400, and the fattest ancestors sit there — those are food for nobody, because
; eating one would take 800 and the ceiling forbids it.
;
; ---------------------------------------------------------------- and why it divides on mass
;
; Dividing on **mass** rather than on energy keeps this lineage's weight in a band instead of
; letting it drift upward, which matters because weight is the whole of what decides who it can
; eat — including, without a kin check anywhere, its own daughters.
;
; It is *not* sufficient on its own, and the third section below is the record of finding that
; out: `#divide` needs energy as well as mass, so a fat and poor cell sails past `BIG` and keeps
; growing. `#mouth` is what actually closes that hole. This gene keeps the band; that gene refuses
; to qualify while the band is exceeded.
;
; The energy guard stays, below the mass guard: a cell big enough but too poor to pay
; `division_energy` must not spend the whole copy discovering that.
;
; ---------------------------------------------------------------- WHAT KILLED IT, THREE TIMES
;
; The first draft of this header blamed `docs/ECONOMY.md` §12.1 — six organelles is unaffordable,
; therefore an engulfer cannot live. **That was wrong, and it was wrong in the way a comfortable
; explanation usually is: it was true of something, and it was not what was happening here.** Two
; of the three things that killed this cell were bugs in this file.
;
; Measured on `predator_introduction.ron`, six founders against twenty-four ancestors, `Spread`:
;
;   what it was                                    trajectory                    ends
;   no lysosome                                    dead by tick 2,500            extinct
;   + a lysosome                                   6 30 18 14 5                  dead by 10,000
;   + a mouth that shuts at BIG                    6 29 14 28 24 ... 4           holds to 5,000
;
; **First: it had no gut.** Engulfment used to deposit a victim as structural matter, which is
; build material and needs no enzyme, so the first draft carried no lysosome deliberately and said
; so — "matter is what it steals, light is what it works for". Once a swallowed cell began handing
; over its cytoplasm and its body separately, the body arrived as *carrion*, and this cell had
; nothing to digest it with. The instrumentation said so plainly and nobody read it: carrion held
; climbed to 88, which is exactly this body's `interior_capacity`, and stopped. It was eating from
; tick 200 and starving with a full stomach the entire time.
;
; **Second: it ate its own children.** `ecology::step` gates a kill on bulk and has no kin check,
; so a genome cannot refuse a victim — the engine picks. `SPLIT` halves mass evenly, so a mother
; qualifies to swallow her daughter the moment she doubles back to her divide weight. The first
; draft called that safe on the grounds that the two thresholds were the same number, and they are
; not: `#divide` also needs *energy*, so a cell that is fat and poor sails past `BIG` and keeps
; growing. Cells were measured at 249 against a divide threshold of 140, which is twice a 124-unit
; daughter with room to spare. The lineage peaked at thirty and ate itself back down while every
; survivor was well fed — high mass, high energy, falling population, which is a signature worth
; recognising because it looks nothing like starvation.
;
; `#mouth` is the fix and it is the interesting one: **not to qualify.** Past `BIG` the cell shuts
; its mouth, because the only things it now outweighs two to one are its own young.
;
; **Third, and still open: it does not hold past five thousand ticks.** With both fixed it
; sustains twenty to fifty cells to tick 5,000 and is down to four by 10,000 — with a scavenger
; present and mutation on, twenty-one at 2,000 and one at 10,000. It is no longer dying of any of
; the above: energy is high at the end, not low. What it is dying of is not yet known, and the
; candidates are §12.1's ceiling (this still carries six organelles), prey growing past what it
; can outweigh as the slide fills, and the mouth-shut band above `BIG` leaving it a pure autotroph
; for long stretches with a heterotroph's upkeep.
;
; So the honest state: engulfment works, the feeding chain works, and this genome is viable for a
; few thousand ticks and not yet for twenty. `an_engulfer_cannot_pay_for_the_body_that_engulfment
; _needs` still passes and still asserts the *solo* case, which is unchanged — alone on a slide
; with nothing to eat it dies of upkeep at about tick 700, and that part was always about §12.1.

        EXPRESS #build
        EXPRESS #bulk
        EXPRESS #gut
        EXPRESS #mouth
        EXPRESS #feed
        EXPRESS #keep
        EXPRESS #divide
        HALT

; ---------------------------------------------------------------- build the body
;
; Nucleus 56 is 448 bytes, against this genome's length; the ancestor's 40 would hold 320 and
; truncate every daughter at division, which stops a lineage without ever reporting an error.
;
; The chloroplast is *larger* than the ancestor's rather than smaller, which is the opposite of
; what `predator.mm` does and is the point. Engulfment delivers structural matter, not energy —
; the swallowed body is build material, and it is `interior[structural]` when it arrives. So a
; big eater still has to earn every unit of energy it spends on upkeep, and it is carrying two
; and a half times the ancestor's upkeep to do it. Matter is what it steals; light is what it
; works for.

        GENE    #build
        IMM     56              ; nucleus: 448 bytes
        IMM     1
        IMM     1
        BUILD
        IMM     100             ; chloroplast — the only income this cell has
        IMM     3
        IMM     3
        BUILD
        IMM     50              ; mitochondrion, to burn what the chloroplast fixes
        IMM     2
        IMM     2
        BUILD
        RET

; ---------------------------------------------------------------- the ballast and the stomach
;
; Slot 4 is the vacuole: the mouth, and the room to put a body in. `BASE_INTERIOR_CAPACITY` is
; 64 units and a swallowed ancestor arrives as three quarters of its mass — 45 units for a median
; prey — so a cell with no vacuole would fill up on its second meal and spill the rest into the
; water, which is the public good all over again. 255 buys the headroom to keep what it takes.
;
; Slot 5 is the shell, opened to nothing. See the header: cheapest mass in the catalogue, and
; `control[0] = 0` declines the cover so the chloroplast above still sees the sun.

        GENE    #bulk
        IMM     120             ; vacuole — the mouth, and the stomach it needs to be
        IMM     4
        IMM     4
        BUILD
        RET

; ---------------------------------------------------------------- the gut
;
; **Without this the cell swallows and starves with its mouth full**, and it did, for exactly as
; long as this gene was missing. Measured on `predator_introduction.ron`, six founders, prey in
; reach from tick zero:
;
;   tick   cells   mass   energy   carrion held
;    200      10    107      214             44
;    500       3    361       98             88
;   1000       0      0        0              —
;
; It was eating from the first two hundred ticks. The carrion column is the whole story: it climbs
; to 88 and stops, because 88 is this body's `interior_capacity` — sixty-four base plus what the
; vacuole adds — and then nothing happens to it ever again. A swallowed body arrives as *flesh*,
; and flesh is not food until something digests it.
;
; That is a change this genome was written before. Engulfment used to deposit the victim's mass as
; structural matter, which is build material and needs no enzyme, so the first draft carried no
; lysosome and said so in its header: "matter is what it steals; light is what it works for". Once
; a swallowed cell started handing over its cytoplasm and its body separately, the body became
; carrion and the header became wrong.
;
; No `OSET` needed: a lysosome is not one of the organelles that reaches outside the membrane, so
; `default_control` starts it at `[Q10_ONE, 0]` — open — where a spike or a holdfast starts shut.

        GENE    #gut
        IMM     100             ; lysosome, to turn what it swallows into something burnable
        IMM     11
        IMM     6
        BUILD
        RET

; ---------------------------------------------------------------- open the mouth
;
; Appetite is the vacuole's `control[1]`, and it is shut on a fresh organelle. That is not an
; accident of the catalogue: it was on `control[0]` once, `Organelle::finished` starts that word
; wide open, and every one of the ten vacuole-growing genomes in `genomes/` silently became a
; predator. `m2_life::selection_guard` caught it — the tidy strain's advantage fell from over 90%
; to 57%, because cells had started dying of being eaten rather than of copying badly, and
; mortality uncorrelated with fidelity is precisely what that test measures the absence of.
;
; So a mouth has to be asked for, every generation, by a genome that means it. This one means it.

; It shuts the mouth once it is big, and that is not thrift — it is the only defence against
; eating its own young that a genome has.
;
; `ecology::step` gates a kill on bulk and nothing else. There is no kin check and there must not
; be one, so **a genome cannot refuse a victim**; appetite is all-or-nothing and the engine picks
; the target. The only lever left is not to *qualify*: stay under twice the weight of whatever is
; standing next to you.
;
; `SPLIT` halves mass evenly, so a mother at `BIG` leaves two cells at `BIG/2` and reaches twice
; her daughter's weight exactly when she gets back to `BIG`. The first draft called that safe
; because `BIG` is also the divide threshold — she would divide on the same tick she qualified.
; It is not safe, because `#divide` needs mass **and** energy, and a cell that is fat but poor
; sails past `BIG` and keeps growing. Measured: a divide threshold of 140 and cells at 249, which
; is twice a 124-unit daughter with room to spare. The lineage peaked at thirty and ate itself
; back down to nothing while every survivor was well fed.
;
; So the mouth closes at `BIG` instead. Past that weight the cell has nothing to gain by eating —
; it is trying to divide — and everything to lose, because the only things it now qualifies to
; swallow are its own children.

        GENE    #mouth
        ZERO                    ; reading 0 — this cell's own mass
        ZERO                    ; slot 0 — the membrane is the self-sensor
        OGET
        IMM     140             ; BIG, the same number `#divide` uses
        CMP
        ONE
        ADD
        JMPZ    hungry          ; under weight — safe to eat, nothing here is half of me
        ZERO                    ; at weight — shut, before a daughter looks like dinner
        ONE
        IMM     4
        OSET
        RET
hungry:
        IMM     255
        IMM     2
        SHL                     ; 1020, near the Q10 clamp
        ONE                     ; control 1 — appetite
        IMM     4               ; the vacuole
        OSET
        RET

; ---------------------------------------------------------------- feed

        GENE    #feed
        IMM     40
        IMM     11              ; carbon dioxide, for the chloroplast
        EAT
        DROP
        IMM     20
        IMM     14
        EAT
        DROP
        IMM     16
        IMM     4               ; structural carbon, for whatever is still being built
        EAT
        DROP
        RET

; ---------------------------------------------------------------- keep house

        GENE    #keep
        IMM     255
        IMM     13              ; peroxide, out, or it poisons itself
        EMIT
        DROP
        IMM     8
        IMM     8
        EMIT
        DROP
        RET

; ---------------------------------------------------------------- divide
;
; Mass first, then energy. See the header for why the mass guard is what stops this lineage
; eating its own young.

        GENE    #divide
        ZERO                    ; reading 0 — this cell's own mass, in whole units
        ZERO                    ; slot 0 — the membrane is the self-sensor
        OGET
        IMM     140             ; BIG
        CMP
        ONE
        ADD
        JMPZ    done            ; not yet worth splitting, and not yet a danger to a daughter
        ONE                     ; reading 1 — energy
        ZERO
        OGET
        IMM     100
        CMP
        ONE
        ADD
        JMPZ    done            ; too poor to pay for the division; do not sleep through the copy
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
done:
        RET
