# The zoo, and what it is waiting for

Why the slide holds one strategy instead of a dozen, what would have to be true for the rest of
them to live, and the order to do it in.

Read `docs/ECONOMY.md` first, and `docs/FEEDING.md` §4. This document does not re-derive them; it
puts their conclusions next to each other and argues that they add up to something neither of them
says on its own.

---

## 0. Where this stands, for whoever comes back to it

### What is settled

- **The economy is differentiated already.** Four pathways, each naming its own substrate, oxidant
  and waste, and per-chemical energy yields that actually pay: lipid at 1536 against sugar's 1024,
  read through `Metabolism::latent_of` and asserted by `metabolism`'s own tests. M10.3 did this
  deliberately and it works. A cell that burns the richer substrate really does earn more.
- **The world is not differentiated at all.** `light_occlusion` is zero in
  `MetabolicRates::default`, so light is a non-rival good: a chloroplast reads the value at its
  centre square and does not decrement it. Nothing shades anything but a barrier.
- **That zero is real and it is not the binding constraint.** §2 has the mechanism and it holds: a
  world whose energy input scales with population has no energy ceiling. §5 then measured what
  switching it on actually does, and the answer is the opposite of what this document first claimed
  — see §5.1. The first draft of §1 and §2 argued light rivalry was the keystone. It is not, and the
  experiment that was put first precisely to test that is what found out.
- **Heterotrophy is a cost centre, not an income, and that is the binding constraint.** Every
  archetype in the library carries a chloroplast; `gross` income is a flat 2,400 across all fourteen
  shipped genomes and only the `upkeep` column distinguishes them. So any change that thins the
  light kills the highest-upkeep loadout first, and that is always the heterotroph. §5.2.
- **The vent is the existence proof.** `the_vent` and `the_black_smoker` run at autotroph zero and
  scavenger 716 and 672 permille *today*, with no changes. They work because light is genuinely not
  on offer, so the chloroplast everybody carries earns nothing and the lysosome is the only income
  in the world. That is what §4's third condition looks like when it is met.
- **`rulesets/rival_light.ron` exists, is measured, and is used once** — occlusion at 128 and
  `rigidity_gain` at 16384, one ruleset rather than two settings because *neither does anything on
  its own*. Its header says making light rival is "the only dial measured so far that changes which
  strategy wins", which §5 confirms in an unexpected direction: it changes it *towards the
  autotroph*. Only `the_thicket.ron` inherits it.

### What this document is for

The zoo is not a list of features to build. Almost every mechanism it needs is already in the tree,
built and measured and switched off — each for a good local reason, and never switched on together.
So this is a document about **composition and defaults**, not about invention. §6 lists the four
things that are genuinely absent, and it is a short list.

### How to run any of it

```
cargo test --release -p mm-core --test regime_matrix -- --nocapture   # the library, both ways
cargo run -p mm-cli -- run scenarios/soup.ron --ruleset rival_light   # any world, rival light
cargo test --release -p mm-core --test transport_probe -- --ignored --nocapture
cargo test --release -p mm-core --test motility_probe -- --ignored --nocapture
```

---

## 1. The finding, in one line

**Every mechanism that would make the world discriminate between strategies is defaulted to zero,
and the one dial known to change which strategy wins is enabled in one scenario out of eighteen.**

The pattern is consistent and each instance is individually defensible:

| dial | default | what it would do | why it is off |
| --- | ---: | --- | --- |
| `light_occlusion` | 0 | makes light rival — a cell is shaded by what lies over it | "zero switches it off, which is how every test written before it runs" |
| `rigidity_gain` | 0 | closes occlusion's escape route, which is *size* | same, and it does nothing without occlusion |
| `leak_fraction` | 0 | a wounded cell leaks, so a wound feeds the wounder | `FEEDING.md` §8 ranks it first of everything and it has not been done |
| `crowding_damage` | 0 | being buried costs something | on the evidence at the time |

Read one at a time, each of those is a careful decision. Read together, they are a world that
cannot tell one way of making a living from another, and the reason is §2.

---

## 2. Why "no energy ceiling" is the root of all of it

`MetabolicRates::light_occlusion`'s own doc comment states the mechanism and does not overstate it:

> the plane is prescribed from a closed form every fluid step, a chloroplast reads the value at its
> centre square and does not decrement it, so nothing shades anything but a barrier and absorption
> is not competed for. **Energy input therefore scales with *population* rather than with area, and
> a slide has no energy ceiling at all.**

Every absence in the zoo follows from that one sentence, and they follow in a chain rather than
separately:

1. **No ceiling, so autotrophs are never limited.** They fill the slide. Population is bounded by
   something else — matter, space, turgor — but never by the energy coming in.
2. **So there is never surplus biomass concentrated anywhere.** A heterotroph eats what an
   autotroph could not use, and in a world with no energy ceiling there is no such thing. The niche
   is not underpriced; it does not exist.
3. **So being buried is free**, and there is no vertical structure. No mats, no biofilm, no rind
   and core. `SPEC` §17.8 measured exactly this — a dense pack has no interior — and named light as
   the reason.
4. **So nowhere is better than anywhere**, and motility never pays. `ECONOMY.md` §4b concluded
   precisely that: *"motility is nearly free, and nearly useless, because on a uniformly lit slide
   there is nowhere better to be."* Which is also why §14's discovery that **nothing can swim
   anyway** (thrust under 192 `Q10` is net negative against its own wake) has never cost anything
   visible. Two independent faults, hiding each other.
5. **And because light is free, a chloroplast is strictly dominant.** Every genome in `genomes/`
   carries one. Every "strategy" in the library is therefore respiration with extra upkeep bolted
   onto it, which is why the fitness gradient over loadouts points at *shed organelles* —
   `ECONOMY.md` §1's anti-complexity ratchet.

§1 reads that ratchet as a pricing problem and says so: *"no reprice of `OrganelleCatalogue`
can fix it, because no entry in the catalogue except the mitochondrion appears in the income
expression at all."* That is true and it is the smaller half. The larger half is that **the income
expression has no ceiling term**, and a strategy cannot be paid for being efficient in a world
where the resource it is efficient with is free.

---

## 3. The evidence already in the tree

Two independent scarcities exist in the library, and both of them produce a mixed community. The
other fifteen scenarios have neither and are monocultures. Measured at 10,000 ticks with the four
shipped strategies seeded into each, guild shares in permille of the population:

| scenario | scarcity | producers | osmotrophs |
| --- | --- | ---: | ---: |
| `the_thicket` | rival light | 753 | **240** |
| `the_lean_water` | lean nutrient | 227 | **773** |
| `soup` | none | 955 | 45 |
| `the_short_night` | none | 999 | 1 |
| *(twelve more with none)* | | ≥955 | ≤45 |

The thicket-against-soup comparison is not controlled — they are different worlds — so the
load-bearing statement is `the_thicket.ron`'s own, made by whoever built it: *"It does not work
without occlusion — at `light_occlusion: 0` the same sweep only moves `buried`."* §5 is the
controlled version, run both ways on the same worlds.

A third observation belongs here because it is the same shape. `m8_ecology`'s degenerate-optimum
acceptance test read guild columns rather than lineage shares, and the founder kit hands every
seeded cell a finished chloroplast — so the producers column sits at 955–999 permille across
fifteen of the eighteen scenarios and the test could only ever fail while measuring nothing but the
kit. **The library was reporting a monoculture that was partly an artefact and partly real, and no
instrument distinguished the two.** `mm_core::census` is that instrument now.

---

## 4. What a way of making a living needs

Three things, and a strategy is missing exactly as many of them as it is unviable:

1. **Its own income term.** Something it does that adds to `cells.energy` by a route the others do
   not have. *Present.* Pathways with distinct substrates and distinct yields.
2. **Its own binding constraint.** Something that limits it which does not limit the others
   equally. *Mostly absent.* The mitochondrion currently caps everyone identically — `gross` income
   is a flat 2,400 across all fourteen shipped genomes — so every strategy is competing on the same
   axis and the cheapest loadout wins.
3. **A place where its constraint binds loosest.** *Absent by default, available by ruleset.* This
   is what §5's matrix is for.

The three are ordered by how hard they are to add and inversely by how much they buy. The first is
done. The third is a default and a set of scenarios. The second is the real work, and §6 is what it
consists of.

---

## 5. The regime matrix

The controlled version of §3: the whole library, twice, with the same four archetypes seeded into
each world by `World::place_community`, censused by lineage rather than by loadout.

The claim under test is deliberately **not** "the heterotroph is viable". It is that the *ranking*
differs — between the two arms, and between scenarios within the rival arm. A world where one
archetype tops every column is a world with one answer, however many organelles it offers.

Eighteen scenarios, four archetypes at eight founders each, 10,000 ticks, mutation on. Shares are
permille of the living population, by descent.

```text
                                --- default rules ---          --- rival light ---
scenario                     auto  scav  hunt  filt     auto  scav  hunt  filt
soup                         501‰   54‰  110‰  333‰     701‰    0‰    3‰  295‰
photosynthesis_or_die        373‰  160‰  466‰    0‰     541‰  209‰  248‰    0‰
predator_introduction        402‰   94‰  185‰  317‰     742‰   14‰    0‰  241‰
the_long_dusk                411‰  163‰   78‰  346‰     549‰   86‰    0‰  363‰
archipelago                  393‰  151‰  181‰  274‰     413‰  197‰  192‰  196‰
archipelago_control          427‰  130‰  157‰  285‰     625‰   85‰    0‰  288‰
seasons                      396‰   14‰  545‰   43‰       1‰    0‰  998‰    0‰
the_vent                       0‰  716‰  283‰    0‰       0‰ 1000‰    0‰    0‰
the_drift                    277‰  195‰  493‰   33‰     308‰  402‰    3‰  284‰
the_black_smoker               0‰  672‰  327‰    0‰       0‰  998‰    1‰    0‰
the_thicket                  395‰  125‰  147‰  331‰     587‰   55‰    0‰  357‰
the_marbles                  501‰   54‰  110‰  333‰     701‰    0‰    3‰  295‰
the_lean_water               309‰  113‰   19‰  557‰     258‰   62‰   17‰  661‰
the_short_night              339‰  328‰    0‰  332‰     492‰  467‰    0‰   40‰
the_shallows                 515‰  472‰    4‰    7‰     982‰   17‰    0‰    0‰
the_tide                     432‰  163‰  134‰  269‰     703‰    2‰  294‰    0‰
the_scattering              1000‰    0‰    0‰    0‰    1000‰    0‰    0‰    0‰
the_slow_gyre               1000‰    0‰    0‰    0‰    1000‰    0‰    0‰    0‰

scenarios won, default rules: auto 12 scav 2 hunt 3 filt 1
scenarios won, rival light:   auto 13 scav 3 hunt 1 filt 1
```

### 5.1 Rival light does not open a heterotroph niche. It closes one.

**The hypothesis this document was built on is false**, and the experiment §7 put first is what
falsified it. Making light rival does not cap the autotroph and hand the surplus to anybody. It
moves the autotroph *up* — 501 to 701 on the soup, 402 to 742 on the predator slide, 515 to 982 on
the shallows — and it takes the hunter out almost everywhere: three scenarios won becomes one, and
the hunter's share goes to **zero** on six worlds where it previously held between 78 and 185
permille.

Even `the_thicket`, the one scenario written for these settings, gets *less* diverse under them:
auto 395 → 587, scav 125 → 55, hunt 147 → 0.

### 5.2 Why, and it is the same fact §6.1 is about

**All four archetypes are autotrophs with side-jobs.** Every one of them carries a chloroplast, and
`ECONOMY.md` §1's table is the proof: `gross` income is a flat 2,400 for all fourteen shipped
genomes, and what distinguishes them is only the `upkeep` column — the scavenger's lysosome at 568,
the sentinel's spike and stomach and touch sensor at 897, against the ancestor's 460.

So cutting light income does not redistribute anything. It thins every cell's margin by the same
proportion, and **the loadout with the highest upkeep dies first.** The heterotrophs have the
highest upkeep, because their feeding organelles are pure cost sitting on top of the same
photosynthetic income as everybody else.

Light rivalry cannot help a heterotroph while heterotrophy is a cost centre rather than an income.
That is not an argument against making light rival — §2's mechanism is real, and a world whose
energy input scales with population still has no ceiling. It is an argument about **order**, and it
reverses §7's.

### 5.3 What the matrix found that was not being looked for

The default column is better than the guild-census reading in §3 suggested, and the difference is
the instrument. Guild columns said fifteen of eighteen were monocultures. Lineage shares say the
autotroph wins twelve and **six worlds already have a different winner**:

- **`the_vent` and `the_black_smoker`: the autotroph is at zero and the scavenger holds 716 and 672
  permille.** These are functioning heterotroph worlds, today, with no changes at all. What makes
  them work is the thing §4 asks for and nothing else in the library has — light is genuinely not on
  offer, so the chloroplast everybody carries earns nothing and the lysosome is the only income in
  the world. **The vent is the existence proof.**
- `photosynthesis_or_die` (hunt 466), `seasons` (hunt 545) and `the_drift` (hunt 493) are hunter
  worlds under default rules.
- `the_lean_water` is a filter-feeder world (filt 557, and 661 under rival light — the one archetype
  rival light *helps*, because a holdfast needs moving water rather than contact with other cells).

And `seasons` under rival light is the strangest row in the table: auto 396 → 1, hunt 545 → 998. A
near-total hunter monoculture. Unexplained, and worth its own investigation.

`soup` and `the_marbles` return byte-identical numbers in both arms, which confirms separately that
the marble scenario's physics overrides do not touch the ecology at 10,000 ticks.

---

## 6. What is genuinely missing, not merely switched off

Four things. Two of them were found by trying to write a predator and failing.

### 6.1 Heterotroph throughput is out of reach in one organelle

A lysosome's digestion capacity is `param × throttle` (`ecology.rs`, the scan), and `param` is a
`u8`. So one lysosome delivers at most 255 `Q10` a tick of carrion-turned-substrate, against a
mitochondrion at param 50 that wants 3,200 and a chloroplast at param 60 that supplies 3,840.

Measured directly: on a pitch-dark slide thick with carrion, `scavenger.mm` grew from four cells to
twelve and then died, and thinning the larder from 400 per square to 50 barely moved the result
(twelve against nine). **Supply is not the constraint; conversion is.** The ancestor control, which
has no lysosome, died immediately — so carrion was genuinely what fed the others. The route works.
The rate cannot be lived on.

This is the same shape as `ECONOMY.md` §11.2's hoarder: *"The strategy was not mispriced; it was
out of reach in one organelle, and no amount of tuning a single number could have found it."*

### 6.2 Engulfment leaves no carrion, so an engulfer gets bricks and no bread

Engulfment was built to satisfy both of `FEEDING.md` §4's conditions at once — the body arrives
inside the predator, and it arrives as *structure*, which steps around the mitochondrion's
conversion cap. It does both. What it also does is set `cells.mass[j] = 0`: the victim is absorbed
whole and **leaves no carrion behind**.

So an engulfer cannot digest its own kill. It receives structural carbon, which is building
material, and no substrate, which is the only thing a mitochondrion burns. Engulfment and
scavenging do not compose, and the mechanism that solved ownership removed the energy route in the
same stroke. `genomes/engulfer.mm` is the demonstration: it builds exactly the body it is written
for — mass 147, six organelles, by tick 100 — sheds its own chloroplast to pay for the rest by tick
300, and is dead by 700. Starved, not poisoned; peroxide never exceeds 11 and damage stays at zero.

**The fix is internal digestion, and it is one mechanism for four problems.** Engulf, and the body
enters the vacuole as a *held carrion store*; the lysosome digests that store internally over many
ticks into substrate. That is a phagosome, it is what a real phagocyte does, and it fixes:

- ownership — the meal is inside, where nothing else can reach it;
- the substrate route — internal carrion becomes burnable, so an engulfer can eat;
- composition — engulfment and the lysosome finally work together instead of excluding each other;
- the peak-rate cap of §6.1 — digestion is spread over time from a store, so the throughput a
  lysosome needs at any instant is small.

It also gives the vacuole a second reason to be large, which is the first time size has bought
anything but a bill.

### 6.2a It was built, it works, and the engulfer still dies

Both halves landed. A lysosome now draws from the interior before the square, and engulfment hands
over the victim's cytoplasm as itself, its minerals as themselves, and its body as carrion inside
the eater. `engulf.rs::a_swallowed_cell_hands_over_its_cytoplasm` holds the mechanism to it.

**It did not save `engulfer.mm`.** Seeded into `predator_introduction.ron` against 24 ancestors,
20,000 ticks, with and without a rival scavenger:

```text
  with a scavenger      engulfer  0 0 0 0 0 0 0 0    extinct by tick 2,500
  without one           engulfer  0 0 0 0 0 0 0 0    extinct by tick 2,500
  sentinel, same slide  sentinel  187 203 226 197 182 130 93 68    held
```

Which is what the solo measurement already implied and nobody read carefully enough: it dies at
about tick 700 **of its own upkeep, before it has eaten anything at all.** A food route is no use
to a cell that cannot afford to stay alive long enough to use it.

So §6.1 and §6.2 are discharged and the wall behind them is exactly where §6.4 said it was. The
order of work in §7 stands, with step 2's prediction now answered in the negative: cells that *can*
eat still cannot live, because six organelles is unaffordable. The next lever is not another
feeding mechanism. It is §6.4 — either the specialisation ceiling, or multicellularity as the
cheaper way round it.

### 6.3 Nothing swims

`ECONOMY.md` §14.1: a cell below 192 `Q10` of thrust is driven *backwards* by its own wake. One
cilium at param 20 produces thrust 80 and a net displacement of **−4.5** against +4.6 with the
fluid solver off. Every cilium in the library is under the threshold.

This blocks every strategy whose constraint is *encounter*: the hunter, the chemotroph following a
plume, anything migrating between light zones. It is currently invisible because of §2.4 — there is
nowhere better to be, so nothing has wanted to move. Fixing it before fixing §2 would be fixing a
capability nothing has a use for.

`stalker.mm` is the cautionary case and it is worth reading. It is the only genome with its senses
wired to its thrusters, and it is the *worst* of the four armed genomes: seeded into the food web it
is extinct by tick 5,000 where blind `predator.mm` at least reaches 79 cells. Its own header says
why — the signature is "a homing sense and not a searching one" with an `em_range` of six squares,
and `Spread` puts founders some fifty squares apart, so it reads a gradient of exactly zero and sits
perfectly still while the blind genomes wander into something. **Eyes with no search behind them are
a liability.** What that wants is a run-and-tumble: wander while the gradient is zero, steer when it
is not. Nothing in `genomes/` does that.

### 6.4 Nothing specialises, and multicellularity is the cheaper way round

`ECONOMY.md` §12.1, measured with matched pairs built by hand so mutation never had to find them:

```text
 pairs  organelles   population   starved   poisoned
     1           4          818     2,177          0
     2           6          666       737          0
     3           8            0        16        153
     4          10            0         0         32
```

Eight organelles is fatal. Every interesting body needs more than six: an engulfer needs the vacuole
*and* the mass, a hunter needs the spike and the stomach and the senses, a filter feeder needs the
holdfast and somewhere to put what it catches.

**Multicellularity is the cheaper route around this than raising the ceiling**, and it is already
built. Junctions exist, `JXFER` moves matter and energy between joined cells, and two cells split
the organelle budget between them — so a spiker joined to a digester is two four-organelle bodies
doing what one eight-organelle body cannot. It also solves §6.2 socially rather than mechanically:
the wound is made by one cell and digested by its partner before the carrion can diffuse to anyone
else. That is a real answer to `FEEDING.md` §4 that does not need §12.1 fixed at all.

---

## 7. The order of work, and why this order

**This order was wrong in the first draft and §5 corrected it.** The draft put light rivalry first,
on the argument that it was the keystone and could invalidate everything below it. It went first, it
did invalidate something, and what it invalidated was itself. The corrected order follows from §5.2:
nothing about the *environment* can help a heterotroph until heterotrophy is an income.

**1. Give heterotrophy a real income, by internal digestion (§6.2).** Engulf into a held carrion
store in the vacuole; digest it internally over many ticks. One mechanism, four problems — ownership,
the substrate route, engulf/lysosome composition, and §6.1's peak-rate cap. This is now first
because §5 showed every environmental dial is neutral-to-harmful while a heterotroph's only real
income is its chloroplast.

**2. Then re-run the regime matrix.** The same experiment, unchanged, against cells that can
actually eat. The prediction to check is specific: the six worlds that already have a non-autotroph
winner should grow in number, and the hunter's zeros under rival light should come back. If they do
not, §6.2 was not the constraint either and this document needs a third revision.

**3. Then re-open light rivalry.** §2's mechanism is real and a world with no energy ceiling is
still wrong; it simply cannot be the *first* change. Once heterotrophs earn from food, thinning the
light should redistribute rather than uniformly cull — which is the thing §5.2 says it cannot do
today.

**4. Make the regime matrix an acceptance test.** A `Census` per (scenario × archetype), asserting
that no archetype tops every column. This is what `TrophicMix::is_monoculture` was reaching for and
could not express, and it is the statement the product actually wants: *the same physics, a
different answer in a different world.* It is deliberately after 1–3, because an acceptance test
written against today's numbers would lock in the world §5 just measured.

**5. Then swimming (§6.3), then multicellularity (§6.4).** Both are capabilities that pay nothing
until there is somewhere better to be and something worth specialising for.

**And take the vent seriously as the template.** §5.3's finding is that the two worlds which already
work are the two where the dominant strategy's income is *absent* rather than merely contested. That
is a stronger and cheaper lever than rivalry: a scenario that removes light entirely gets a
heterotroph world for free, today. Whether that generalises — a world that removes the *oxidant* and
gets fermenters, one that removes fixed nitrogen and gets diazotrophs — is untested and is the
cheapest experiment on this list.

**5. The flux counter, which is a product feature and not test infrastructure.**
`Ledger::convert(from, to, amount)` is the single chokepoint through which all nineteen
transmutation sites pass, and it currently throws the pair away and keeps a scalar. Keeping the
directed pair makes the carbon and nitrogen cycles *legible* — "the carbon in this water has been
through a body three times" is a sentence the microscope can put on the screen, and it is how a
fishtank teaches somebody what a carbon cycle is. It also makes the cycle testable at all, which
`nitrogen.rs`'s own `the_leak_is_off_until_somebody_has_measured_the_loop` is waiting for.

---

## 8. The guardrail

There is no fitness function in this codebase and there must never be one, so **no test here may
assert that a strategy is good.** The assertion is always comparative and always about place:

> the ranking differs by regime.

A hunter that loses in a bright still pond and wins in the dark is the result. A hunter that wins
everywhere is a bug of the same kind as a hunter that loses everywhere — both mean the world has
one answer and the scenarios are decoration.

The archetypes in §5 are also **labels the analysis infers, never an enum.** `mm_core::census`
attributes cells by descent from a founding cohort and reads their guilds off their organelle
loadouts; nothing in the engine knows what a hunter is, and a lineage that stops hunting stops
counting as one without anybody updating a field.

---

## 9. What this document does not settle

- ~~Whether rival light is enough on its own.~~ **Settled, and negatively.** §5.1: it is not, and it
  is worse than neutral — the hunter loses six worlds to it. The guess recorded here in the first
  draft was that the surplus it creates might be *space* rather than *food*; the measured answer is
  that it creates no surplus at all, because it thins every cell's margin proportionally and the
  heterotroph's margin is the thinnest. §5.2.
- **Why `seasons` becomes a 998-permille hunter monoculture under rival light**, from 545 permille
  under default rules, while the autotroph collapses from 396 to 1. It is the largest single move in
  the matrix and nothing here explains it. A seasonal light cycle interacting with occlusion is the
  obvious place to look.
- **Whether removing an input generalises the way removing light does.** §7's closing note: the vent
  works because light is absent, not contested. Nobody has tried a world with no oxidant, or no
  fixed nitrogen, and those are one scenario file each.
- **What a `u8` param should be replaced with**, if §6.1 needs the cap raised rather than
  side-stepped. `ECONOMY.md` §11.2 hit the same wall from the hoarder's side and did not resolve it
  either.
- **Whether the six-organelle ceiling is worth raising at all**, given §6.4's argument that
  multicellularity is the cheaper route. Raising it and building colonies are alternatives, not a
  sequence, and nothing here chooses between them.
- **`build_matter` is a flat mass gain with almost no upkeep attached**, so mass per unit of upkeep
  runs 37 at param 255 against 145 at param 1. Eleven slots of param-1 shells would buy some 143
  units of mass for about one unit of upkeep. That is a hole in the cost model rather than a
  strategy, it was found while sizing `engulfer.mm` and deliberately not exploited, and it wants
  looking at on its own terms.
