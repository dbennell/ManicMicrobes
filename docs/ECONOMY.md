# The economy, measured — and why every road leads back to the autotroph

An audit of what a cell earns, what it pays, and what it is competing for, prompted by the
observation that every shipped genome ends up doing the same thing: build a chloroplast and sit
still. The observation is correct, the reason is not the one the design assumed, and the fix is
not in the price list.

Everything below is measured by `crates/mm-core/tests/economy_probe.rs`, with mutation off, on
`soup.ron`'s chemistry. Run it with

```
cargo test --release -p mm-core --test economy_probe -- --ignored --nocapture --test-threads=1
```

`Q10` energy per tick throughout; 1024 `Q10` is one energy unit.

---

## 0. Where this stands, for whoever comes back to it

This document has grown by measurement, and several of its later sections **correct earlier
ones**. Read this page before trusting anything below it.

### What is settled

* **Respiration is the only income, and its rate depends on one organelle** (§1). No entry in the
  catalogue but the mitochondrion appears in the income expression, so every other organelle can
  only ever move the upkeep column. This is the finding everything else turns out to be a face of.
* **The carrying capacity is spatial, not economic** (§3). A saturated soup has eaten 8% of its
  structural carbon and refuses thirty divisions a tick for want of room.
* **Organelles are not too dear** (§9a). Scaling the whole catalogue from a quarter to double
  changes the evolved loadout not at all, and changes a matched pair's share by about 5%.
* **Light is not scarce anywhere the library measures** (§5), and making it scarce is the only
  dial that moves which strategy wins (§9).
* **A fat prey is worth 240% of a division to its killer, by `EAT` alone** — `apply_deaths`
  returns the whole cytoplasm to the square before any of predation's lossy arithmetic applies.

### What has been corrected, and where

| do not believe | believe instead |
| --- | --- |
| §2, "the ranking is upkeep upside down" | §2a — that was one world. Across five, the median contender moves 536 of 1000 |
| §6, "the `#feed` genes eat on a timer and swell the cell" | §6a — it is oxygen from a chloroplast bigger than its mitochondrion. §6b is what happened when it was fixed |
| §10a, "re-cut `seasons` first" | §11 — the winter already thins rather than culls, and the hoarder's death was never about it |
| §10, "dimming the light comes first" | §11.4 — nothing can afford to get fat in the dark, so the cost side has to move first |
| §12.1, "the earning pair cannot be stacked" | fixed. A lysosome is now catalase; see below |
| §4b, "not the cost of swimming — motility is nearly free and nearly useless" | §14 — the cost is right and the rest is not. Below 192 `Q10` of thrust a cell was driven *backwards* by its own wake, and every cilium in the library was under it. **Fixed in §14.7** |
| §12.3, "the sensors and cilia work, and have been judged on the wrong slide" | §14.4 — half withdrawn. `drifter.mm`'s 927‰ in the drift was its cilia holding station against the current with their backwash, not swimming. Every motility number in this document predating §14.7 is about a body that could not move forward |

### What has landed in the code

* **`sentinel.mm` and `stalker.mm` draw their weapons** (§4a). They never had, in any run, because
  their kin check reads an unbadged cell as an empty square.
* **`hoarder.mm` builds two granules** (§11.2). One could not exempt enough solute to keep the
  cell alive, because a vacuole's `param` is a `u8`.
* **A lysosome decomposes the cell's own peroxide** (§12.1). This was named in a comment in
  `metabolism.rs` and never built, and it was the wall that stopped any cell carrying more than
  two mitochondria.
* **A cilium no longer swims its cell backwards** (§14.7). Its reaction stopped accumulating, and
  a cell stopped being carried by the part of the water that is its own wake. `drifter.mm` went
  from −8.5 squares in 600 ticks to +121.5, and an anchored ciliate still reads the current it is
  making, so a cilium is a pump and a propeller rather than one or the other.
* **Six parameters reach the state hash** that did not, and a guard enumerates the config so it
  cannot happen again.

### The open questions, in the order they matter

1. **Does specialising lose because the world is packed?** With catalase, a metabolic specialist
   survives at depth 3, 4 and 6 — and its share *falls* with depth: 365, 294, 230, 221. The
   hypothesis is §3 returning: `radius` goes as the square root of mass, a fifteen-organelle cell
   takes more area, and a share counted in cells falls by construction. **This is unmeasured.** It
   wants the specialist's mass and radius against the ancestor's, and it is the single most
   valuable hour left in this document — if it holds, a space-bound world always rewards "small
   and numerous", no amount of earning beats it, and §10.4 stops being an also-ran.
2. **Give the acquisition routes something to deliver** (§10.2, §12.2). Lysis on the spike's free
   `control[1]`, and a wound that leaks. Unchanged and still the largest piece of work.
3. **Price the vacuole on what it holds rather than how large it is** (§11.4). It is the one
   catalogue entry whose function is economic and the one where the price decides the outcome.
4. **Measure on worlds that vary.** Free, and already paying: `drifter.mm` is 19‰ in the still
   soup and 927‰ in the drift. The sensors and cilia are not broken; they have been judged on the
   one slide where nothing they report is worth knowing. — **The cilia are broken; see §14.** The
   worlds still want varying and the 927‰ is not evidence for it.

### What §14 adds, and it is the largest thing outstanding

**Nothing in this engine has ever swum forward.** A cell producing under 192 `Q10` of thrust is
driven backwards by the wake it makes, because its thrust reaches it through a dragged velocity
and its own reaction reaches it through an undragged drift that accumulates sixteenfold first.
Every cilium in `genomes/` is under that threshold, `drifter.mm` included, and `IMM` cannot push
the number that would clear it. In a crowd the first cilium returns 3% of its open-water speed and
the third returns 67%, so the gradient into motility is convex where evolution needs it concave.

This displaces the ranking above rather than joining it, because motility is upstream of most of
it: the pursuit predator, the bloodhound, the pack hunter and the ram feeder in §8 are all
strategies that have to get somewhere first, and none of them has ever been tested on a body that
could.

### How to run any of it

```
cargo run -p mm-cli --release -- balance          # the panel, and its fairness control
cargo test --release -p mm-core --test balance -- --ignored --nocapture
cargo test --release -p mm-core --test economy_probe -- --ignored --nocapture --test-threads=1
```

The gates live in `tests/balance.rs`; the measurements behind this document live in
`tests/economy_probe.rs`. Both are `#[ignore]`d, because a probe answers a question once and an
acceptance test guards an answer forever, and only the second kind belongs in the default run.

---

## 1. The finding, in one line

**Respiration is the only income in the engine, its rate depends on one organelle, and every
other organelle is a pure cost.**

```text
    income  =  respiration_efficiency  ×  throughput_per_param  ×  Σ mitochondrion param
            =  0.75 × 64 × param
```

Nothing else anywhere adds to `cells.energy`. A chloroplast makes *matter*; a lysosome makes
matter; a filter makes matter; `JXFER` moves energy between cells but creates none. All of that
matter becomes energy only by going through a mitochondrion, at a rate the mitochondrion sets and
the supply does not.

So the books of every shipped organism have the same top line. Measured, one founder alone in a
lit dish after 900 ticks:

| genome | fix | burn | gross | upkeep | thrust | spike | **net** | loadout beyond the ancestor's |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| `ancestor` | 3840 | 3200 | 2400 | 460 | 0 | 0 | **1940** | — |
| `mutator` | 3840 | 3200 | 2400 | 460 | 0 | 0 | **1940** | — |
| `oscillator` | 3840 | 3200 | 2400 | 508 | 0 | 0 | **1892** | oscillator(32) |
| `parasite` | 3840 | 3200 | 2400 | 548 | 0 | 0 | **1852** | nucleus 48, touch(12), junction port(12) |
| `scavenger` | 3840 | 3200 | 2400 | 568 | 0 | 0 | **1832** | lysosome(90) |
| `reflex` | 3840 | 3200 | 2400 | 576 | 0 | 0 | **1824** | nucleus 48, cilium(40), touch(12) |
| `hoarder` | 3840 | 3200 | 2400 | 676 | 0 | 0 | **1724** | vacuole(200) |
| `sponge` | 3840 | 3200 | 2400 | 681 | 0 | 0 | **1719** | holdfast(200) |
| `predator` | 3520 | 3200 | 2400 | 809 | 0 | 20 | **1571** | nucleus 56, spike(80), lysosome(70) |
| `sentinel` | 3520 | 3200 | 2400 | 897 | 0 | 0 | **1503** | nucleus 64, spike(80), lysosome(70), touch(40) |
| `marble` | 3840 | 3200 | 2400 | 950 | 0 | 0 | **1450** | membrane 255, vacuole(12) |
| `stalker` | 3520 | 3200 | 2400 | 1137 | 5 | 0 | **1258** | nucleus 80, photosensor(24), spike(80), 2×cilium(60), touch(24), lysosome(70) |
| `hunter` | 3840 | 3200 | 2400 | 662 | 0 | 640 | **1098** | spike(80), half extended |
| `drifter` | 3200 | 2560 | 1920 | 804 | 38 | 0 | **1078** | nucleus 64, chemosensor(60), 2×cilium(80) |

`fix` and `burn` are matter a tick — what the chloroplasts can photosynthesise in full light and
what the mitochondria can respire. `gross` is the energy the second of those recovers.

The `gross` column is a constant. `drifter` is the only genome that differs, and only because it
built a smaller mitochondrion. Every other column is a bill.

And there is a second thing in that table worth stopping on. **Every shipped genome fixes more
matter than it can burn** — all fourteen of them, most at 3,840 against 3,200. The library's
uniform habit of building a `param 60` chloroplast beside a `param 50` mitochondrion means every
cell in it produces a surplus of sugar and oxidant it can never consume, at 640 `Q10` a tick, and
`cell_step`'s photosynthesis arm writes that surplus into the cytoplasm with no capacity check.
It is free solute by construction, and §6 is what happens to it.

**That is the whole balance problem.** The catalogue prices capability and never pays for it, so
the fitness gradient over loadouts points in exactly one direction: shed organelles. It is an
anti-complexity ratchet.

And it is not a tuning error. No reprice of `OrganelleCatalogue::balanced` can fix it, because no
entry in the catalogue except the mitochondrion appears in the income expression at all — every
other organelle can only ever move the `upkeep` column. What a rebalance has to change is which
terms are in the expression, which is a mechanism change and is what §10 is about.

---

## 2. The race, and it goes strictly by upkeep

Sixteen founders of each genome on one slide, 20,000 ticks, mutation off. `mm-cli match` and the
probe's own two-lineage slide agree to within a few per cent, and the result is side-independent
(`the_two_halves_of_the_slide_are_worth_the_same` is the control).

| challenger | challenger | `ancestor` | challenger's share |
| --- | ---: | ---: | ---: |
| `sponge` | 326 | 536 | 38% |
| `scavenger` | 367 | 632 | 37% |
| `hoarder` | 265 | 775 | 25% |
| `sentinel` | 230 | 709 | 24% |
| `stalker` | 33 | 1034 | 3% |
| `drifter` | 21 | 1058 | 2% |
| `predator` | 11 | 1055 | 1% |
| `hunter` | 3 | 1036 | 0.3% |

Nothing beats the ancestor. The ordering is the upkeep column of §1, upside down, with one
correction for body size. There is no strategy here — there is a single scalar, "what does your
body cost", and the ranking is its reciprocal.

That is more literally true than it looks: §4a establishes that `sentinel` and `stalker` never
extend a spike in any of these runs, so two of the four armed rows are not predation losing, they
are autotrophy carrying luggage.

Note also that challenger plus ancestor is roughly constant at 1000–1070 in every match. The two
lineages are dividing a fixed quantity, and §3 says what it is.

### 2a. And that ranking is not universal — the panel says so

**This table was taken on one world.** `mm-cli match` plays on `arena_scenario`, which is uniform
light, still water and food everywhere — `soup.ron` in all but name. Every conclusion in it is a
conclusion about the control condition, and reading it as "nothing ever beats the ancestor" was
generalising from a single slide. The panel in `mm_core::balance` runs five, and it disagrees:

```text
                  soup   thicket      dusk   seasons     drift     best  spread  wins
     drifter       19       211         0*        0       927       927     927     1
     hoarder      132       467         0*        0*      303       467     467     0
      hunter        8         1         0*        0*        0*        8       8     0
      marble        9       340         0*        0*       17       340     340     0
  oscillator      385       456       417       244       637       637     393     1
    parasite      574       367         8         0*     1000      1000    1000     2
    predator       20        11         0         0*      263       263     263     0
   scavenger      387       429       350         3       539       539     536     1
    sentinel      315       273       326         0*      733       733     733     1
      sponge      333       433       332         0*      692       692     692     1
     stalker       57         0         0*        0      1000      1000    1000     1

fairness control:  soup 518   thicket 502   dusk 501   seasons 481   drift 516  — all level
```

The `spread` column is the correction. `drifter` is 19 in the soup and **927** in the drift; a
swimmer is a passenger in a still dish and the best thing on the slide in a current. `stalker` goes
57 to 1000. **The median contender's fortunes move by 536 of 1000 across the panel**, against a
floor of 100, and three different genomes are best in at least one world. The economy is not the
one-axis contest §2 describes; the *soup* is.

That is worth saying plainly because it changes what the rebalance is for. The problem is not that
strategies cannot pay — several can. It is that **the world every result in this project is
measured on is the one world where none of them do**, which is §5 and §3 arriving together.

Two cautions on the numbers above. They are a single seed for the contenders (the fairness control
gets three, by `MIRROR_SEEDS`), so an individual cell is noisy and only the shape is safe to read.
And `drift` inflates everything, because the reference is a sessile autotroph in a current and
copes badly there — a high share in that column is partly the ancestor losing rather than the
contender winning.

---

## 3. What the slide actually runs out of, which is none of the things it was built to run out of

`ancestor.mm` on `soup.ron`, run to saturation:

```text
   tick   cells  C in body  C in cyto  C in water   refused   med E  med solute
   6000    1034      58793      66091     1386425        34    2024         308
  12000    1049      59398      67090     1383468        29    1602         285
  20000    1063      59908      68023     1379363        31     104         269
  24000    1061      59676      67904     1378815        37     102         269
```

* **Not matter.** 128,000 units of structural carbon are in cells and 1,379,000 are still
  dissolved in the water. The population has consumed **8%** of the one resource the design calls
  conserved and limiting.
* **Not energy.** Every cell is running a five-fold surplus over its own upkeep.
* **Not light.** See §5.
* **Space.** Thirty divisions a tick are refused by `split_pressure` — a cell with nowhere to put
  a daughter. The carrying capacity is a *packing* number, and the two-lineage totals in §2 are
  constant for the same reason.

So the world is a zero-sum contest over area, and the winner of a contest over area is whoever
turns matter into daughters fastest, which is whoever carries least. §1 and §3 are the same
finding arriving from two directions.

The median cell energy falling from 2024 to 102 as the slide fills is the population converting
its entire surplus into division attempts, most of which are refused. A saturated slide is a
thousand cells all spending everything to try to breed into a space that does not exist.

---

## 4. Why there is no mobile hunter, precisely

Five separate mechanisms each independently prevent it. Fixing any four leaves it broken. The
fifth is a bug in two genomes rather than a property of the engine, and it is §4a.

**1. A kill cannot beat sunlight, because both go through the same pipe.** Measured, following a
corpse the whole way:

```text
   prey mass 30  →  carrion 15  →  sugar 9  →  7,672 Q10 usable
                    (½)            (⅔)         (×0.75 respiration)
   = 3.2 ticks of one mitochondrion's output, 37% of one division
```

An autotroph's mitochondrion is *already* saturated by its own chloroplast — a `param 60`
chloroplast fixes 3,840 `Q10` of sugar a tick and a `param 50` mitochondrion can burn 3,200 of it. `predator_probe` measured the consequence and it is worth
restating because it is the whole argument: carrion placed directly under a predator raised its
internal sugar from 3–17 units to 64–75 and the energy trace was **bit-identical**. Food it
cannot burn is not food. A predator gains nothing from eating unless it first gives up
photosynthesis, at which point its ceiling is the same 2,400 and it has to work for it.

**2. The weapon costs more than the prey is worth.** A spike at `param 80` held fully out costs
`spike_upkeep × extension` = **1,280 `Q10` a tick**, which is 53% of the gross income of a
standard body, on top of 202 a tick to carry the organelle. A kill at 37% of a division must
therefore arrive every six ticks to pay for the weapon alone, and takes four to five ticks of
sustained contact to make.

**3. The meal lands where the victim died.** `apply_deaths` deposits carrion on the victim's
square; `ecology::step` lets a lysosome digest only the square its own cell is standing on. A
predator kills what it is next to. The killer has to walk onto the corpse before the deposit
spreads, and carrion diffuses at `Q10/64` — deliberately, so that a corpse stays where it fell
and is worth swimming to. The design got the mechanism right and put the reward on the wrong
square.

**4. A predator kills its own children.** A spike damages everything within reach, at no extra
cost, and a daughter is born inside her mother's reach. `predator.mm` had to turn its weapon down
to a sixty-fourth to have descendants at all — which is why its `spike` column in §1 reads 20 and
`hunter.mm`'s reads 640. `sentinel.mm` is the fix (a badge and a touch sensor, so the spike goes
away when the nearest thing is kin) and it is the best-performing armed genome by a factor of
twenty.

### 4a. And two of the four armed genomes never draw

This was found by a coincidence in §9 — `spike_upkeep` at an eighth of its value produced
bit-identical populations over eighty thousand ticks, which can only happen if the cost was never
charged — and then measured directly. Eight hunters among eight `ancestor.mm`, 8,000 ticks:

```text
      genome   cell-ticks with a spike out   wounds dealt   badges worn
  stalker.mm                             0              0   {0, 211}
 sentinel.mm                             0              0   {0, 210}
 predator.mm                       562,283      1,323,456   {0}
   hunter.mm                        37,004          7,032   {0}
```

**`sentinel.mm` and `stalker.mm` have never wounded anything.** Not once, in any run. Their badges
are set correctly — 210 and 211 are being worn — so `#dress` works and the kin recognition is
reached. The fault is one instruction in `#watch`:

```text
        IMM     3
        IMM     7               ; the nearest one's badge
        OGET
        DUP
        JMPZ    kin             ; nobody in reach
```

"Nobody in reach" is encoded as *a badge of zero*, and the genome's own header explains why the
test is there: without it, a solitary cell reads zero, finds it differs from its own, and arms
permanently against open water. But **an unbadged cell also reads zero**, and `place_founders`
gives every founder `badge: 0`. So the guard against menacing nothing is also a guard against
attacking anything that has not chosen a badge, which is every other genome in the library.

Two consequences, and the second is the larger one.

* The §2 race is even more purely a cost contest than it looks. `sentinel` at 24% and `stalker` at
  3% are not predators doing badly; they are heavy autotrophs carrying weapons they never draw,
  and their ranking is their upkeep and nothing else.
* **Badge-zero is not a safe sentinel value**, because it is the default for every cell the engine
  seeds and it collides with "no reading". `TouchReading` needs to distinguish "nobody there" from
  "somebody wearing nothing" — either by a separate contact count (reading 0 already reports
  `contacts`, so the genome fix is available today) or by making the absence read as something no
  badge can be.

### 4b. What is *not* the reason

Measured, so that nobody spends a week re-deriving it.

* **Not the instruction budget.** A full pass of every shipped genome takes 4–8.4 ticks
  (`stalker.mm`, the longest at 591 bytes, is 134 instructions = 8.4 ticks). A cell steers several
  times per square travelled. The control loop is not the bottleneck.
* **Not the cost of swimming.** Two cilia at `param 80` at full power cost 160 `Q10` a tick, under
  7% of gross. Motility is nearly free. It is also nearly useless, because on a uniformly lit,
  uniformly fed slide there is nowhere better to be.

  **The second sentence is right and the third is not the reason — see §14.** That measurement was
  taken on one cell alone in open water, at a power no shipped genome can command. Below 192 `Q10`
  of thrust a cell is pushed backwards by the wake it makes, `drifter.mm` produces 158, and in a
  crowd the first cilium returns 3% of its open-water speed. Motility is not a cheap thing nothing
  needs; it is a thing the engine does not currently do.
* **Not blindness.** `hunting_probe` established that a photosensor reading the metabolic band
  steers a cell to a crowd five squares away, and that swimming blind does not. Homing works.

---

## 5. Light is not scarce anywhere in the library

Chloroplast rate is `throughput_per_param × param × light`, so intensity multiplies income
directly. Sixteen `ancestor` founders, mutation off, at four horizons:

```text
 intensity      12k       25k       50k      100k    med E
      1024     1049      1062      1059      1058      102
       768      990       994       994       994     2093
       512       83        81       813      1041     2049
       256       48        32        32        32       93
       128        0         0         0         0        0
```

The first reading of this sweep was taken at 12,000 ticks and reported a cliff between 512 and
1024. **There is no cliff there.** A dim slide has a long lag phase and then converges to the same
ceiling: half intensity holds eighty cells until tick 25,000 and a thousand by 100,000. The real
knee is between 256 and 512, and extinction is at 128.

Against that, the library: 1024 in eight scenarios, 1536 in two, 4096 in the two vent slides.
Every one of those is at or above **four times** the intensity at which light starts to bind.

Two scenarios do make light scarce, and neither has a recorded result anywhere:

* `seasons.ron` runs a 240-tick day inside a 96,000-tick year, summer days at 1280 and **winter
  days at 224** — below the extinction knee — with nights at zero.
* `the_long_dusk.ron` declines from 1536 to nothing over a million ticks, so it crosses the knee
  somewhere around tick 830,000.

So the mechanism for scarcity exists in the scenario format and is used twice, but every slide
that anything is *measured* on — `soup.ron` above all, which is the control condition for the
whole tree — is on the flat top of the curve. Light is a free good in every result this project
has, and `light_occlusion`, the one mechanism that would make it rival between cells rather than
merely finite, is off by default and set in exactly one scenario.

The consequence follows in one step. A second trophic level exists in nature because primary
production is limited; where sunlight is unlimited, being a producer is strictly better than
eating one, and no pricing of the weapon changes that.

---

## 6. Turgor is a division-rate tax, and it forbids every slow strategy

`osmotic_upkeep` charges quadratically on solute above four interior capacities. The design intent
(SPEC §17.7, `STIFFNESS.md`) is a hoarding tax — a fixed point in a quantity that otherwise climbs
forever. What it actually is, in the population:

A cell's solute climbs monotonically, and three things independently make it climb.

**The genomes eat unconditionally.** Every shipped `#feed` gene is the same twelve instructions,
copied verbatim: eat 40 carbon dioxide, eat 20 oxidant, eat 16 carbon, drop each result. Not one
genome in the library reads a membrane chemical reading before eating, so none of them can tell a
full cytoplasm from an empty one.

**A legal amount of everything is sixteen times the threshold.** `interior_capacity` bounds one
chemical at a time — 64 units each, sixteen of them — and `osmotic_load` sums all sixteen.
`biology.rs` says so in its own doc comment. A cell obeying every rule the engine enforces holds
sixteen capacities against a threshold of four.

**And the metabolic step ignores the capacity entirely.** Of the five ways matter enters a
cytoplasm, three respect `interior_capacity` — `EAT` through `CellHost::headroom`, lysosome
digestion and filter capture both through an explicit `room` term — and two do not:
`cell_step`'s photosynthesis and respiration arms `saturating_add` straight into the interior with
no check at all (`metabolism.rs:869` and `:916`). That is how the measured load gets past even
sixteen capacities, which one founder below does.

The only thing that ever *removes* solute is division, which halves it. So the charge is quadratic
in **how long a cell has gone without dividing**. Measured over one founder alone for 6,000 ticks,
eight of the sixteen founders are dead by the end, and six of those eight died swollen — 819 to
1,138 units against a threshold of 256:

```text
            genome   cells  births  died at   energy   solute   damage
       ancestor.mm     598    1878    alive      754      455        0
        drifter.mm      47     290     1548        0      911        0
        hoarder.mm       0      51     1503        1      883        0
     oscillator.mm     499    1511     1618        2     1138        0
      scavenger.mm     419    1260     2963        3     1108        0
         sponge.mm     383     929     3999        0      819        0
```

`ancestor.mm`'s founder is in the table alive, for contrast, at 455 units and climbing;
`drifter_blind.mm` is omitted because it is `drifter.mm`'s body exactly and dies at the same tick
with the same load. The two of the eight that died of something else are `ancestor_sloppy.mm` at
tick 262 and `predator.mm` at 2,037, both at damage 23–24 against a membrane tolerance of 24 —
poisoned, not swollen. The first is by design (§10.5); the second is `predator.mm` stabbing
itself and its own daughters.

The `cells` column is the lineage and the `died at` column is the individual, which is why
`ancestor.mm` has 598 descendants alive while its founder is not among them. A lineage that
divides fast enough survives this; the founder does not have to.

`reflex_probe` found this from one direction ("an undivided cell dies of turgor"). This is the
other direction, and it is the one that matters for balance: **any strategy that trades
reproduction rate for anything else is killed by the osmotic charge.** There is no dormancy, no
lean season, no sit-and-wait ambush, no slow-growing sessile, no k-selected life history — not
because those are priced badly, but because not dividing is lethal within fifteen hundred to four
thousand ticks regardless of how rich the cell is. `reflex.mm` is the clearest case: 0 births in
6,000 ticks, 2,107 energy in hand, and 121 units of solute only because it never earned enough to
fill up.

This is a second, independent reason the hunter loses: hunting is time not spent dividing.

### 6a. The correction: it is oxygen, and it is not the genomes' fault

**The three paragraphs above blame the wrong thing, and the measurement that settles it is
`what_a_swollen_cell_is_swollen_with`.** The unconditional `#feed` genes are real and they are
worth fixing on their own account, but they are not what makes a cell swell. This is what a
founder is actually carrying:

```text
ancestor.mm        solute   energy   the three largest holdings
   tick 1000          314     1028   oxygen 210,  sugar  74,  carbon 16
   tick 4000          823     1706   oxygen 582,  sugar 212,  carbon 16
```

**Seventy-one per cent of a swollen cell is oxygen**, and no genome in the library vents it. The
cause is §1's other finding, arriving from the far end: every shipped genome carries a `param 60`
chloroplast against a `param 50` mitochondrion, so it fixes 3,840 `Q10` a tick and can burn 3,200.
Photosynthesis makes one oxidant for every substrate and respiration consumes one of each, so
**both** accrue, at 640 a tick, forever — and `cell_step`'s photosynthesis arm writes them in with
no capacity check, where `EAT` respects one. A cell cannot eat its way to that; only its own
chloroplast can put it there.

The corroboration is `reflex.mm`, the one genome that vents chemical 14, and the one genome in the
library that does not swell: 38 units against everyone else's 800 to 1,150.

### 6b. What happened when it was fixed, which is why it is not fixed

Recorded because the next person will have the same idea, and because the outcome is a finding
rather than a failure.

Three changes were tried and measured, in this order:

1. **Teaching every `#feed` gene to read before eating** — the immediate becomes a level to hold
   rather than a helping to take, `EAT` taking the shortfall. It works exactly as designed and it
   is aimed at the wrong thing: solute barely moved, and the ancestor went from 1,878 births in
   six thousand ticks to 24, because 16 was never a sensible *level* of structural carbon.
2. **Venting the oxidant** — the excess above a level, `EMIT`'s mirror of the same idiom. This
   works: every genome's solute fell from 800–1,150 to 108–222, every founder in the library
   survived six thousand ticks, and `marble.mm` went from 1 descendant to 262 and `parasite.mm`
   from 4 to 1,209. But it also costs a quarter of the growth rate, because what is being vented
   is carbon the cell spent light to fix.
3. **Matching the chloroplast to the mitochondrion** — the elegant version, since a chloroplast
   larger than the engine it feeds buys no energy at all. It removes the surplus at the source,
   fixes the solute without the vent (the ancestor holds 91 units, flat), and passes the frame
   budget.

And **3 still breaks two acceptance tests**: `sentinel.mm` stops reproducing, and
`ancestor_sloppy.mm` — the deliberately broken control — now dies inside the 300 ticks
`every_shipped_genome_fits_the_nucleus_it_builds_for_itself` allows it. Both were confirmed
against the committed library, so both are consequences of the change and not pre-existing.

**So the library is unchanged and the diagnosis stands.** The organelle sizes in `genomes/` are
not independent numbers: each was chosen against the others, and the nucleus sizes, the membrane
targets and the divide gates were all calibrated on a cell that hoards. Re-sizing one organelle
across fourteen genomes is a re-tune of the whole library, and CLAUDE.md is explicit that a
failing acceptance test is a finding to report rather than a number to move.

What it wants is the panel, not judgement: `mm-cli balance` measures sixteen founders a side over
three seeds across five worlds, which is the instrument for "did this make the library better",
and the founder probe — one founder, one seed — is not. That probe's own header now says so, after
`ancestor.mm` and `mutator.mm`, which have identical `#divide` genes and differ only in length,
came back at 4 cells and 622.



It is worth being precise about the fault, because turgor is a good mechanism badly aimed. The
charge should be on *holding* solute, and it is; the problem is that nothing but division ever
reduces the holding, and the shipped genomes acquire solute on a timer. Both halves are fixable
and neither is the quadratic.

---

## 7. What the sensors can and cannot see

Two questions came up directly and they have short answers.

**The EM profile is real and it is thin.** `OrganelleType::EM_BANDS` is two bands, accumulated
from energy actually paid as it is paid, cleared every tick, and read by a photosensor at
`OGET` indices 3–8 (magnitude and both gradient components per band) out to `ecology.em_range` =
6 squares, falling off as inverse square. It is honest by construction — a cell cannot lie about
what it spent. It is not colour; colour is `ChemicalDef::colour` and is drawn, never sensed.

But **only two things in the whole engine emit**:

| band | emitter | |
| --- | --- | --- |
| 0, mechanical | spike upkeep actually paid (`ecology.rs:433`) | an extended spike, and nothing else |
| 1, metabolic | total upkeep actually paid (`metabolism.rs:1082`) | body size, turgor and leak |

So a swimming cell is silent, a gripping holdfast is silent, a filter is silent, and a dividing
cell is silent. The signature reports "how big and how taxed", plus "is it armed". That is enough
for the hunter `hunting_probe` demonstrates — the loudest thing on the slide is a large well-fed
autotroph, which is exactly the right prey — and it is not enough for anything subtler.

**The signature is a homing sense, not a searching one.** `hunting_probe` measured it: at 20
squares the steered swimmer reads a gradient of zero and sits perfectly still. Six squares is
the whole horizon.

**A bloodhound needs no new sense — it needs something to smell.** This is worth stating
carefully, because "damage is private" makes the gap sound like a sensing problem and it is not.
The sensing half is already built and free: a chemosensor reports a concentration and both
components of its gradient, and a genome that climbs one is four instructions. What is missing is
that **nothing leaks when a cell is hurt**. `cells.damage[i]` moves and no chemical moves with it,
so there is no plume to climb.

That is what `FEEDING.md` §8 ranks first, and the shape of it matters: a wounded cell leaks a
fraction of its *interior* into its square in proportion to `damage`. No new organelle, no new
opcode, no new sensor, no new accounting — the substrate already conserves and diffuses, and the
chemosensor already reads it. The bloodhound is then an existing sense pointed at a signal that
now exists.

What is available today is a *metabolic*-glow bloodhound — climb the band-1 gradient — which finds
the fattest cell rather than the weakest one. That is a different animal and a good one, and
`stalker.mm` is it. §10 argues the two are complements rather than substitutes.

**And nothing draws any of it.** `grep emission crates/mm-app` returns nothing. The microscope
cannot show what the cells can smell, which is a product gap as much as a simulation one — the
species wiki has no way to say "this one is loud".

---

## 8. The strategies the mechanics should support, and what each is missing

Taking the catalogue and the physics as they stand, here is every living the engine can express,
what it would take to see one, and whether it is reachable today. This is the list the rebalance
should be measured against.

| strategy | mechanism it would use | state |
| --- | --- | --- |
| **Sit-still autotroph** | chloroplast + mitochondrion | ✅ works, and wins everything |
| **Osmotroph** | `EAT` on a dissolved substrate | ✅ works, but only `the_long_dusk` seeds free sugar |
| **Hoarder / diel battery** | vacuole + held sugar, burnt at night | ⚠️ mechanism complete, loses under uniform light as designed; **never tested under a real night**, which is the only condition it was built for |
| **Sessile filter feeder** | holdfast + barrier + detritus + slip | ⚠️ **works, and earns nothing** — see §8a |
| **Ram feeder** | cilia + `captured`'s relative-speed term | ❌ expressible, no genome does it, and no scenario has detritus and open water together |
| **Ciliary suspension feeder** | cilia stirring their own square + holdfast + filter | ❌ FEEDING.md §7 flags this as one probe that has never been run: cilia already inject impulse and `slip` is already computed, so an anchored ciliate may already filter on its own current |
| **Scavenger** | lysosome on carrion | ⚠️ works, pays nothing extra (mitochondrion cap), best-performing non-ancestor loadout purely because a lysosome is cheap |
| **Ambush predator** | spike, retracted until contact | ❌ `sentinel.mm` is this and **has never drawn** (§4a); its 24% share is upkeep, not hunting |
| **Pursuit predator** | photosensor + cilia + spike + lysosome | ❌ `stalker.mm`, 1.5% share, and it has never drawn either. All four faults of §4 apply, plus §4a |
| **Bloodhound** | gradient of injury | ❌ **damage is private**; not expressible at all |
| **Pack hunter / histophage** | arrive together at a wound | ❌ same |
| **Parasite** | `JOIN` + `INJECT` into a host nucleus | ❌ mechanism complete and **never once observed**; `reflex_probe` found `parasite.mm` branches on a junction-port reading that does not exist, so its `connected` state is unreachable and it has never injected anything |
| **Toxicyst / chemical warfare** | concentrate peroxide, `EMIT` on a neighbour | ❌ legal today with existing instructions, never measured |
| **Cross-feeder** | pathway A's waste is pathway B's substrate | ❌ built at M10.3, **no scenario authors a pathway set**, so it has never run |
| **Anaerobe** | low-yield pathway that wins where oxidant is scarce | ❌ `Pathway` has no oxidant ratio; FEEDING.md §8 item 2 |
| **Colony / tissue** | junctions + differentiated expression | ⚠️ `reflex.mm` builds one; it does not divide at all (0 births in 6,000 ticks) |
| **Piercing-sucker** | take through a junction | ❌ `JXFER` is push-only by design |
| **Dormancy specialist** | `HALT`, wait out a bad season | ❌ **forbidden by turgor** (§6), which is the finding this document adds |

Twelve of nineteen are unreachable or unexercised, and the reasons cluster into three: the
mitochondrion cap, the absence of scarcity, and the division-rate tax.

### 8a. The filter feeder, measured properly, and the third confirmation of §1

`sponge.mm` on `the_drift.ron` seeded the ordinary way ends at 59 cells against `ancestor.mm`'s
60, which says nothing: `place_founders` spreads founders over the whole slide and a holdfast
grips nothing unless the body overlaps a barrier, so most of those sponges never anchored and
never had any water going past them to strain.

Seeding both lineages **along the wall** — the upper barrier of that scenario, at y = 31 — is the
fair test, and it has now been run. 30,000 ticks:

```text
        genome   cells     med E   filtered   detritus left in the water
     sponge.mm      59       409     55,277                      717,097
   ancestor.mm      66       465          0                      717,111
    hoarder.mm      58       388          0                      717,111
```

**The mechanism works.** The sponge anchored, refused the drift, read a real slip, and pulled
55,277 units of detritus out of the water over the run — every part of `captured`'s rate law
doing what SPEC §17.4 designed it to do.

**And it lost anyway**, to a cell standing next to it doing nothing but photosynthesise.

The last column is why, and it is §1 again in a third costume. The filter delivers **structural
carbon**, at about 0.023 units per cell per tick after `capture_efficiency` — against a growth
rate of 0.25 units a tick, and into a world with **717,000 units of detritus still floating in
it**. It is a working pipeline delivering a resource nothing is short of. The holdfast's 221
`Q10` a tick of upkeep is charged all the same.

So all three of the engine's alternatives to photosynthesis — predation, scavenging and
filtering — fail for one reason in two forms: what they deliver is either capped by the
mitochondrion (energy substrates) or not scarce (structural matter). Neither is a pricing
problem.

---

## 9. Which dial actually moves the answer

One parameter changed at a time — never two — because the value of this is knowing which lever
matters, not producing a world where the hunter happens to win. Sixteen founders a side,
`stalker.mm` against `ancestor.mm`, 80,000 ticks, mutation off.

```text
                   variant   stalker  ancestor    sponge  ancestor
                as shipped        16      1045       333       440
                turgor off        48       996       210       437
  spike 8x cheaper to hold        16      1045       333       440
    a corpse loses nothing        25      1071       345       416
            light is rival       140      1560       244       319
                half light       356      1129       324       573
    half light, turgor off       410      1115       320       559
```

Read the `stalker` column as a share of its own match: 1.5% as shipped, 4.6% with turgor off,
**8% with light rival, 24% at half light, 27% with both**.

**Pricing the weapon changes nothing.** The `spike 8x cheaper` row is bit-identical to the
control — not close, identical, over eighty thousand ticks. §4a says why.

**Fixing the yield of the kill changes nothing.** Making a corpse lossless — `carrion_fraction`
1, `digestion_efficiency` 1, `digestion_rate` 8× — moves the hunter from 16 cells to 25 and moves
the prey *up*. This is `FEEDING.md` §4's finding reproduced from the other end: supply was never
the limit.

**Scarcity is the only thing that moves it, by a factor of sixteen.** Halving the light takes the
hunter from 1.5% of the slide to 24% of it, and it does so without shrinking the world — the
totals are 1,061 as shipped and 1,485 at half light. That is the shape a rebalance wants: not a
smaller world, a world where being a producer is no longer free.

Note the `light is rival` row raises the *ancestor* too, from 1,045 to 1,560. That is
`rigidity_gain` at 16384 changing how cells pack, not occlusion feeding them; it is a reminder
that `the_thicket.ron`'s four settings are one setting and should not be taken apart.

The `sponge` columns are a control on the whole exercise: a holdfast with no barrier to grip and
no detritus to strain is pure upkeep, so its share should move only with the general cost of
living, and it does — 43% as shipped, 32% with turgor off, 36% at half light. Nothing in the
predation dials touches it.

---

## 9a. Do organelles cost too much? No — and that is the worse answer

The obvious reading of §1 is that the catalogue is priced too dear, and it is the first thing
anybody says on seeing that a `param 80` spike at full extension costs 1,482 `Q10` a tick against
a gross income of 2,400. It is worth testing rather than assuming, because if it were true the fix
would be a column of numbers.

It is not true. Every `upkeep` and `upkeep_per_param` in the catalogue, scaled together, sixteen
founders on the soup with **mutation on** — the one question in this document that needs it —
forty thousand ticks:

```text
  upkeep   cells    organs  loadouts   types carried by 1% or more of the population
     25%    1023      4.00         1   membrane, nucleus, mitochondrion, chloroplast
     50%    1041      4.00         2   the same four
    100%    1031      4.00         3   the same four
    200%     997      3.98         4   the same four
```

**Four times cheaper buys nothing.** The mean organelle count does not move off 4.00 — the loadout
the founders were seeded with — across an eightfold range of price, and no fifth type reaches one
per cent of the population at any of them.

The reason is §1 read the other way round. A price suppresses a thing that would otherwise pay;
these do not otherwise pay. **No organelle but the mitochondrion appears in the income
expression**, so a free chemosensor earns nothing, a free cilium earns nothing on a uniform slide,
and a free spike earns nothing because the mitochondrion cap binds before the food does. Zero cost
against zero benefit is zero, and there is no gradient for evolution to climb at any price.

That is the more useful answer, because it rules out the cheap fix. Repricing the catalogue is a
morning's work and would have produced exactly the table above with the population column jittering.

**One entry is genuinely mispriced, and it is the exception that shows the rule.** §11 is the
vacuole, whose upkeep is charged on the container while its saving is only on the contents — the
one organelle whose function *is* in the economy, and the one where the price does decide the
outcome.

---

## 10. The recommendation

Ranked by how much of §1–§6 each item repairs, against what it costs to build. Items 1 and 2 are
the ones that matter; everything below them is refinement.

**1. Make light scarce, and make it rival.** §9 says this is the only dial that moves the answer,
by a factor of sixteen, and §5 says why the question has never come up: every slide anything is
measured on is on the flat top of the light curve. Two changes, independent of each other:

  * **Bring the default scenario intensity down from 1024 towards 512.** The population ceiling is
    barely touched — 994 against 1058 at 100,000 ticks — and the *surplus* is not, which is exactly
    the shape wanted: the same world, with less slack in it. At half light `stalker.mm` goes from
    1.5% of a slide to 24% of it without anything else changing. Note the lag: a dim slide takes
    tens of thousands of ticks to leave its lag phase, so every acceptance horizon has to be
    re-checked, not just every acceptance number.
  * **Turn `light_occlusion` on by default.** It is built, measured (`the_thicket.ron`), and off
    everywhere. Finite light bounds the population; *rival* light makes producers compete with one
    another, which is a different and necessary thing. Take `the_thicket.ron`'s four settings
    together — §9's `light is rival` row shows they move the population as a unit and should not be
    separated.

### Why dimming the light is not merely a nerf

The obvious objection to item 1 is that making the world poorer is a blunt instrument: everything
gets less, nothing changes shape. It is worth writing down why that is wrong here, because the
mechanism it relies on **already exists and is lossless**, and it is the reason to do this before
anything else.

A dark that a cell has to survive forces it to *store*. Energy will not do — `energy_reserve`
leaks anything above two thousand units, and `apply_deaths` dissipates whatever is left as heat
the moment a cell dies, on the explicit ground that "a corpse is not a battery". So the only way
to carry a night is to store **matter**: fix sugar while the sun is up, hold it out of solution in
a vacuole so turgor does not charge for it, and burn it in the dark. That is exactly what
`hoarder.mm` is, and its own header says it should lose under uniform light and that if it does
not, the vacuole is too cheap.

And then the second half. `apply_deaths` returns a dead cell's **whole cytoplasm to the square it
died on** (`biology.rs:1695`) — every chemical it was holding, in full, before any of the
predation arithmetic of §4 applies:

| route | yield of a cell holding 200 units of sugar |
| --- | --- |
| the corpse | mass × ½ × ⅔, digested by a lysosome, on the victim's square |
| **the cytoplasm** | **200 units of sugar, whole, taken by `EAT`, which is free and instant** |

Sixty-four units of sugar — one interior capacity, all a killer can hold at once — is 49,152 `Q10`
of energy once burnt, against a division at about 24,000. **One fat prey is two daughters.** No
lysosome, no `carrion_fraction`, no `digestion_efficiency`, none of the three lossy steps that
make a corpse worthless. It still has to go through the mitochondrion, so it is drunk over twenty
ticks rather than swallowed in one, and the killer has to be standing there before it diffuses.

So the chain is: **dark forces storage, storage is matter, and matter is exactly what a dying cell
hands to whoever is standing on it.** Dimming the light does not merely make producers poorer, it
makes them *worth eating*, through a channel that is already built, already conserved, and already
free of every loss that makes predation not pay today. That is why item 1 comes before item 2 and
not after it — and it is a prediction the panel in `mm_core::balance` is pointed directly at, since
`seasons` and `dusk` are the two worlds in it that make the dark real.

It also predicts what should be watched for and would be a finding either way: if `hoarder` still
loses in `seasons`, the vacuole is too dear or the night is too short; and if `hoarder` wins there
but `predator` does not follow it up, the missing piece is the killer's ability to *find* a fat
cell, which is the metabolic glow of §7 and the one sense that already works.

**2. Then make an acquisition route deliver something respiration cannot.** This is the fault of
§1, and it has to be fixed at the mechanism rather than the price list — no reprice of the
catalogue can help, because no entry in it but the mitochondrion is in the income expression. Two
candidates, both already ranked first and fourth in `FEEDING.md` §8:

  * **Blood in the water** — a wounded cell leaks a fraction of its interior in proportion to
    `damage`. No new organelle, no new opcode, no new accounting: the chemosensor already reads
    gradients and the substrate already conserves. It simultaneously creates the bloodhound, makes
    pack attack expressible, and moves predation's yield towards the predator.
  * **Lysis on the spike's free `control[1]`** — flesh into particulate in one step rather than
    three with two lossy conversions, applying to the living, depositing where the *predator*
    stands. That last clause is the largest term in §4's arithmetic.

  **Second, not first, and §8a is why.** `FEEDING.md` recommends both partly because they deliver
  *structural matter*, which bypasses the mitochondrion cap. §8a shows bypassing the cap is not
  sufficient: the filter already delivers structural matter, and loses, because structural matter
  is not scarce either. These are still the right two items — the spatial term in §4 is the largest
  one and only lysis fixes it — but they buy nothing until item 1 or item 4 makes something scarce.

**3. Give turgor a way out other than dividing.** §6 is what forbids every patient strategy, and
there are three fixes, of which the third is the honest one:

  * The shipped genomes should read before they eat. Every `#feed` gene should gate on a membrane
    chemical reading. This is a genome fix, not an engine fix, and it should happen regardless.
  * The vacuole should be worth building. `hoarder.mm` sequesters 200 units for 216 `Q10` a tick
    and still dies of turgor at tick 1,503.
  * **Passive transport.** A cell that cannot excrete down a gradient has no way to shed solute
    except by splitting. `organelle.rs:547` records why membrane permeability is unimplemented and
    the reason is a good one — `Organelle::finished` starts every control wide open — but the
    consequence is this. Permeability and the pump are one design, and the pump is the one
    catalogue entry that is priced, buildable, and read by nothing.

**4. Make the carrying capacity economic rather than spatial.** §3 says the slide is a packing
contest with 92% of its matter unused. Either the structural monomer should be scarce enough to
bind — `the_thicket.ron` already seeds 40 a square rather than 400, on `CHEMISTRY.md` §6's measured
knee — or `soup.ron` should stop being the control condition everything is measured on. The
cheapest version of this is to reseed the default slide near the knee and re-take the acceptance
numbers.

**5. Fix the genomes that do not work.** Separately from the balance, and cheaply. Four of them
are broken in a way that makes the library misleading about its own engine:

  * **`sentinel.mm` and `stalker.mm` have never wounded anything** (§4a) — their kin check reads
    an unbadged cell as an empty square. This is the one to fix first, because both genomes are
    shipped as demonstrations of a mechanism they do not exercise.
  * **`parasite.mm` has never injected anything**, because it branches on a junction-port `OGET`
    reading that does not exist: the port falls through `CellHost::oget`'s sensor arm,
    `read_sensor` returns `None` for it, and the genome is handed a zero forever.
  * **`reflex.mm` never divides at all** — 0 births in 6,000 ticks.
  * `marble.mm` gets 53 births and ends at one cell.

Three of the four are a genome reading a sensor that cannot tell it what it needs, which is worth
noticing as a pattern rather than four bugs.

`ancestor_sloppy.mm` is **not** on that list, and it is worth saying why since the probe reports
it dying at tick 262. It differs from `ancestor.mm` by one immediate — it emits chemical 15
instead of 13, which is one bit — so it never excretes its peroxide and poisons itself. That is
the genome working exactly as designed, and 262 ticks is now the measured price of failing to
excrete.

**6. Draw the signature.** Nothing in `mm-app` reads `cells.emission`. The microscope should show
what the hunters can smell, and the wiki should be able to say that a species is loud.

**7. Measure the three that were never measured.** Each is one probe and each could cancel work
above it: whether an anchored ciliate already filter-feeds on its own current (FEEDING.md §7),
whether a toxicyst pays, and whether the hoarder's battery wins under a night long enough to need
it. `the_long_dusk.ron` exists and no result from it is recorded anywhere.

---

## 10a. What the panel says to do first

The four gates, on the library as it stands (with §4a's badge fix landed and nothing else):

```text
  viability      pass   extinct everywhere: []
  payoff         FAIL   pays nowhere (floor 400): ["hunter", "marble", "predator"]
  discrimination pass   median spread 536 (floor 100), distinct winners 3
  no sweep       pass   swept the panel: []
```

Three of four pass. That is a better starting point than §2 suggested and it narrows the work
sharply.

**The one failure is predation.** `hunter` reaches 8 of 1000 at its best and `predator` 263, and
neither clears the floor in any of five worlds — including the two built to make light scarce. So
§4's account survives contact with a wider panel: the weapon is not the problem (§9), the yield of
the kill is not the problem (§9), and now *scarcity on its own* is not the problem either. What is
left is the two mechanism items — **blood in the water and lysis** — and the panel has now removed
the objection §10.2 raised against doing them first. They were held back until something was
scarce; `dusk` and `seasons` make light scarce and predation still pays nowhere.

**The storage prediction failed, and that is the finding §10 asked for.** `hoarder.mm` is extinct
in `seasons` and in `dusk` — the two worlds its own header says it exists for. §10's dark →
storage → feast chain predicted this would be where the vacuole finally earned itself, and it does
not. Its own text says which way to read that: *"if `hoarder` still loses in `seasons`, the vacuole
is too dear or the night is too short."* On the numbers, both look implicated — `seasons` at winter
224 is below the extinction knee of §5, so it is not a lean season, it is a cull, and almost
nothing survives it (nine of eleven contenders extinct). **The panel entry wants re-cutting before
it can answer the question**: a winter that thins a population rather than ending it, which is
somewhere between 256 and 512.

**`marble` failing the payoff gate is a different thing** and probably not a balance fault: it is a
`param 255` membrane, which is 71 units of structural carbon, and `the_thicket` — where it does
best, at 340 — is the only world in the panel that makes carbon scarce. It is a genome that needs
a world nobody has written yet.

So the order the panel argues for, replacing §10's:

1. **Re-cut `seasons` so its winter thins rather than culls**, and re-run. One number in
   `shipped_panel`, and until it is right the panel cannot answer the storage question at all.
2. **Then blood in the water, and lysis.** The objection to doing them first is gone.
3. **Leave the light default alone.** §10.1 wanted the default dimmed; the panel says the library
   already contains dim worlds and that they discriminate. What was missing was measuring on them,
   not making the control condition darker.

## 11. The hoarder, and the falsification of §10's central chain

§10 argues that dimming the light is worth doing before anything else, because dark forces storage,
storage is matter, and matter is what a dying cell hands to whoever is standing on it. The prey
half of that is measured and holds — a cell holding 200 units of sugar is worth 240% of a division
to its killer, by `EAT` alone. The chain still fails, at the first link, and this is the account.

### 11.1 The hoarder could not live anywhere

`hoarder.mm` was extinct in every world in the panel, and the panel column was read as "the winter
culls" (§10a). It is not the winter. It **starves**, and never once poisons itself — in `seasons`
by tick 500 and on the control slide by tick 2,500. The world it was written for merely hastens a
death that was already happening in the soup.

```text
holds 837 units of free solute, threshold 256
  excess 581  ->  ratio 9,296  ->  turgor 2,632 Q10 a tick
  gross income                                2,400 Q10 a tick
```

The tax on what it was storing exceeded everything it earned, before a single organelle was paid
for. Setting the two equal locates the cliff exactly: **this engine can hold about 810 units before
storage costs it everything**, and the hoarder sat at 837 — over the edge by three per cent, which
is why it died on every slide rather than only the dark ones.

### 11.2 A vacuole exempts `param` units, and `param` is a `u8`

So one granule can hide at most 255 and this cell needs 510. The strategy was never mispriced; it
was **out of reach in one organelle**, and no amount of tuning a single number could have found it.
Measured on the soup:

```text
   granules   cells   exempt   free solute
          1       0        -             -
          2     591      510           656
          3      33      765           393
```

Two, not three: the third granule is cheaper turgor and a worse cell. Each costs 36 units of
structural carbon to build and 271 `Q10` a tick to carry whether or not there is anything in it.
`hoarder.mm` now builds two.

### 11.3 And storing against the dark still does not pay

The two-granule hoarder against the reference, across the panel:

```text
     world    share
      soup       71
   thicket      145
      dusk        0*   the light runs out for good
   seasons        0*   the light comes and goes
     drift      372
                       * extinct at every seed
```

**It is extinct in exactly the two worlds it exists for, and does best where storage is
irrelevant.** The arithmetic is the same shape as §11.1 and the term is different:

```text
seasons, mean light about 376 of 1024
  photosynthesis 3,200 x 376/1024 = 1,175 Q10 a tick of substrate
  respiration recovers 0.75 x     =   881 Q10 a tick of energy

  ancestor upkeep             460  ->  net  +421   lives
  hoarder  upkeep 460 + 542  1,002  ->  net  -121   dies
```

The two granules alone flip it from solvent to insolvent. **Upkeep is charged on the container and
the saving is only on the contents**, so a storage organelle costs the same empty as full — and in
the dark, where income falls, the fixed cost is what becomes unaffordable. A storage organelle that
costs more per tick than the storage saves is not a storage organelle.

### 11.4 What this does to §10

The prey half of the chain is intact and the predator half never gets to start, because **nothing
can afford to get fat in the dark**. So the cost side has to be fixed before the scarcity side does
anything, which reverses §10.1 and §10.2 for the second time.

The specific change this argues for is narrow rather than a reprice of the catalogue — §9a rules
that out — and it is a *shape* change rather than a number: the vacuole should be charged for what
it holds rather than for how large it is, or its upkeep should be small enough that an empty one is
free. Every other organelle in the catalogue does something every tick it is carried. A vacuole
does something only when there is something in it, and it is priced as though it did not.

---

## 12. Why nothing specialises, which is three different problems

The loadout every evolved cell converges on is **four organelles** — membrane, nucleus,
mitochondrion, chloroplast — and §9a establishes that price is not what holds it there. The design
intuition it is failing against is a shape rather than a number: sixteen slots is the hard cap, a
soft cap somewhere near twelve past which a body should have to be delivering something
extraordinary, and a specialist putting four or six or eight of its slots into the one thing its
niche rewards. Against that, **four of the same organelle is currently an extreme outlier**.

The catalogue splits three ways under that question, and the three want different work.

### 12.1 The two that already earn cannot be stacked, and it is not economics

`capacity_by_pathway` *sums* over every organelle of a type, so four chloroplasts fix four times as
much and four mitochondria burn four times as much. A matched specialist should earn near four
times an ancestor's income for well under four times its upkeep, because the metabolic floor and
the membrane and the nucleus are paid once either way. It is the one place in the engine where
carrying more of something is supposed to be worth more.

Measured — matched pairs, built by hand so mutation never has to find them, alone on the soup for
twelve thousand ticks:

```text
 pairs  organelles   population   starved   poisoned
     1           4          818     2,177          0
     2           6          666       737          0
     3           8            0        16        153
     4          10            0         0         32
     7          14            0         0         16
```

**They build them.** Eight, ten and fourteen organelles are all constructed successfully, so
neither the slot count, the build cost nor `max_mass` is the wall. And they die of **poison**, not
of starvation — the counters are unambiguous and the default chemical table has exactly one toxic
species.

It is peroxide. `reactive_fraction` is a fixed share of respiratory throughput, so three
mitochondria make three times the exhaust, while excretion is one `EMIT` per pass of the genome.
And it is a double squeeze, because the second half is easy to miss: **adding organelles lengthens
the genome, which lengthens the cycle, so `#grow` runs less often exactly as it needs to run more.**
The cell drowns in its own waste somewhere between two pairs and three.

Nothing had ever tried to carry three mitochondria, so nothing had ever met this. It is the
cheapest of the three problems and the most surprising: the only organelles in the engine that
genuinely pay were capped by their own exhaust rather than by their price or their yield.

#### Fixed, on the organelle the code had already named

`metabolism.rs` carried a note where the mechanism should have been — *"a cell does not decompose
its own peroxide … catalase is a lysosome, an M8 organelle this cell does not have"* — which named
the organelle and did nothing about it. A lysosome now decomposes the cell's own reactive
byproduct into inert waste, at its existing capacity, through the ledger like every other species
change. Measured, alone on the soup, `+cat` being the same body with one lysosome added:

```text
     pairs  organelles   alone   poisoned   share
         3           8       0        153       0
         4          10       0         32       0
     3+cat           9     557          0     294
     4+cat          11     502          0     230
     6+cat          15     433          0     221
     7+cat          16       0         65       0
```

Extinct becomes alive at three, four and six pairs, and poisoning goes to zero. Three properties
are why this is a mechanism rather than a knob.

**Stacking is now a coupled investment.** More mitochondria demand more catalase; both cost slots
and upkeep. The gate in `tests/balance.rs` *still failed* after this landed, until its specialist
was given a lysosome — which is the coupling working rather than a bug.

**It gives the lysosome something to earn**, where before it paid off only where carrion happened
to be lying. That puts one of §12.2's group into §12.1's, which is the shape the whole rebalance
wants.

**Senescence is untouched.** `background_damage` ages every cell whatever it is doing and no
lysosome touches it; what a catalase removes is the *extra* ageing a cell brings on itself by
respiring hard, which is what an antioxidant is for. It is kept out of the water for the reason
the note it replaces gives: free interior decay made *retaining* peroxide an advantage, because it
decayed into carbon dioxide right where photosynthesis needed it.

**The wall moves rather than disappearing, and where it moves to is the finding.** Depth seven
asks for seventeen slots against sixteen, builds sixteen and dies poisoned — so the *slot cap* is
the ceiling now, which is where the design's soft cap was always meant to sit. And the share falls
with depth all the way: 365, 350, 294, 230, 221. **A specialist survives and still loses, and
loses more the deeper it goes.** That is not upkeep — six pairs cost about 1,480 `Q10` a tick
against roughly 14,400 of income, a tenth of it. §0's first open question is what that leaves, and
it is unmeasured.

### 12.2 The ones that could earn and do not

Spike, lysosome, holdfast. Each delivers something, and what it delivers is either capped by the
mitochondrion (§4: food it cannot burn is not food) or not scarce (§8a: the filter works perfectly
and loses, because structural matter is lying about unused). This is the largest group and it is
where §10.2's lysis and blood in the water land.

### 12.3 The ones that can never earn directly, and are not broken

Chemosensor, photosensor, touch sensor, oscillator. These are *information*, and information pays
only where the world varies — no pricing of a chemosensor can make it earn on a slide with no
gradient in it.

They are not the problem they look like. §2a measured `drifter.mm` at **19‰ in the still soup and
927‰ in the drift**: the same body, the same cilia, the same sensor, and the difference is entirely
that one world has somewhere better to be. These organelles work. They have been measured on
`soup.ron`, which is the one world in the library where nothing they report is worth knowing.

**The last two sentences are withdrawn; see §14.4.** The 927‰ is not a swimmer in a world worth
swimming in — `drifter.mm`'s cilia point east into an eastward current, and what keeps its
population off the downstream wall is the *westward wake* they make. It has never produced net
forward motion on any slide. The sensors may well be fine and the cilia are not, so the two halves
of this paragraph have to be re-measured apart from one another.

### 12.4 What that means for the order of work

The three groups want, in order of cost:

1. **Let the earning pair stack.** Faster excretion, a lower `reactive_fraction`, or a peroxide
   sink that is not one `EMIT` a cycle. Any of them unblocks the only specialisation the engine
   already rewards, and none of them is a new mechanism.
2. **Measure on worlds that vary.** Already done and already paying: the panel's five worlds move
   the median contender by 536 of 1000 where the soup moves nothing (§2a). This costs no code.
3. **Give the acquisition routes something to deliver.** §10.2, unchanged, and still the largest
   piece of work.

`SPECIALIST_DEPTH` in `mm_core::balance` is this section written as a gate: a body carrying four of
one organelle must at least be able to live.

---

## 13. What this audit did not settle

* **Whether the sponge would win if carbon were scarce.** §8a shows the filter working into a
  world with 717,000 spare units of detritus. `the_thicket.ron` already seeds structural carbon
  ten times leaner; the same test on a lean slide is one run and has not been done.
* **What the right intensity is.** §9 measures 512 and 1024 and rival light; it does not sweep
  the space, and the interaction between occlusion and intensity is unexamined.
* **Whether any of this survives mutation.** Every number here is mutation-off, deliberately, so
  that a loadout is being measured rather than a population's drift away from it. Whether
  evolution finds the same ordering is a different and longer experiment.
* **The junction economy.** `junctions.transfer_cost` and the whole of SPEC §8 are outside this
  audit; `reflex_probe` covers the parts of it that have been measured.
* **Why a specialist that survives still loses.** §0's first open question, and the largest one
  outstanding. The hypothesis is §3 — a bigger cell takes more area and a share counted in cells
  falls by construction — and it has not been measured. One run comparing the specialist's mass
  and radius against the ancestor's settles it.
* **Whether the active organelles behave like the passive ones.** §9a scaled the *catalogue*, and
  `spike_upkeep`, `THRUST_ENERGY` and `HOLDFAST_ENERGY` sit outside it — the per-tick cost of
  actually using a spike, a cilium or a holdfast was never in that sweep. Everything §9a concludes
  is about organelles that cost the same whether or not they are doing anything. **§14 is the
  cilium's half of this and it turns out not to be a pricing question at all.**

---

## 14. Nothing swims, and the reason is not economic

Prompted by an observation from the microscope rather than from the ledger: on a full slide every
cell sits still and grows from its chloroplasts, and no lineage has ever been watched to move
under its own steam. §4b answered that question once — *"not the cost of swimming… motility is
nearly free, and nearly useless, because on a uniformly lit slide there is nowhere better to
be"* — and that answer is **half right and measured in the wrong place**. It costs what §4b says
it costs. It does not do what §4b assumes it does.

Everything below is `crates/mm-core/tests/motility_probe.rs`:

```
cargo test --release -p mm-core --test motility_probe -- --ignored --nocapture --test-threads=1
```

### 14.1 A cell below 192 `Q10` of thrust is driven backwards by its own wake

One sterile body, alone, still water, 60 ticks, against the same body with the fluid solver
switched off so that the only difference is the water it stirs:

```text
 cilia  param  power   thrust | with wake   no wake     lost  wake Q10
     1     20   1024       80 |      -4.5       4.6     198%      -256
     1     40   1024      160 |      -1.4      11.1     112%      -256
     1     80   1024      320 |      13.2      24.2      46%      -256
     2     80    255      158 |      -1.5      10.9     113%      -230
     2     80   1024      640 |      46.4      49.6       7%      -256
     3     80   1024      960 |      74.4      74.4       0%         0
     4     80   1024     1280 |      99.3      99.3       0%         0
     2    255   1024     2040 |     158.3     158.3       0%         0
```

**A weak swimmer swims backwards.** Not slowly — *backwards*, in the direction opposite the one
its cilia are pointing, at up to 198% of the distance it would have covered in water that did not
push back.

The mechanism is exact, and it is an **asymmetry between two paths into the same cell**:

* **Thrust reaches the cell through its velocity**, which is dragged. `DRAG_RETAIN` is a quarter,
  so a cilium producing `f` settles the cell at `4f/3`.
* **The reaction reaches the cell through the drift**, which is *not*. `step_physics` adds
  `drift` straight to the position step, deliberately and with a comment saying so — a cell
  carried by a current has a velocity of zero. So the wake acts at full strength.
* **And the wake is amplified sixteenfold before it saturates.** `impulse_retain` is 15/16, so a
  cell beating steadily in one square accumulates its own reaction to `16f`, and
  `CurrentField::apply` then clamps the water to `MAX_VELOCITY` = 256 `Q10`. **Any cilium
  producing more than 16 `Q10` saturates the water underneath it**, so the backwash is 256
  whatever the cell is doing.

Set the two equal and the threshold is a number:

```text
    4f/3 = 256   ->   f = 192 Q10 of thrust
```

Below 192 a cell moves backwards; above it, forwards. At full power that is **one cilium above
`param` 48** — and the `lost` column shows the second half, which is that a cell fast enough to
leave the square before the impulse accumulates escapes the wake entirely rather than merely
outweighing it. At three cilia the loss is not small, it is *zero*.

So motility has a **hard threshold with a reversal below it**, and neither is a price.

### 14.2 Every cilium in the library is under that threshold

The shipped genomes, assembled and run as written — one founder, empty slide, 600 ticks:

```text
            genome    cilia     thrust  dx (wake) dx (none)     cells
        drifter.mm        2        158       -8.5      121.5        6
  drifter_blind.mm        2        158       -8.5      121.5        6
        stalker.mm        2         28       -1.4        2.4        2
         reflex.mm        1          0        0.0        0.0        1
       ancestor.mm        0          0        1.4        1.4        8
```

**`drifter.mm` swims backwards.** The genome written to be M3's chemotaxis ancestor — whose header
says *"the cell swims, in whatever direction its cilia happen to be mounted, forever"* — travels
eight and a half squares the wrong way in six hundred ticks, and would have travelled a hundred
and twenty-one the right way in water that did not push back.

Two separate faults put it there, and the second is an ISA-level trap:

**A genome cannot ask for full power in one instruction.** `Template::value` is a `u8`, so `IMM`
pushes at most 255, and a control input is a `Q10` fraction of 1024. `drifter.mm`'s `#swim` gene
is `IMM 255 / ZERO / IMM <slot> / OSET`, which is **a quarter throttle**, and the comment above it
reads "Full power on both cilia". Two cilia at `param` 80 at a quarter throttle make 158 `Q10`,
which is under the 192 of §14.1 by 18%.

Reaching 1024 needs arithmetic, and the idiom is already in the tree: `stalker.mm` writes
`IMM 255 / IMM 2 / SHL` for its spike. It is a genome fix rather than an engine one, and nothing
that drives a cilium does it.

**And `Organelle::finished` starts a new organelle at `control = [1024, 0]`.** So a cilium that no
genome ever touches runs at *four times* the power one that is deliberately switched on does:
`#swim` does not turn drifter's cilia on, it turns them down. That is also why `reflex.mm` reads
thrust 0 — it is the one genome that writes a zero.

`drifter.mm` never writes `control[1]`, the mount angle, so **both its cilia are mounted due
east** and the "one +x and one +y" in its slot comment is not what it builds. `stalker.mm` does
write it — angles 0 and 12, which is the perpendicular pair drifter's comment describes — so this
is drifter's bug rather than a gap in the ISA.

The M3 acceptance test does not catch this. `cilia_actually_move_a_population_around` asserts that
some cell's position differs from the founder's start, and eight squares of backwash satisfies it
as readily as a hundred of swimming.

### 14.3 In a crowd the return on thrust is convex, so the first cilium buys nothing

§4b measured one cell alone in open water. Every cell anybody has *watched* is in a mat. The same
bodies dropped into one end of a saturated ancestor mat — 1,840 cells on a 300×24 channel, so
nothing here reaches the far wall — against the same bodies in open water, 100 ticks each:

```text
rigidity_gain 0  (the default)
 cilia  param  power   open sq |    mat sq  mat path ticks v>0   Q10/t of open
     0      0      0       0.0 |       0.7       9.6         0       0      0%
     1     80   1024      22.0 |       0.8       6.2        38      80      3%
     2     80   1024      77.4 |      27.0      32.2        49     160     34%
     3     80   1024     124.2 |      83.7      96.0        91     240     67%
     4     80   1024     165.9 |     124.0     133.5        93     320     74%
     2     80    255       2.8 |       1.1       5.9         1      38     40%
     2    255   1024     264.4 |     209.0     219.6        96     510     79%
```

**The return on investment rises with the investment: 3%, 34%, 67%, 74%.** That is the wrong shape
for anything evolution has to climb. A lineage can only add one cilium at a time, and the first
one returns three per cent of its open-water speed while costing 80 `Q10` a tick from the first
tick it exists. The regime where swimming works starts at three.

Raced as genomes rather than hand-built bodies — `ancestor.mm` with *n* extra `BUILD`s for a
cilium, against `ancestor.mm`, on the arena slide, 20,000 ticks — the gradient is not merely flat,
it is monotonically downhill:

```text
   cilia   1      2      3      4
   share   33‰    28‰    15‰    17‰
```

So on the control slide there is no path to the working regime at all: every step towards it is
punished, and the reward at the end of the four steps is a world where nothing needed the trip.

**And `rigidity_gain` is zero by default**, which is the second half of this and the one that is
plainly a mistake rather than a trade. `neighbours::firmness` returns 0 unconditionally when the
gain is zero, so the membrane and the turgor a genome pays for in matter and upkeep buy *nothing*
in the contact solver: every cell is maximally limp, `CONTACT_FRICTION` takes three quarters of
its sliding speed every tick, and `REST_SPEED` pins it outright below `Q10/24`. The code's own
comment says exactly what that costs — *"getting into a crowd, through it and out again is
exactly the manoeuvre a soft cell cannot do"* — and then the parameter that would let a cell buy
its way out is set to zero everywhere but `the_marbles.ron` and `the_thicket.ron`. Turning it up:

```text
   two cilia at param 80, full power, in the same mat
   rigidity_gain       0    1024   16384
   of open water     34%     30%     72%
   ticks v>0          49      62     100
```

At the high gain the pinning stops entirely — the cell has some velocity on all hundred ticks
instead of half of them — and a two-cilium body keeps 72% of its open-water speed instead of 34%.
The first cilium is still worth 8%, so firmness lifts the curve without unbending it: the
convexity of §14.3 is the wake of §14.1 and the crowd is on top of it.

### 14.4 And on the one slide where cilia pay, they are working as an anchor

§2a has `drifter.mm` at 19‰ in the still soup and **927‰ in the drift**, which reads as a swimmer
finally earning its keep. It is not. `the_drift.ron` runs a uniform current east at 128 `Q10`;
every cilium in the library is mounted due east, *with* it. Sixteen founders of `drifter.mm` with
one immediate changed — the value `#swim` writes to both cilia — 20,000 ticks:

```text
     power   thrust     wake    cells   mean x    at x>80
         0        0     +128       48       94         48
        64       40      -87      672       69        214
       128       80     -256     1141       53        217
       255      158     -256      964       52        179
      1024      640     -256       28       94         28
```

The channel's downstream wall is at x=90. **At zero power all 48 survivors are piled against it,
and at full power all 28 of them are too** — a cell rowing east in an eastward current arrives
faster and dies there. The population that lives is the one at power 128, sitting at x=53 in
mid-channel, and what holds it there is the **westward wake**, which saturates at −256 and cancels
the +128 current twice over.

So the cilia are not carrying the cell anywhere. They are a holdfast made of backwash, on a slide
with no barrier to grip. The trait §2a credits with the largest swing in the panel is
station-keeping by accident, and the shipped throttle is next to the optimum by luck — 128 beats
255 by 18% and beats both extremes by twenty-four fold.

### 14.5 There is nowhere to swim to anyway, and nothing dies to make room

The slide the observation came from, run headless: `predator_introduction.ron`, sixteen
`ancestor.mm` founders, mutation as shipped.

```text
   tick   cells   births   deaths   refused  contacts moved/cell    thrust    cilia
   1000     510     1528     1034      1154       230      28.90         0        0
   4000    3456     2907     2577     92815      4016      80.73         0        0
   7000    3959      188       87    116344      6551      84.22         0        0
  10000    4135      121       54    119134      7296      85.32         0        0

what the 4135 survivors carry:
   membrane 4135   nucleus 4118   mitochondrion 4124   chloroplast 4126   photosensor 4
```

Three things, and the third is the one that matters most.

* **`thrust` is zero in every interval.** Not one cell in ten thousand ticks beat anything, and
  there is not one cilium in the final population. §9a said no fifth organelle type reaches one
  per cent at any price; this is the same finding with the type named.
* **119,134 divisions are refused per thousand ticks against 121 births.** The slide is 99.9%
  blocked, which is §3's packing ceiling arriving at three times the scale.
* **Deaths fall to 54 per thousand ticks in a population of 4,135** — a turnover of about one per
  cent per thousand ticks, with a mean age of 4,498 at tick 8,000. A saturated slide is not an
  equilibrium with vacancies being competed for. **It is a jam of near-immortal cells**, and a
  world where nothing dies is a world with no selection in it, whatever its mutation rate. That
  is what the microscope is showing: not cells choosing to sit still, but a population that has
  stopped having a history.

### 14.6 What this argues for, in order

The first two are faults rather than balance, and neither is a price.

**1. Stop a cell sitting in its own wake — done, and §14.7 is what it did.** This was the whole
of §14.1 and most of §14.2 and §14.4.

**2. Make the throttle reachable, and fix the genomes.** A genome cannot write 1024 with one
`IMM` and every genome in the library tries to. Two halves:

  * The genome fix is free and should happen regardless: `#swim` should write `IMM 4 / IMM 8 /
    SHL`, and something should write `control[1]` so that a cell with two cilia is not pointing
    both of them the same way. This is §10.5's list with a fifth entry.
  * Whether `Organelle::finished` should default a *cilium* to full power is a real question. It
    is the right default for a chloroplast, and for a cilium it means the first thing a mutation
    builds sets off east at full tilt. Worth deciding deliberately rather than inheriting.

**3. Turn `rigidity_gain` on somewhere that is not one of two scenarios.** It is built, it is
measured, it is in the state hash, and at zero it makes the membrane's contribution to the contact
model unbuyable — the same shape of fault as `light_occlusion` in §10.1. §14.3 measures what it is
worth. The `the_thicket.ron` caution in §9 applies: its settings move as a unit.

**4. Only then ask what a cilium is worth.** Every number in §4b, §9a and §12.3 about motility was
taken on bodies that could not move forward. `drifter.mm` at 19‰ in the soup and 927‰ in the drift
is not a swimmer being judged on the wrong slide, as §12.3 concluded — it is a body whose cilia
have never once produced net forward motion, scoring twice on the merits of its backwash. **That
paragraph is withdrawn and the question is open again.**

### 14.7 What was changed, and what it bought

Two changes, both in `sensing::step_physics`, and neither of them a price. They have to be read
together: either alone removes the reversal and only the pair gives a cilium both of its jobs.

**A cilium's reaction no longer accumulates.** It goes into a new `stir` layer — one entry per
square, rebuilt from the cilia every physics phase and consumed by `refresh_velocity` in the same
tick, so it is scratch on the same terms as `slip` and `crowding` and adds nothing to the state
hash. `impulse` stays what it always was and is now written only by `World::inject_impulse`. The
distinction is that `impulse` is a *disturbance* — something happened and the water is still
moving — while `stir` is a *machine*: cilia are beating here now, and when they stop it stops.

That matters twice. A wake that accumulates to sixteen times one tick's injection saturates at
`MAX_VELOCITY` however small the cilium, so every wake was the same size and none could be
attributed to what made it. And a saturating wake means **one ciliate stirs its square as hard as
a hundred do** — which is precisely the wrong shape for a colony.

**And a cell is not carried by its own wake.** Its drift is moved toward zero by no more than the
thrust it is itself producing, and only against the direction it is thrusting, so it can never
gain from the correction — a cell facing into a current can cancel exactly as much of it as it is
generating and no more, and holding station in a river still costs the full thrust of swimming up
it. The cap is exact because `stir` does not accumulate.

The wake stays **under** the cell, which is the point: `slip` is read at the cell's own square and
`ecology::captured` charges a filter on `slip`, so the same beating that moves a ciliate is the
current it feeds on. Depositing it astern also removes the reversal and was tried first; it buys a
clean swimmer at the price of the pump, and measured, an anchored ciliate's `slip` went to zero.

Measured, one sterile body, still water, 60 ticks — `no wake` being the same body with the fluid
solver switched off:

```text
 cilia  param  power   thrust | with wake   no wake     lost   water under
     1     20   1024       80 |       6.1       6.1       0%          -80
     1     40   1024      160 |      12.4      12.4       0%         -160
     1     80   1024      320 |      24.7      24.7       0%         -256
     2     80    255      158 |      12.1      12.1       0%         -158
     2     80   1024      640 |      49.6      49.6       0%         -256
     3     80   1024      960 |      74.4      74.4       0%         -256
     2    255   1024     2040 |     158.3     158.3       0%         -256
```

**Zero loss at every power, and displacement exactly linear in thrust** — against 198% loss and a
reversal at the bottom of the old table. The last column is the pump still working. The shipped
swimmer settles it: `drifter.mm` goes from **−8.5 squares in 600 ticks to +121.5**, which is
exactly what it does in water it has not stirred.

In a saturated mat the crowd's own tax is unchanged and still convex — 24%, 60%, 69%, 74% of
open-water speed for one to four cilia at the default `rigidity_gain` of zero — so §14.3 stands
and the first cilium is still the weak step. What has changed is that two cilia now cover half a
square a tick *inside a full slide*, where before they covered a fiftieth.

### 14.8 And the genome side, which was two wrong constants

The engine fix leaves `drifter.mm` running its engines at a quarter throttle, because a control
input is a `Q10` fraction of 1024 and `IMM` cannot push more than 255. Both drifters now shift for
it — `ONE / IMM 10 / SHL`, which is `stalker.mm`'s idiom for its spike, and the two files stay
byte-identical in length so the blind control is still a control.

The second constant was not written at all. `Organelle::finished` leaves `control[1]` at zero,
zero is due east, and neither drifter ever set it — so **both cilia pushed the same way** and the
slot comments describing one +x and one +y described a cell the genome did not build. That is not
cosmetic: a cell with one axis cannot steer whatever it reads, so the four instructions M3 has
been waiting for had nowhere to go. `stalker.mm` had it right all along and says why — two
perpendicular mounts make steering two independent scalars rather than an arctangent nobody can
compute in this instruction set.

```text
            genome    cilia     thrust  dx, 600 ticks
        drifter.mm        2        158           -8.5     before §14.7
        drifter.mm        2        158         +121.5     engine fixed
        drifter.mm        2        640         +245.8     and the genome
```

The last row is one cilium's worth of *x*; the other is now pushing north, which is the point.

One thing this deliberately does not touch. `stalker.mm`'s `#hunt` writes the metabolic-glow
gradient straight to `control[0]` rather than a literal, so it is a proportional controller and
its 144 `Q10` on a flat slide is the signal being small rather than a ceiling being hit. Whether
it wants a gain is a separate question from this one, and it is unmeasured.

Still open, and cheap:

* **`rigidity_gain`.** Item 3 above, unchanged.
* **What a cilium is now worth**, which is §14.6 item 4 and is the whole reason for the exercise.
  Every motility number in this document was taken on a body that could not move forward.
* **What a lysosome should cost now that it does two jobs.** It digests carrion and decomposes
  peroxide on one capacity and one upkeep. That is defensible — one machine, two substrates — but
  it was priced when it did one of them.
