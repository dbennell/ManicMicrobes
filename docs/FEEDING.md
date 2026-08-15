# Seven ways to eat, and where the catalogue runs out

An investigation prompted by watching an hour of *Journey to the Microcosmos* on feeding — a fair
question afterwards being *which of those can happen here?* — and the answer it produced, which
was not the one the question expected.

Short version: **the engine answers four of the seven, and the reason it cannot answer the other
three is not that it lacks organelles.** It is that nothing internalises anything, nothing can
take, and damage is private. Those are three primitives, not three organs. Meanwhile the
catalogue has one slot left, the cilium has no free control word, and a flagellum needs a slot —
so the number of organelle types stops being an abstract question in §5 and becomes a decision
with a date on it.

---

## 1. The seven ways, and what answers them

Stripped of the microscopy, the film enumerates seven livings and three pieces of plumbing.

| the film | the engine | |
| --- | --- | --- |
| **Autotrophy** — light into food | chloroplast, `waste + light → substrate + oxidant` | ✅ but light is not rival; nothing shades anything (§17.8) |
| **Osmotrophy** — absorb what is dissolved | `EAT` | ✅ and free, instant and rate-unbounded |
| **Filter feeding** — intercept what goes past | holdfast filter, `concentration × relative speed × frontal area × filter` | ✅ the most complete thing here |
| **Raptorial** — hunt, immobilise, engulf whole | spike → contact damage | ⚠️ damage only. No capture, no engulfment, no ownership of the kill |
| **Piercing-and-sucking** — Vampyrella, the tardigrade stylet, the suctorian tentacle | `JXFER` | ❌ push-only. A cell can *donate* through a junction; nothing in the ISA can *take* |
| **Histophagy** — attack the wounded, cued chemically, in groups | — | ❌ damage is visible only to its owner. No wound cue, nothing to swarm towards |
| **Saprotrophy** — the peranema in the dead gastrotrich | lysosome + carrion-as-chemical | ✅ |
| *plumbing:* **the food vacuole** — engulf, digest over time, absorb, egest | vacuole = solute sequestration, for turgor accounting only | ❌ nothing internalises anything |
| *plumbing:* **fermentation** — anaerobic, low yield, waste toxic to the producer and food to somebody else | — | ❌ all four default pathways share one oxidant and one waste |
| *plumbing:* **egestion** | `EMIT` | ✅ trivially |

Four of seven, and the fourth — filtering — arrived only at §17.6 and is the newest thing in the
tree. That is a better score than it looks, because the four are the four that pay: photosynthesis
and osmotrophy are what every shipped genome lives on, and scavenging is the only route by which
one lineage's body reaches another.

---

## 2. What is already here and idle

Before adding anything. The same exercise `CHEMISTRY.md` §2 did for chemicals, and it returns the
same kind of answer.

**The pump does nothing.** `OrganelleType::Pump` is catalogue slot 5, passes `is_implemented()`,
appears in the implemented list that `only_the_reserved_slots_are_unimplemented` asserts, is
priced by `OrganelleCatalogue::balanced`, and **no mechanism anywhere reads it**. A cell can build
one, pay for it, carry it, and get nothing. It is the one catalogue entry that lies.

**The membrane is a perfect barrier.** `control[0]`, permeability, is unimplemented — recorded in
place at `organelle.rs:547` with the reason, which is a good one: `Organelle::finished` starts
every control at full throttle, so switching permeability on means *wide open*, and every existing
ancestor would leak its sugar into the water and absorb the peroxide it just excreted. So there is
no passive transport at all, `EAT` is the only route in, and holding solute costs nothing in
matter. Turgor (§17.7) charges *energy* for holding it, which is the other half of a mechanism
whose first half was never built.

**`ChemicalDef::structural` is never read.** It is hashed into the state, and one test counts four
of them, and no mechanism consults it. Bodies are built out of `chemistry.structural`, one index,
which the default table makes carbon. Nitrogen, phosphorus and silicon are decorative — exactly as
`CHEMISTRY.md` §3 said in point 3, still true two milestones later.

**Cross-feeding is built and unexercised.** M10.3 made metabolism a set of four pathways selected
by `control[1]`, and the default set is:

| pathway | substrate | oxidant | waste |
| --- | --- | --- | --- |
| 0 | 8, sugar | 14 | 11 |
| 1 | 9, lipid | 14 | 11 |
| 2 | 10, sulphide | 14 | 11 |
| 3 | 8, sugar | 14 | 11 |

They vary in *one column*, and `the_default_set_offers_more_than_one_way_to_make_a_living` asserts
that they must — sharing the oxidant and the waste is what makes them alternatives competing for
one pool rather than four disjoint worlds. That was the right call for a default. But **no shipped
scenario authors a different set**: `the_vent.ron` is the only file that mentions pathways at all,
and it mentions them to say it does not have one ("no chemosynthesis pathway distinct from
photosynthesis"). The chain the design document calls "legal and interesting" — one pathway's
waste being another's substrate — has never been run.

**Chemical warfare is already legal.** `ChemicalDef::toxicity` is membrane damage above a
threshold, `EMIT` will place any chemical a cell holds into its square, and peroxide is toxic at
24. A cell that concentrates peroxide and dumps it on a neighbour is a toxicyst, expressed in
instructions that already exist. Nobody has ever measured whether it pays.

---

## 3. The three missing primitives

The three unanswered rows of §1 are not three organs short. They are three verbs short, and each
verb serves more than one of them.

**Nothing internalises anything.** `EAT` moves a dissolved chemical from the square into the
cytoplasm, where it is immediately available and immediately in solution. There is no
compartment, no delay, no sequence. The film's food vacuole is a *duration* — the Nassula's polka
dots are vacuoles at different stages of the same process — and duration is what makes digestion a
capacity a cell can run out of rather than a rate it always achieves.

**Nothing can take.** `JXFER` rejects a non-positive amount and `resolve_transfer` moves from
sender to receiver; the whole junction channel is a donation. This is not an oversight — it is
what makes the binding key (§8.2) coherent, because a channel that could pull would make a forced
join a robbery rather than an intrusion. But it means the entire piercing-and-sucking family is
unavailable, and that family is four of the film's set pieces.

The one thing that *can* reach into another cell is `INJECT`, which writes genome bytes. So the
engine's answer to a suctorian is a parasite that writes code making its host donate — which is
strictly better as design than a suction opcode, and is available today, and as far as this
investigation can tell has never happened in a run.

**Damage is private.** `cells.damage[i]` is read by its owner through membrane reading 4 and by
nothing else. A wounded cell looks exactly like a healthy one to every sensor in the catalogue. So
there is no blood in the water, and histophagy — which is the film's most vivid mechanism, the
Coleps arriving from a distance like piranhas — has nothing to arrive towards.

---

## 4. Why a new way to eat cannot pay by itself

This is the finding that reorders everything below it, and it is already in the tree.
`predator_probe.rs` established it and it is worth restating because it invalidates the obvious
plan:

> Carrion placed directly under a predator raised its internal sugar from 3–17 units to 64–75, and
> the energy trace was **bit-identical** — because `burn = min(mitochondrion capacity, substrate,
> oxidant)` and the capacity was already the binding term. **Food it cannot burn is not food.**

And the chain, measured in the same place: a corpse yields `carrion_fraction` = ½ of the dead
cell's mass; digestion recovers `digestion_efficiency` = ⅔ of that; the deposit lands where the
*victim* died and diffuses from there, while a lysosome digests only what is under its own centre
square. Perhaps a sixth of a corpse reaches the killer, and only if it is standing on it. The
consequence was measured too, and it is the wrong way round from the design's hope:

> A predator that gives up its chloroplast and commits to carrion, surrounded by eleven hundred
> prey, is **poorer** than one that keeps it — 168 against 203. There is no high gear.

Two things follow for everything proposed below.

1. **A new acquisition route that delivers a burnable substrate delivers nothing**, because
   conversion and not supply is the limit. A route pays only if it delivers *structural matter*
   (which is built with, not burnt, and so bypasses the mitochondrion entirely), or energy
   directly, or arrives with throughput headroom attached.
2. **Ownership of the kill matters more than the yield of the kill.** The largest term in the loss
   is spatial, not efficiency: the food lands somewhere else. Any predation fix that does not move
   the deposit to the predator is tuning a smaller number.

---

## 5. The catalogue, measured

The question that prompted this section was a flagellum, and it turns out to be a question about
control words before it is a question about types.

### The control-word ledger

Every organelle has `control[0]` and `control[1]`, sixteen types, thirty-two words. What is
actually spoken for:

| type | `control[0]` | `control[1]` |
| --- | --- | --- |
| 0 membrane | permeability — **declared, unimplemented** | investment (`metabolism.rs:620`) |
| 1 nucleus | copy fidelity | free |
| 2 mitochondrion | throttle | pathway |
| 3 chloroplast | throttle | pathway |
| 4 vacuole | free | free |
| 5 pump | free (nothing reads the organelle at all) | free |
| 6 cilium | signed power | **mount angle, 16-way** |
| 7 chemosensor | which chemical | free |
| 8 photosensor | free | free |
| 9 touch sensor | free | free |
| 10 junction port | free | free |
| 11 lysosome | throttle | pathway |
| 12 spike | signed extension | **free** |
| 13 oscillator | period | free |
| 14 holdfast | grip, and filter effort — deliberately one word | free |
| 15 reserved | — | — |

Twenty-one words free of thirty-two, which is a lot of room, and it is the room the project has
been using: `control[1]` selects a pathway on three types, `control[0]` throttles four. The
established rule, stated here because it has been followed twice without being written down:

> **A type is the right unit when the thing has its own build cost, upkeep and teardown. A control
> word is the right unit when it is the same machinery doing a different job.**

Applied honestly to the candidates this investigation raises:

- **Lysis** (§17.5, designed and unbuilt) — same machinery as a spike, something you extend to
  touch a neighbour, different job. **Spike `control[1]`.** No slot.
- **The food vacuole** — the vacuole already exists and spends neither word. **Vacuole
  `control[0]` as an engulf throttle, `control[1]` as the pathway it digests into**, matching
  every other converting organelle. No slot.
- **A pump that pumps** — slot 5 is already there and already priced. No slot; a debt.
- **A flagellum** — see below. **A slot.**

### The finding that decides it

**The cilium is the only organelle in the catalogue that has spent both control words on things
that cannot be taken away.** `control[0]` is signed power and `control[1]` is the mount angle,
sixteen directions, and the mount angle is load-bearing: it is what makes a cilium steerable and
what makes the arrangement of cilia around a body mean something. There is nothing to reclaim.

So a flagellum cannot be a mode of a cilium. It needs a catalogue slot, and there is exactly one —
`ReservedB`, number 15. Which means the flagellum can be built today **and the catalogue is then
full forever**, at which point the next thing that genuinely needs a slot has nowhere to go.

That is the decision, and it should be made deliberately rather than by spending the last slot on
whatever asked first.

---

## 6. Sixteen types, thirty-two, and the layout that makes the difference

### What widening costs

Measured against the code rather than reasoned about.

**The 4-bit type operand is not a bytestream field.** `OrganelleType::from_operand` is
`CATALOGUE[(ty as u16 as usize) % SLOT_COUNT]`, and `BUILD`'s type arrives off the stack as an
`i16`. The limit is one modulo and one array length. Widening is a constant.

**Per-cell cost: zero.** A cell holds sixteen *slots* regardless of how many *types* exist. Note
that `SLOT_COUNT` is currently doing double duty for both, which is the first thing to fix
whatever else is decided — they are independent quantities that happen to be equal.

**Per-world cost: about half a kilobyte.** `OrganelleCatalogue` holds one `OrganelleSpec` per
type, sixteen more of roughly thirty-two bytes each, once per world.

**Mutation cost: this is the real one, and it is smaller than it looks.** A copy error is a single
bit flip, not a reroll (`mutation.rs:153`) — which changes the arithmetic completely from the naive
"a 1-in-32 lottery instead of 1-in-16". Take a byte that is the immediate feeding a `BUILD`:

| | bits that change the type | reachable neighbours in one flip |
| --- | --- | --- |
| 16 types (`% 16`) | 4 of 8 | 4 of the other 15 — 27% of the space |
| 32 types (`% 32`) | 5 of 8 | 5 of the other 31 — 16% of the space |

This is exactly `CHEMISTRY.md` §5's correction, arriving in the same direction: **a larger space
makes a single mutation more local, not less.** More copy errors land on the type at all (5 of 8
rather than 4 of 8), each moving a shorter relative distance.

What *is* a genuine new cost is that bit 4 — value 16 — becomes a switch into a half of the
catalogue that is mostly reserved. Flip it on a working organelle today and nothing happens,
because the operand is reduced mod 16. Flip it under 32 types and a working organelle becomes a
no-op. That is a failure mode that does not currently exist, and it is 1 flip in 8 on every type
byte in the genome.

### The layout that turns the cost into a feature

Unless the upper sixteen are laid out so that bit 4 *means* something. `CHEMISTRY.md` §5 reached
the same conclusion for chemicals and it is the sentence worth reusing:

> What is actually valuable is that related chemicals sit adjacent, and that is a property of the
> layout rather than of the count.

So: **type `n + 16` is the same job done a different way.** The upper half is not new organs, it
is variants, paired by construction:

```text
  6  cilium          →  22  flagellum        stir vs. propel
  4  vacuole         →  20  (reserved)
  2  mitochondrion   →  18  (reserved)
  3  chloroplast     →  19  (reserved)
 12  spike           →  28  (reserved)
 14  holdfast        →  30  (reserved)
```

A cilium mutating into a flagellum in one copy error, and back, is a property worth paying for —
it makes the choice between stirring and swimming something evolution can hill-climb rather than
something it has to find. A reserved entry in the upper half then means "this organ has no variant
yet", which is a meaningful reservation rather than filler, and the no-op risk falls only on organs
that have no variant.

**Thirty-two, not twenty or twenty-four**, for the two reasons `CHEMISTRY.md` §5 gives: the index
stays a mask rather than a division, and the pairing property above only exists for a power of two.

### ISA version

An organelle-catalogue change, so a bump: 5 → 6 (hard rule 8). It renumbers nothing — 0..=15 keep
their meanings — but it **changes what an out-of-range operand means**, because the wrap changes.
`BUILD 19` meant chloroplast under ISA ≤ 5 and means type 19 under ISA 6. Mutation produces such
operands constantly, so archived genomes have to be replayed under their stamped version, which
rule 8 already requires and `genome_file.rs:233` already enforces.

This is cheaper now than later for the same reason M10.3's bump was: the archive is small.

---

## 7. The flagellum, which is what raised the question

Worth stating what a flagellum would actually *be*, because "a bigger cilium" is already
expressible — `param` scales thrust — and would not be worth a slot.

The honest difference, and it is the film's own distinction: **a cilium stirs and a flagellum
propels.** The rotifer and the Stentor beat cilia to make a vortex that brings food to a body that
is not going anywhere; a flagellate swims. In engine terms that is the split between how much of a
thrust goes into the fluid as impulse and how much goes into the body as motion:

- **Cilium** — more impulse into the square, less into the cell. Cheap to build, many of them,
  steerable through the existing 16-way mount angle, turns on the spot.
- **Flagellum** — more into the cell, less into the square. Dearer, faster, and worse at turning:
  a coarser mount angle, or one fixed at `BUILD`, so a flagellate goes fast in one direction and
  manoeuvres badly.

The reason this is worth a slot rather than a `param` threshold is what it connects to. §17.4's
capture law is `concentration × relative speed × frontal area × filter`, and relative speed is
`|v_water − v_cell|`. **A cell that stirs its own square raises the water's speed past itself
without moving**, which is ciliary filter feeding — the film's single most-shown mechanism —
falling out of a rate law that already exists, with no new mechanism at all. Anchor it with a
holdfast, which already carries the filter, and the sessile ciliary suspension feeder of §17.6 is
assembled from parts that were each built for something else. Which is the design rule working.

**Measure before building.** Cilia already inject impulse into the fluid (`step_physics`), and
`slip` is already computed there, so it is possible that a ciliated, anchored cell *already*
generates measurable slip and already filter-feeds on its own current. If it does, the flagellum's
job shrinks to being the propulsive counterpart and the case for the slot is weaker. If it does
not, the reason why is the design constraint. That is one probe, and it comes before any of this.

### The probe ran, and the answer was "no, and here is why"

`tests/ciliary_probe.rs`. Three arms, one cell each, gripping a floor with detritus in the water:
a **pump** beating two cilia at full power in still water, an **idle** control with the cilia
built and switched off, and a **current** benchmark holding station against a quarter-speed flow.

The first run said a beating cell earned 0.86 of the current arm — and then the movement column
said it had travelled **twenty-four squares in four hundred ticks**. `ecology::captured` reads
`|v_water - v_cell|` and cannot tell a sessile pump from a swimmer ram-feeding; that symmetry is
deliberate and it means a filtering number alone proves nothing about sessility. What had been
measured was ram feeding.

The reason was one asymmetry. `step_physics` advances a body by `velocity + drift`, and the
holdfast was only ever offered the `drift`: it cancelled the water's pull and left the cell's own
push untouched, so a ciliate could beat its way off its own anchor for nothing. Not a decision —
`cells.vx` simply never appeared in the block.

Grip now resists the net of the two, and the same probe reads:

| arm | filtered | moved |
| --- | --- | --- |
| pump — anchored, cilia at full power, still water | 44,630,652 | **0.00 squares** |
| current — anchored, cilia off, quarter-speed flow | 43,314,818 | 0.00 squares |
| idle — anchored, cilia off, still water | **0** | 0.00 squares |

**Pumping its own water is 1.03× holding station in a current.** So §7's first branch is the one
that obtains, once the constraint is removed: the sessile ciliary suspension feeder of §17.6 is
assembled from parts each built for something else, and **the flagellum's job does shrink to being
the propulsive counterpart**. The impulse/motion split it is really about is a number, not a
mechanism, and the cilium's two control words are both spoken for — so the honest home for it is
`param`, where short beats stir and long beats propel, rather than a catalogue slot.

The idle arm going to exactly zero is the sharper half of the result. Before, a held cell still
caught a trickle, because Brownian jitter gave it a relative speed it had not earned. Capture is a
flux; no motion, no flux.

State-hash footprint, checked by A/B: three runs move — `sponge.mm` on `archipelago`, `the_drift`
and `the_tide`, the only shipped combinations of a holdfast genome with something to grip. Every
other scenario and genome is bit-identical.

---

## 8. The recommendation

**This ranking was made before `transport_probe` ran, and the run reordered it.** Items 3 and 8
have swapped: making light rival is the foundation, and occupancy-impeded transport is now
actively not recommended. The reasoning is in SPEC §17.8's correction and in the probe's own
header; the short version is that a pack produces the thing it consumes, so impeding transport
concentrates its own exhaust where it is and *helps* the interior. Light is the only resource in
this world a cell cannot manufacture, and therefore the only one whose absence can make the middle
of a crowd a bad place to be. The rest of the list stands.

Ranked by value against cost, with the §4 finding applied — a route that delivers a burnable
substrate delivers nothing.

1. **Blood in the water.** A wounded cell leaks a fraction of its interior into its square in
   proportion to `damage`. No new type, no new opcode, no new accounting — the chemosensor already
   reads gradients and the substrate already conserves. It gives the engine histophagy and pack
   attack, and it fixes predation the right way: the leak is *structural matter*, which is built
   with rather than burnt, so it steps around the conversion cap that made carrion worthless. The
   cheapest item here and the one that most directly answers §4.

2. **Anaerobic pathways.** `Pathway` gains an oxidant ratio, `Q10`, zero meaning anaerobic; such a
   pathway runs `substrate → waste` one-for-one instead of two-for-two, still exactly balanced, at
   a much lower yield, and its waste is a *different* chemical that is another pathway's substrate
   and is toxic. That is simultaneously the film's yeast — waste toxic to its producer and treasure
   to everyone else — the first real cross-feeding chain, and a strategy that wins precisely where
   oxidant is scarce. Data plus a small change to the metabolic step.

3. **Occupancy-impeded transport.** §17.8 already names it as the missing mechanism and already
   measured its absence: a tracer through a 306-cell pack is *bit-identical* to a tracer through
   the empty slide, at every ring and every timestep, and being buried is currently the best place
   to be. Scale the fluid's edge flux by occupancy — exactly conservative, since it is still one
   number moved between two squares. This is what gives item 2 somewhere to win, and what finally
   gives motility a reason. It costs performance on the piece already furthest from its gate;
   measure before committing.

4. **Lysis, on the spike's free `control[1]`** (§17.5). Flesh into particulate in one step instead
   of three with two lossy conversions, applying to the living — theft rather than damage — and
   depositing where the *predator* is. That last clause is the largest term in §4's arithmetic. No
   slot spent.

5. **The food vacuole, as a chemical, on the vacuole's two free control words.** Engulf carrion and
   detritus into a sequestered pool that is not free solute; the lysosome converts that pool over
   time. Digestion gains a duration and stages, and a predator can carry its kill. One new per-cell
   field, which must be serialised in the same commit (rule 7). No slot spent.

6. **Split `SLOT_COUNT` into slots-per-cell and types-in-catalogue.** Costs nothing today, changes
   nothing today, and makes §6's decision a one-constant edit rather than a refactor. Do this
   whether or not the catalogue is ever widened.

7. **Widen the catalogue to 32 on the `n + 16` pairing, and put the flagellum at 22** — after the
   probe in §7 says whether ciliary self-stirring already works. ISA 5 → 6.

8. **Make light rival.** Chloroplast absorption decrements the light plane locally. Producers
   currently never compete with one another, which removes an entire axis of selection; a surface
   mat should shade what is under it. Cheap, since the plane is re-prescribed every fluid step, but
   it touches `energy_in` and so needs care against I5.

9. **A cross-feeding scenario.** Nearly free and overdue: M10.3 built the mechanism, no `.ron`
   exercises it, and the one file that mentions pathways mentions them to decline. One scenario
   where pathway A's waste is pathway B's substrate would let somebody watch the thing the
   milestone was for.

Items 1–3 are the answer to "more ways to make a living"; everything else on this list is a
*strategy*, and those three are what create places where different strategies win. That is §17.9's
argument — five limits select for five things — and it is the same argument the film makes without
meaning to: a Heliozoan and a Vampyrella and a peranema are not three solutions to one problem,
they are three problems.

---

## 9. What this investigation did not settle

Recorded so the next person does not assume it was checked.

- **Whether chemical warfare pays.** `EMIT` plus a toxic chemical is a toxicyst today. Unmeasured.
- **Whether an `INJECT` parasite that makes its host donate is reachable.** The mechanism exists;
  no run has been examined for it.
- **Whether ciliary self-stirring already produces slip.** §7. This is the probe that should come
  first, and it may cancel item 7.
- **What the pump should do**, given that turgor (§17.7) now charges for holding solute and passive
  transport still does not exist. The pump and membrane permeability are one design, not two, and
  neither should be built without the other.
