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
; ---------------------------------------------------------------- mass is bought with shell
;
; Mass comes from building organelles: `matter_cost = build_matter + build_matter_per_param *
; param`, and it lands in `cells.mass`. So the question is which organelle is the cheapest
; ballast, and the answer is not the vacuole:
;
;   at param 255      mass    upkeep/tick    mass per upkeep
;   vacuole           35.9        2.12             17
;   shell             76.8        2.07             37
;
; A shell is **twice the mass for the same upkeep**, because `build_matter_per_param` is q10/4
; against the cheap default's q10/8 and its base upkeep is the lowest in the catalogue.
;
; The catch is that a shell shades what is under it — `shell_cover_of` scales cover by
; `control[0]`, and the same word closes the shell and darkens the cell, deliberately, because
; "it is one surface doing one thing". A finished organelle starts at `[Q10_ONE, 0]`, so a shell
; built and left alone is shut and dark.
;
; So `#bulk` opens it: `control[0] = 0` is a shell that covers nothing, shades nothing, protects
; nothing, and still weighs everything. That is not a loophole, it is the trade the control word
; exists to express — a genome that wants the light back opens up, and this one wants the light
; and the ballast and is willing to be edible for them. A descendant that meets something big
; enough to swallow it has the other setting available and one mutation away.
;
; ---------------------------------------------------------------- and why it divides on mass
;
; Engulfment has no kin check either, for the same reason the spike has none, and `SPLIT` halves
; mass evenly — so mother and daughter come out of a division at 1:1 and neither can eat the
; other while the ratio needs to be 2:1. The danger arrives later: a mother who regrows to twice
; what her daughter still weighs qualifies, and `predator.mm`'s comment records what happens to a
; lineage whose parent kills its young.
;
; Dividing on **mass** rather than on energy is what keeps that from being a sterility switch. A
; cell that splits the moment it reaches `BIG` can never be more than `BIG`, its daughters start
; at `BIG/2`, and reaching twice a `BIG/2` daughter means reaching `BIG` — which is the tick it
; divides instead. The two thresholds are the same number by construction, so the mother is
; always dividing at the moment she would otherwise qualify, and she is small again immediately.
;
; It is not proof against a *stalled* daughter: one that cannot feed stays at `BIG/2` while her
; mother regrows, and is eventually eaten by her. That is left in deliberately. A starving
; daughter being recycled by her mother is an ecological outcome, not a bug, and it is exactly
; the kind of thing the engine should be allowed to do without anybody legislating about
; families.
;
; The energy guard stays as well, below the mass guard: a cell that is big enough but too poor to
; pay `division_energy` must not spend the whole copy discovering that.
;
; ---------------------------------------------------------------- IT DOES NOT WORK, AND WHY
;
; **This genome is not viable, and it is shipped for the same reason `hunter.mm` is.** It builds
; exactly the body it is written to build and then cannot pay for it. Alone on
; `predator_introduction.ron` at full light, one founder, mutation off:
;
;   tick    mass  energy  organelles
;      0      30     400  membrane nucleus/44 mito/50 chloro/60      (the seeded kit)
;    100     147     393  membrane nucleus/56 mito/50 chloro/100 vacuole/24 shell/160
;    200      73     214  the same six
;    300      36     126  **membrane only** — everything else shed
;    400      91      71  membrane, mito rebuilding
;    500     132      40  membrane nucleus(building) mito vacuole — no chloroplast
;    600     132       8  the same, and broke
;    700       —       —  dead
;
; It reaches its target body inside a hundred ticks, holds it for a hundred more, and then
; autophagy takes it apart from the outside in. The chloroplast goes with everything else, which
; is the death spiral: the one organelle earning anything is shed to pay for the ones that are
; not, and it never gets rebuilt because rebuilding it costs energy the cell no longer has.
;
; It **starves**; it is not poisoned. Peroxide never exceeds 11 and damage stays at zero for the
; whole run, so this is not the exhaust problem — it is upkeep against income, flat out.
;
; And that is not a fixable property of this genome. `docs/ECONOMY.md` §12.1 measured the same
; wall from the other side, with matched pairs built by hand so mutation never had to find them:
;
;     pairs  organelles   population   starved   poisoned
;         1           4          818     2,177          0
;         2           6          666       737          0
;         3           8            0        16        153
;         4          10            0         0         32
;
; Six organelles is where the population has already fallen by a fifth and eight is where it
; reaches zero. This cell has six, and it needs them: the vacuole is the mouth, the shell is the
; mass, and neither is optional for a cell whose whole living is being twice the bulk of its
; neighbour. A leaner engulfer is not an engulfer.
;
; So the conclusion is about the engine and not about the assembly. **Predation needs ownership of
; the kill, ownership needs engulfment, engulfment needs a large body, and a large body is not
; currently affordable.** M8's trophic-structure gate is therefore blocked behind §12.1, whose
; stated cause is that respiration's exhaust scales with respiration while excretion does not —
; and the starvation mode measured here says upkeep against income is the half of it that bites
; first, before the exhaust does.
;
; What this genome is for, until that changes: it is the smallest complete statement of the
; problem. It assembles, it fits its nucleus, it builds the right body, it opens the first vacuole
; mouth in `genomes/`, and it dies of the bill. When §12.1 is fixed this genome should live, and
; that makes it the test for whether the fix worked.

        EXPRESS #build
        EXPRESS #bulk
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
        IMM     24              ; vacuole — the mouth. Small: see below.
        IMM     4
        IMM     4
        BUILD
        IMM     160             ; shell — ballast, not armour
        IMM     15
        IMM     5
        BUILD
        ZERO                    ; control 0 — cover *nothing*, so the light still lands
        ZERO
        IMM     5
        OSET
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

        GENE    #mouth
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
