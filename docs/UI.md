# Manic Microbes — the instrument

The design for M10. Normative for the front end the way `SPEC.md` is for the simulation:
where this document and an implementation disagree, this document is what was agreed and the
implementation is what needs changing — or this document does, in the same commit, with the
reason written down.

Everything here lives in `mm-app`. Two things do not: the configuration schema (M10.2) and
the operand decoding the genome view needs (M10.3) reach into `mm-core` and `mm-asm`. Those
are called out where they occur.

---

## 1. What it is meant to look like

The reference is a microscope, and specifically the one in the Astomes footage: a slide under
an objective, a circular field, things drifting through focus, a scale bar in the corner and a
species name in the other. The instrument is diegetic — the panels are the notebook next to
the microscope, not a HUD.

Three consequences that decide arguments later:

- **The slide is the document.** Panels are transient and can all be closed; the slide cannot.
  There is no "main menu screen" with a slide behind it — the world is always there.
- **Chrome is dark and the slide is not.** The field carries the only saturated colour in the
  window. Panels are near-black with low-contrast text so the eye stays on the plate.
- **Nothing is drawn that the simulation did not produce.** Vignette, dust and defocus are the
  objective, not the world, and they live in `optics.rs` behind the same wall everything else
  in `slide.rs` lives behind. The wall is `slide.rs` and it does not move.

---

## 2. Layout

```
┌──────────────────────────────────────────────────────────────────────────────┐
│ File   Slide   Simulation   View   Tools   Help       ⏸ ▶ ½× 1× 8× max ⏭     │  menu bar
├────────────────┬───────────────────────────────────────┬─────────────────────┤
│                │                                       │                     │
│   CELL         │                                       │   METRICS           │
│   ┌─────────┐  │                                       │   population  ╱     │
│   │ ◯ ◯     │  │            THE SLIDE                  │   dissipation ╱     │
│   │   ◯     │  │                                       │   distinct    ╱     │
│   └─────────┘  │        (central viewport —            │                     │
│   energy ███   │         the only region that          │   LEGEND            │
│   mass   ███   │         pans, zooms and selects)      │   ■ carbon          │
│   damage ▁     │                                       │   ■ sugar           │
│                │                                       │   ■ peroxide        │
│   chemistry ▸  │                                       │                     │
│   machine   ▸  │                                       │   tick   1 204 887  │
│                │                                       │   cells  48 213     │
├────────────────┴───────────────────────────────────────┴─────────────────────┤
│  GENOME │ ECOLOGY                                                  ▲ collapse │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  87  GENE     %00110000        gene b                                  │  │
│  │  96  IMM      %000101          = 40                                    │  │
│  │ 103  EXPRESS  %1101            → gene d  (drift 1)                     │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
├──────────────────────────────────────────────────────────────────────────────┤
│ Cryptous mixtus · 48 213 cells · 761 genomes · 102×          ├─── 200 µm ───┤ │  status
└──────────────────────────────────────────────────────────────────────────────┘
```

Regions, and the egui construct each one is:

| region | construct | notes |
| --- | --- | --- |
| menu bar | `TopBottomPanel::top` + `menu::bar` | always present |
| transport | same bar, right-aligned | pause/run/speed/step, mirrors the keys |
| left rail | `SidePanel::left`, resizable, collapsible | cell inspector; empty state when nothing is selected |
| right rail | `SidePanel::right` | metrics, legend, world readout |
| bottom drawer | `TopBottomPanel::bottom`, tabbed, collapsible | genome and ecology — what reads the slide (§12.1; the editor and debugger are windows) |
| windows | `egui::Window`, movable, non-modal | build, parameters, editor, debugger — what you open to do a job (§12.2) |
| status bar | `TopBottomPanel::bottom`, below the drawer | selection, counts, magnification, scale bar |
| **the slide** | `CentralPanel` | whatever is left |

The important word is **`CentralPanel`**. Today every panel is a free-floating
`egui::Window` anchored to a corner, which means there is no such thing as "the slide's
rectangle" — the viewport is the whole window with panels sitting on top of it, and the only
question the input code can ask is `is_pointer_over_area()`. Giving the slide a real rect is
what makes §3 possible, and it is most of why the layout changes at all.

Tearing a panel off into a floating window — a small ⇱ in its header — is worth having on a
wide screen, and the routing in §3 already accounts for a window sitting over the viewport.
It is **not** in M10.1; the docked layout has to be lived with first, and adding a second
layout mode before the first one has been used is how both end up mediocre.

### The menu bar

```
File      MAKE
          New slide…              Ctrl+N      a lit dish, seeded, that runs
          New scenario…                       an empty stopped slide to build one on
          Reseed                  R           the same recipe again, differently
          ──
          SCENARIOS — THE RECIPE
          Scenario library…                   whatever is in scenarios/, read on selection
          Open scenario…                      .ron, by path
          Save scenario…          S           opens the scenario pane, which holds the path
                                              field and the RON that saving will write
          ──
          SLIDES — THE WORLD AS IT STANDS
          Open slide…             Ctrl+O      .mmslide, resumes where it was saved
          Save slide              Ctrl+S
          Save slide as…
          Export…                             species archive · metrics · genome · screenshot
          ──
          Quit                    Ctrl+Q

View      ON THE SLIDE                        cell I · metrics P · legend L ·
                                              genome G · ecology W    (each a checkbox)
          ──
          WINDOWS                             build B · parameters , ·
                                              editor E · debugger D   (each a checkbox)
          ──
          Interventions…                      what has been changed mid-run, and when
                                              (opens the ecology pane on that view)
          Overlays                ▸           one per chemical, 1–9
                                              (the fast path is the legend, see §4)
          Flow                    V           which way the water is going
          Optics                  O           vignette, defocus, dust
          ──
          Follow selection        T
          Reset camera            Home

Tools     Select                  F1
          Move                    F2
          Remove                  F3
          Draw barrier            F4
          Erase barrier           F5
          ──
          Build…                  B           the tools' settings, what is on the slide, and
                                              the scenario they are writing (§12.3). Not a
                                              submenu: §4.3

Help      Keys
          ISA reference
          About
```

Every keyboard shortcut already in `handle_input` appears next to its menu item, and no
shortcut exists that is not in a menu. The current build has fourteen single-key bindings and
no way to discover any of them.

**There is no Slide menu either, and the merge fixed three things rather than tidying one.**
File and Slide were both about documents — a scenario is a recipe and a slide is a state, and
both are files — so the split was never on a real seam. Between them they had `New slide…
Ctrl+N` **twice**, once live in Slide and once dead in File under an M10.2 placeholder;
`Parameters…` in Slide as well as in View, which lists it because it is a panel; and a
`Save parameters as…` which wrote the *whole scenario* under the name of one part of it, and
which the scenario pane of §9.2 now does properly and with a preview of the file.

**There is no Simulation menu, and that is deliberate.** It held Run/Pause, Step and a Speed
submenu, and the transport in the same bar — four inches to the right of it — held all four of
those as buttons. A menu that is a second copy of the controls beside it is not discoverability,
it is two places to keep in step. `Interventions…` moved to View, which is the menu that opens
panes, and the seven keys the Simulation menu was writing down (`space`, `.`, `0`, `` ` ``, `-`,
`=`, `backspace`) are now in the transport buttons' hover text — which is the rule above
satisfied by a different surface, not abandoned.

---

## 3. Input routing

The bug: scrolling the genome listing zooms the microscope. The cause is that the wheel is
read unconditionally at `main.rs:483`, while `over_ui` is computed at line 392 and then used
only for click-to-select at line 515. Panning has the same hole.

The fix is not to sprinkle `over_ui` over three more sites. It is to decide, once per frame,
which region owns the pointer, and to make that decision a pure function that can be tested:

```rust
/// Where the pointer is, and therefore who gets the next event.
pub enum Target {
    /// The slide. Wheel zooms, drag pans, click selects, tools apply.
    Slide { x: f32, y: f32 },
    /// A panel. The slide sees nothing at all.
    Panel,
    /// Outside the window, or the window is not focused.
    Nowhere,
}

pub fn route(pointer: Option<Vec2>, viewport: Rect, egui_wants_pointer: bool) -> Target
```

Rules:

- **The pointer belongs to exactly one region.** Inside `viewport` and egui does not want it →
  `Slide`. Anything else → `Panel` or `Nowhere`. There is no "both".
- **Wheel, drag and click all consult it.** Not just click. A wheel event over a scrollbar
  scrolls that scrollbar and does nothing else, which is what the whole complaint is.
- **Keyboard is routed the same way**, on `ctx.wants_keyboard_input()`. Typing `p` into the
  editor must not toggle the plot panel, and typing `2` must not toggle a chemical overlay.
  This bug is live today and is worse than the scroll one, because the editor is where you
  type the most.
- **A drag that starts on the slide stays on the slide** until the button comes up, even if
  the pointer crosses a panel. Losing the plate halfway through a pan because you brushed the
  metrics rail is worse than the thing being fixed. `route` is consulted on *press*, and the
  outcome is latched.
- **Zoom is centred on the pointer**, not on the middle of the viewport. With a real viewport
  rect this becomes possible for the first time, and it is the difference between zooming a
  microscope and operating a slider.

`route` takes the viewport rect as an argument rather than reading it from anywhere, so the
test is a table of rects and points with no Bevy, no egui context and no window.

---

## 4. Configuration, and how it relates to saving a simulation

This is the part the question was right to flag, and the current state is worse than it looks.

### Where parameters live today

| parameter group | authorable in `.ron`? | in the snapshot? |
| --- | --- | --- |
| `VmConfig` | yes, `Scenario::vm` | yes, via the embedded scenario |
| chemicals, light, current, barriers, seeding | yes | yes |
| `BiologyConfig` | **no** | yes, written field by field |
| `MetabolicRates` | **no** | yes, written field by field |
| `EcologyConfig` | **no** | yes |
| `JunctionConfig` | **no** | yes |

`World::set_biology` is the only way to set the second group, so it is reachable from code and
from nothing else. Every scenario in `scenarios/` runs on the defaults, and the balancing
numbers arrived at by measurement — `repair_energy_per_unit`, `background_damage`,
`metabolic_floor` — are compiled-in constants that no scenario can vary and no user can touch.
Meanwhile the snapshot serialises them by hand, which is why its format version has been bumped
three times in two milestones by changes that would have been free if they had lived in the
scenario.

**M10.2 hoists all four into `Scenario`.** That is a `mm-core` change, it bumps the snapshot
format, and it is the single highest-value structural change in this milestone: it turns
"expose some parameters in the UI" from a per-parameter chore into one generated form.

### The three files

Once that is done there are exactly three kinds of file, and the relationship is simple:

```
   scenarios/soup.ron          a Scenario. Every parameter. No state.
        │                      Opening one starts a world at tick 0.
        │
        ▼
   ~/slides/run-14.mmslide     a Snapshot: the Scenario it was started from,
                               the interventions since, and the world state.
                               Opening one resumes exactly where it stopped.

   $CONFIG/manic-microbes/settings.ron
                               window size, open panels, colours, keybinds,
                               last-used directory. Never reaches the simulation.
```

So: **a slide is a superset of a config, not a different thing.** The instinct in the question
was right, with one correction — they are the same *schema*, not the same *file*. "Save
parameters as…" writes the running world's `Scenario` back out as a `.ron`, which is how a
setting you tuned by hand becomes a scenario you can start ten runs from. "Open scenario"
throws away state; "Open slide" keeps it. Nothing else needs explaining to a user.

`settings.ron` is separate for one reason: opening a panel must not invalidate a run. Keep it
separate and that is true by construction.

### Changing a parameter while the world is running

This is the real design question, and it is a determinism question. `I1` says a run is a
function of `(scenario, seed)`. Raising `repair_energy_per_unit` at tick 40,000 breaks that —
the same scenario and seed no longer reproduce the same world.

Two honest options:

**(a) Refuse.** Parameters are read-only while a world exists. Editing forks a fresh world at
tick 0. Determinism is preserved trivially. Balancing becomes: change a number, restart, wait.

**(b) Record.** A parameter change appends `(tick, ParamChange)` to a list in world state. The
list is serialised with everything else and replayed on load, so `(scenario, seed,
interventions)` reproduces the run exactly.

**Take (b).** It costs a vector, a serialisation block and a replay path, and it buys the
thing this project is actually short of: the ability to nudge a running world and watch what
happens, at a stage where the honest description of the parameter set is "nowhere close to
balanced". Option (a) makes every experiment a cold start.

Two properties fall out of (b) that make it more than a compromise:

- The intervention list *is* the experiment log. "Simulation → Interventions…" shows what you
  changed and when, and it is the first thing to look at when a run did something surprising.
- It belongs on the timeline. A world's history already carries first-occurrence events and
  mass extinctions; `tick 40 000 — repair cost raised ×3` sits in that list naturally and
  makes the crash that follows it legible instead of mysterious. The wiki already frames the
  world as something that narrates itself; the hand reaching in is part of the story.

The acceptance test is exact and cheap: run with interventions, save, reload, run on — state
hash identical to the uninterrupted run. It is M1's serialisation test with a new field.

### Scenario files, and what a scenario does not say

*Built after M10.4.* Every file item was a disabled `soon()` button and the nine authored
scenarios could be run by `mm-cli` and by nothing that draws a picture, which made the setting-up
half of the loop — pick a world, tweak it, keep it — impossible inside the instrument.

It needed no new dependency. `Scenario::from_ron` and `to_ron` have been in `mm-core` since
M10.2, ISA check included, so this is `std::fs` and two calls. The interesting parts live in
`mm_app::library`, away from `main.rs` so they can be tested: where the library looks (working
directory first, then the source tree, so an installed binary gets an empty library rather than
an error), and what to say when a file is not what it claims — a missing file and a file that is
not a scenario report differently, because the first is a typo and the second means the file is
something else. One test loads every scenario in the library, which is the only thing that would
notice one going stale against an ISA bump or a renamed field.

**A `Scenario` describes a world and not its inhabitants.** There is no field naming a genome or
a founder count; `the_vent.ron` says in its own header to be run with `--genome
genomes/ancestor.mm`, and `mm-cli` takes that as a flag. Opened on its own it is a beautifully
authored empty dish. The front end seeds the New-slide founder count on top and says so in the
status bar, which is a patch rather than a fix — the fix is a seeding block in `Scenario`, and it
is a `mm-core` schema change of its own.

**Replacing a world means throwing away what was derived from the old one.** `Slide::set_world`
exists because assigning through `world_mut` does not: `history`, `flows` and `flows_filling` are
summaries of a world that no longer exists, so opening a scenario left the metrics rail plotting
the population of the slide it had just replaced. Reseed and the packing bench had the same bug
and nobody had noticed, because those keep the slide's size and a stale population curve looks
like a continuous one. What `set_world` deliberately does *not* reset is the overlays, the
optics, the camera and the flow toggle — those are the viewer's settings and belong to the
person, not to the world.

### The environment: what falls on the slide and what moves through it

*Added after M10.2, because the list below turned out to be missing the half that decides what
kind of world it is.*

`World::set_light` and `World::set_current` have existed since M8 and had **no caller in
`mm-app` at all**. Every one of the six light regimes and five current fields was reachable
only by hand-writing a `.ron` and running `mm-cli`, which cannot draw a picture. So the
microscope could show a world with a day/night cycle or a stirred beaker and could not be used
to make one, and the whole of §17.6 in `SPEC.md` — cells holding station in flowing water —
was unwatchable in the instrument built for watching.

They are now a variant picker each, the chosen variant's fields beside it, and an apply. Drafted
rather than live because `set_current` invalidates the entire prescribed velocity field, so a
strength dragged through a slider would rebuild it once per frame of the drag.

> **They were the parameter editor's first group and §9.6 moved them into the build window.** The
> grounds for putting them there were that they are configuration, which is true and is not the
> useful distinction: everything else in that editor is what the *living* half costs to run, and
> these are what kind of world it is. Setting a slide up meant crossing from the window with the
> tools in it to the window with the costs in it, for the two settings that decide the most.

**They are not on the intervention record, and that is a known hole rather than an oversight.**
`Intervention` is `{ tick, biology }`, so it cannot carry them; `set_light` and `set_current`
write straight into the running scenario and record nothing. A `.mmslide` resumes correctly —
it carries the world's state and its current scenario — but replaying the original `.ron` no
longer reproduces the run. **The barrier tools have had exactly this hole since M6**, and
neither is visible to the user unless the interface says so, which is why the apply button
says it. Closing it means `Intervention` growing beyond `BiologyConfig`, which bumps the
snapshot format and touches the diff view that reconstructs "what changed"; that is its own
piece of work and it is the next one this section owes.

### Building a slide: the toolbox and `New scenario…`

Setting an experiment up is half of what this is for, and until now the only slide you could
build in the front end was one with walls on it. Everything else — the chemistry, the sources,
who lives there — was a `.ron` and a text editor.

The tools are `Select, Move, Remove, DrawBarrier, DrawRock, EraseBarrier, Paint, Unpaint, Source,
Drain, PlaceCell`, on `F1`–`F11`. `Paint` and `Unpaint` stroke like the wall tools do, at the same
brush width; `Source` and `Drain` are dragged as rectangles, because a flux is an area and
clicking a point cannot say how big; `PlaceCell` drops founders of a named genome where you point.

#### Two tools, because there are two kinds of wall

`DrawRock` is its own tool rather than a checkbox on `DrawBarrier`, and the reason is that the two
make **opposite** kinds of wall. Nothing in the engine declares which kind a square is; the
difference is only what it holds (`docs/CHEMISTRY.md` §10). A blocked square holding no solid is
bedrock — there is nothing in it to dissolve, so it never enters the weathering loop and is
permanent. A blocked square holding solid is rock — it gives its mineral up to water that is short
of it, faster where cells are stripping that water, and opens once it is worn past
`MineralRates::wall_threshold`. A checkbox would make that look like a shade of one thing.

Until this existed, a reef that dissolves — the thing `docs/CHEMISTRY.md` §9 argues phosphorus and
silicon come out of — was something a scenario file could author with `Seeding::Rock` and a hand
could not draw. `World::set_rock` is the hand's version of that recipe, and the picture already
told the two apart: `rock.wgsl` weathers a mineral square and leaves bedrock flat, and grit is
shed along a mineral face and not a bedrock one.

Which mineral is a submenu under the tool's own row in the Tools menu, exactly as the seeding
genome is under `seed`'s — chosen once and then used, which is the kind of setting a menu can
hold. Only `chem::SOLID_CHEMICALS` are offered, because rock made of sugar is a thing the world
does not have and an interface that offers it and then refuses is worse than one that never
offers it. **Silica is the default and it is measured, not picked**: phosphate has zero diffusion
and zero advection by design, so a phosphate reef has nowhere to put what it dissolves — the water
against its face fills to saturation, the deficit the rate is a fraction *of* goes to nothing, and
the wall stalls behind its own skin. An outcrop of phosphate is a *place*; a reef of silica is a
*supply*.

**Every one of them goes through `World`, never through `substrate_mut()`.** `World::inject` and
`World::extract` put matter in and take it out through the ledger, in both currencies. The
spelling that skips them compiles, works, and puts the world's books out by exactly the energy in
whatever you painted — which the next tick's I5 check would report as a failure somewhere else
entirely. A tool is a mechanism like any other and gets no exemption.

#### The settings are a panel because a menu closes

They started in the Tools menu, which is where the brush width already lived, and it does not
work for anything you adjust *while* working: a menu shuts the moment you click the slide, so
changing a dose between two strokes was open, change, close, paint, open again. `Panel::Toolbox`
(`T`) holds the tool row, the brush width, what the chemistry tools are loaded with, the seeding
genome, and the list of sources and drains on the slide.

The list is not a convenience. A source is an area of water that behaves differently, and until
it has filled up there is nothing there to look at — so without a list, a rectangle dragged in
the wrong place could not be found again, let alone removed. They are also outlined on the slide,
in their chemical's colour, held back for a drain: an outline rather than a wash, because a
filled rectangle in the chemical's colour is indistinguishable from the chemical overlay reading
high there, and a source is a *cause* where the overlay is the *effect*.

#### An edit has to survive being saved, and three of them did not

`Slide → New scenario…` gives a stopped slide to build on — not `New slide` with no founders,
which is a petri dish, lit and seeded with three chemicals you did not ask for and cannot see.
Build it, then Save from the same menu. (§9.6 later made the sheet ask *which* three and how
much, which is the same argument carried through: a chemistry you did not choose is one you
cannot reason about, whether or not you can see it.)

For that to mean anything, everything the tools do has to reach the `Scenario`, and three things
did not:

- **Walls** lived in the substrate only. `place_barrier` never touched `scenario.barriers`, so
  drawing on a slide and saving gave back a scenario with no walls in it. Rock is recorded the
  same way but as `Seeding::Rock` per square rather than `Barrier::Square`, because what makes it
  rock is the mineral and not the blocking — a `Barrier::Square` would reopen as bedrock.
- **Painted chemistry** likewise. It is recorded as `Seeding::Spike` per square, merged, so
  leaning on the brush is one entry that grows rather than ten thousand saying the same thing.
- **A hand-placed cell** had nowhere in the format to be said at all, which is what
  `Inhabitant.at` is for.

Erasing inside an authored `Barrier::Rect` is the awkward case, because a square in the middle of
a shape cannot be removed by deleting a list entry. The list is flattened to squares and the
erased one dropped — but only when the eraser actually lands inside a shape, or every scenario
that ships a rectangle would lose it the first time anybody rubbed at an empty corner.

**A scenario is a recipe and not a state.** Saving one mid-run writes down the founders that were
placed, not the population that grew from them; for the world as it stands there is `Snapshot`.
That is the honest division and it is worth stating out loud, because "save" invites the other
reading.

What is still missing: these edits are not recorded as interventions, so a slide edited while
running does not replay from its scenario. That has been true of barrier drawing since it
existed and the new tools inherit it rather than widening it — a scenario built while *stopped*
and then played is exactly reproducible, which is the path the editor is for.

### The parameter editor

One `Parameters…` view, grouped (world · chemistry · light · VM · metabolism ·
biology · ecology · junctions), each row a labelled numeric field with its unit, its default,
and the doc comment from the struct as hover text — those comments are unusually good and
throwing them away in favour of hand-written tooltips would be a waste. Fields that changed
from the scenario's value are marked, with a per-field revert.

Applying is explicit — an `Apply` button, not live-on-keystroke — because each apply is an
intervention that goes on the record, and one per keystroke would be a useless record.

*Built as a drawer tab, not the floating window written above.* A window over the slide is one
you have to drag aside to see what your change did, which is the one thing you opened it to
look at. It is also the wrong shape: the groups are wide tables of label · value · reading, and
the drawer is where everything wide already lives. The practical push was that egui's window
frame draws no fill in this build, so over a lit slide the editor came out as ghost text with
cells swimming through it; a docked panel paints its own background. `,` toggles it, in the
same unmodified-key scheme as the other panels rather than the `Ctrl+,` above.

> **Superseded by §12.2 (M10.10): it is a window after all.** The second reason above expired —
> `skin::sheet_frame` was written for M10.7's sheets and a window paints its own background now.
> The first reason did not, and is the acknowledged cost of §12. What outweighed it is that this
> form is not about the selection and does not belong in the strip that describes it. The key is
> unchanged.

#### There are three defaults, not one

*Added with the `rules` page.* "Its default" above is a single value, and there is no such thing.
`mm_core::ruleset` resolves a parameter from three layers — the engine's own number, a named
ruleset, and whatever the scenario says — and the editor showed exactly one of them, the
scenario's, with no way to tell which layer had put it there. `rulesets/rival_light.ron` is the
whole economy `the_thicket.ron` runs on and the interface never mentioned it existed.

So the rail gets a **`rules` page**, first, holding four things:

- **the stack**, each layer with how many parameters it moves against the one above it, and the
  ruleset's own `notes` — which is usually the best thing written about it;
- **a baseline chip**, which retargets the `was` column and the warm marking on every other page.
  That is what turns *this number is 128* into *this number is 128 because the ruleset says so,
  and the engine would have said 0*;
- **what this world changes** from the selected layer, listed, each with a revert. Computed over
  `mm_core::params::fields` and not over the fifty-one rows, so a world whose only change is one
  organelle's build cost stops reporting itself unchanged;
- **keep these as a named set** — writes `rulesets/<name>.ron` from the diff. The numbers arrived
  at by dragging values here otherwise existed only in that session.

The **per-field revert** §4 asked for and never got arrives with it, and without a sixth column:
the `was` cell already prints the value to revert *to*, so the number is the button. The context
column explains the hovered row in full — its whole note, its dotted path, and what every layer
says it should be — because a tooltip is one field at a time and goes away the moment you reach
for the value.

**There is no "switch this world's ruleset" button, deliberately.** A set may name `vm` and
`chemicals` as well as `biology`, and the only thing a running world can be handed is `biology` —
`World::set_biology` is what an `Intervention` records, and widening that is the change §9.6
already has written down as its own. Adopting a set would apply part of it and silently drop the
rest. Which ruleset a *scenario* names is a different question with a safe answer, and it is
asked in the build window beside Save, where it changes the recipe rather than the state.

---

## 5. The genome view

Currently: `IMM %001111`, and the complaint is exactly right.

The `%` form is not a mistake, it is the *source* form, and it is load-bearing —
`assemble(disassemble(b)) == b` for every byte string is an M0 acceptance test, and rendering
a template as anything else would break it. `disasm.rs` is careful about this to the point of
spelling out non-canonical template letters as separate `NOP` lines rather than risk it.

So the answer is not to change the disassembler. It is that **a listing you read and a listing
you reassemble are different documents**, and the pane currently only has the second one.

### Two modes, toggled in the pane's header

**Source** — what exists today. `Line::to_source()`, byte-exact, the thing the editor round-
trips. Unchanged, still the only text the editor ever hands back.

*Changed at M10.3b: the source form itself is readable.* The premise above — that `%001111` is
the price of round-tripping — was wrong, and the assembler already said so. It takes a numeric
template operand, and a bare number takes the narrowest width that holds it, so `%001111` and
`60` assemble to identical bytes. `Line::to_readable` renders that, pinning the width as
`60:7` only where the template is padded past its value. So the editor and the pane's source
column show `IMM 60`, and `assemble(disassemble(b)) == b` still holds for every byte string —
the exhaustive two-byte sweep in `disasm.rs` now runs against both renderings.

What this does *not* recover is labels and `#names`. Those are hashed at assembly and the
strings are gone (SPEC §4.4), so a genome that evolved never had them. A number is what is
genuinely knowable about a template found in the world; the `%` form remains legal input and
is still what you want when the template is a base-pairing pattern rather than a value.

Numeric operands were previously accepted on `IMM` alone, which appears to have been an
oversight rather than a decision: a template is eight bits wherever it appears, and `%00110000`
is no more readable on an `EXPRESS` than on an `IMM`. They are now accepted on all nine
template-taking opcodes, told apart from a label by the leading character — `is_ident` requires
a letter or `_`, so nothing beginning with a digit was ever a name.

**Reading** — the same instructions with every template operand resolved to what it means:

| op | source | reading |
| --- | --- | --- |
| `IMM` | `IMM %001111` | `IMM 60` |
| `JMPF` / `JMPB` | `JMPF %101` | `JMPF → 0x2f` — or `JMPF ✗ no match` |
| `JMPZ` / `JMPNZ` | `JMPZ %11` | `JMPZ → 0x08` |
| `CALL` | `CALL %0110` | `CALL → 0x87 (gene b)` |
| `LOOPLN` | `LOOPLN %11` | `LOOPLN n = 3` |
| `GENE` | `GENE %00110000` | `GENE gene b` |
| `EXPRESS` | `EXPRESS %1101` | `EXPRESS → gene d (drift 1)` |

`GENE` and `EXPRESS` already work — `inspector::label_for` does exactly this, using
`vm::find_promoter` so the pane cannot claim a binding the VM will not make. The rest is the
same idea applied to the other seven template-taking opcodes. Two of them need a `mm-core`
function each, and both must be the VM's own search rather than a copy of it here, for the
same reason: a listing that says `→ 0x2f` when the VM goes somewhere else is worse than the
binary.

*Built at M10.3b, with four corrections to the above.*

**`LOOPLN` does not read `n = 3`.** Its template is a backward jump target, exactly like
`JMPB`'s — SPEC §4.3 2D is "if `LN != 0`, jump backward to complement". It is `LN` that counts,
and `LN` is a register set by `SETLN`, nothing encoded in the operand. Reading the template's
value as a loop count would have been a confident lie about the one thing the line does. It
reads `LOOPLN ↺ 12`.

**Offsets are decimal**, not `0x2f`. The pane's own gutter is decimal, and a target in hex
beside a gutter in decimal makes the reader convert before they can find the line.

**A miss says where it goes**, not just that it missed: `✗ falls through to 9`. Falling through
is what the VM does with an unmatched template, and it is usually the interesting part —
a jump that never fires is the common reason a lineage goes quiet.

**The reading resolves against the genome's template table, not the disassembled line.** Where
a template's letters are non-canonical `NOP` bytes, `disasm` deliberately reports no template
and spells the letters out — see the losslessness note in `disasm.rs`. The VM does not read
source, so at such a line it sees an ordinary template and jumps. Resolving the line's own
would have printed `IMM 0` for an instruction that pushes 61.

The resolution also takes the world's live `VmConfig` rather than `VmConfig::DEFAULT`, because
M10.2 made `template_search_range` and `promoter_bind_threshold` editable mid-run: narrow the
range and a call stops reaching its match, and the pane has to say so.

The immediate is the easy one and the most common: `Template::new(n, value)` sets bit *i* for
letter *i*, so `%001111` is `0b111100` = 60, and showing `60` is a pure function of the
template. That one line removes most of the binary from a typical listing on its own.

### The rest of the pane

- **Gutter markers**: `▸` on the instruction pointer, `●` on a breakpoint, a subtle band on the
  bytes being copied by `COPYB` — the inspector already knows the copy range and drawing it is
  free.
- **Follow-ip stays a checkbox**, as it is now. It is off by default when paused.
- **Edit in place.** The `edit` button already loads the pane into the editor buffer;
  `tools::rewrite_genome` already applies it to a live cell keeping the IP. What is missing is
  that this is not obvious. The pane gets an explicit `edit → apply to this cell` /
  `apply to species` pair, and applying while running is fine and stays fine — a genome
  rewritten from outside is an intervention like any other and goes on the record with them.
- **Byte column, optional.** `to_listing` already produces offsets and hex.

### Colour, in both panes (M10.3b)

Both the listing and the editor are painted from `mm_asm::highlight`, the assembler's own
lexer, through one palette — `main.rs::ink_colour`, which `token_colour` maps onto for the
editor. Two palettes would mean an opcode is one colour in the listing and another in the
editor, which is two languages as far as anyone looking at both is concerned.

Numbers are the loudest thing on a line, deliberately: the whole point of the reading form is
that `%001111` becomes `60`, and a `60` in the same grey as its surroundings has not really
arrived. `Unknown` — what the assembler would reject — takes the same colour as a jump that
never fires, so a typo is visible as you make it rather than after you press assemble.

The editor gets this *inside* the editable widget, via egui 0.35's `TextEdit::layouter`. It
was previously a read-only highlighted copy drawn beside a plain editable one, on the grounds
that egui had no styled-input widget; it does, and the two halves scrolled independently.
Diagnostics are live for the same reason — the buffer reassembles on every change, because an
error list describing the text as it was several edits ago points at line numbers that have
since moved — and each one quotes the offending line, so `3:12` does not mean counting to
line three.

---

## 6. The ecology pane

The tree of life exists. `wiki::layout` produces a proper dendrogram — rows, depths, parent
links, contiguous subtrees, with three tests asserting the layout is readable. It is then drawn
as an **indented text list inside a 140-pixel scroll area** in the corner of the wiki window
(`main.rs:1436`). That is why it does not feel like a tree of life: the data is right and the
presentation is a footnote.

The ecology pane is a drawer tab with three views, sharing one selection — click a species
anywhere and all three follow it. *A fourth was added after M10.4: the intervention log from
§4, which was a floating window and belongs beside the timeline — the timeline marks when a
parameter was changed and the log says what the change was, and two windows to read one event
is two windows.*

**Tree of life.** Painted, not listed. Horizontal axis is founding tick so the shape means
something; vertical is `TreeNode::row`. Branch thickness from peak population, colour from
trophic guild (producer green, scavenger amber, predator red, osmotroph blue — the same
colours the food web and the metrics use). Extinct lineages fade out and stop at their
extinction tick rather than running to the right edge. Living ones reach the present. Hover
for the name and the population; click for the species page. A "prune below *n* individuals"
slider, because a long run has thousands of species and most of them are noise.

*Built at M10.4, with one correction to the above:* a guild is **not** exclusive.
`TrophicMix` deliberately counts a cell in more than one column, because a mixotroph is a real
thing and forcing every cell into one box would be inventing the cell-type enum by the back
door. So `wiki::Guild` is a set and a branch's colour is the mean of whatever is in it — a
producer that also hunts comes out between green and red rather than being rounded to whichever
the code checked first. Branch weight is the **square root** of peak against the largest peak:
peaks span four orders of magnitude in a long run, and linear width draws the winner and
hairlines for everything else, which is the interesting part of a tree.

**Food web.** `foodweb::web()` already returns nodes with trophic levels and edges with
measured flows, and `Edge::is_death`/`is_recycling` already distinguish the loop. Draw it as a
layered graph, level on the vertical, edge width from flow, the recycling back-edge dashed.
The existing `foodweb_panel` renders it as text and should keep doing so behind a toggle —
the numbers are useful and a graph hides them.

**Budget.** *Added after the scenario files, because setting an experiment up and watching what
it does are one loop.* The world's books, in two halves that answer different questions.

**Energy is a flow** and the question is whether it balances: income against dissipation, with
the *net* called out, because a world that has found its level has a net of about zero — as much
leaving as heat as is arriving. Away from zero, something is still filling or still draining.
Both are rates differenced in `Sample::take` and **not** in the panel: the history samples every
`n` ticks and `n` is configurable, so anything differencing the cumulative counters itself would
report an income that changed when you changed how often you looked. `dissipation` already
worked that way; `absorbed` was added beside it.

**Matter is a stock** and the question is where it went: the total can never move (I4), so every
change is a redistribution between the fluid, the cells and the dead. A bar per chemical in its
own overlay colour, because a stacked plot of sixteen series is unreadable and a table of sixteen
numbers says nothing about proportion.

Almost none of this was new instrumentation. `mm_core::metrics::Sample` carries twenty-odd fields
into the history buffer — the whole per-chemical mass budget among them — and the metrics rail
drew seven. The rest was being measured and thrown away, including a `carrion` field whose own
doc comment reads "a number that climbs and stays climbed means nothing is eating the dead",
which is a plot description for a plot nobody had drawn.

**Timeline.** The existing timeline, full drawer width instead of 26 pixels in a corner.
Scrubbable, first-occurrence events as flags, mass extinctions as bands, **and interventions
from §4 in a distinct colour**, because "the population crashed" and "you tripled the repair
cost ninety ticks earlier" belong on the same axis.

*Built at M10.4.* "Scrubbable" needed pinning down: nothing keeps past world states, so the
cursor cannot rewind anything. It moves along the axis, selects the nearest event, and reorders
the list by distance from it — which is the honest version of the gesture rather than a promise
the engine cannot keep.

One thing this pane costs that the others do not: it is the only one that reaches into the
world, because the archive is far too large to publish every frame. Since M10.1 reaching in
makes the *simulation* stand aside, so the pane gathers everything under one lock and reuses it
for a couple of seconds. A chart of what has already happened does not become wrong because a
hundred more ticks have passed.

---

## 7. The look

### What "3D acceleration" means here

Everything already runs on the GPU — Bevy draws through wgpu and there is no software path.
The thing actually being asked for is that cells should look *rounded and uneven* rather than
like flat squares, and the answer to that is a shader, not 3D geometry.

**Recommendation: stay in 2D and fake the sphere in the fragment shader.** Real 3D — instanced
spheres, a depth buffer, lighting — costs depth sorting, transparency ordering and a great deal
of vertex work, and at the radius a cell occupies on screen it would be indistinguishable from
a shaded disc. An orthographic 2D scene with a good fragment shader gets the entire look at a
fraction of the cost, and keeps the compositing order (fields under cells under junctions under
optics) trivially controllable.

### The current renderer, and why it has to change

```
262 144  sprite entities for the chemical field (512×512), every one updated every frame
 50 000  sprite entities for cells
     ~×3 more for organelles, at organelle LOD
```

Three problems, in order of severity:

1. **The field.** A quarter of a million entities whose only job is to be one texel. This is
   the single biggest cost in the renderer and it is pure waste — Bevy extracts, sorts and
   prepares every one of them each frame.
2. **Per-cell variation kills batching.** `bevy_sprite` batches sprites sharing a texture. The
   moment a cell needs its own shape, its own noise seed and its own defocus, the batching
   assumption is gone.
3. **Per-entity overhead.** 50,000 `Transform`s extracted and propagated per frame is
   significant even before anything is drawn.

### The plan

**Fields become one texture.** A `512×512` RGBA8 texture (or a small texture array, one layer
per active chemical, blended in the shader), written from `Frame::overlays`, `Frame::light`
and `Frame::barriers` each frame and drawn as one quad under everything.

*`Frame::barriers` was added after M10.5.* Until then the renderer was never told where the
walls were: `Frame` carried cells, overlays, light, motes and junctions, and `blocked` appeared
nowhere in `mm-app` outside `tools.rs`. A barrier was visible only as an **absence** —
`set_blocked` evicts the square's chemistry to its neighbours and the light regime shadows
behind it, so a wall read as a dark patch indistinguishable from a square that merely has
nothing in it, and on an unlit slide as nothing at all. A wall is a thing and is now painted as
one: opaque, cool against a slide whose every other colour is warm light or a chemical tint,
and ignoring both the light and the overlays, because a blocked square genuinely has nothing in
it to paint. The mask is empty rather than all-false on a slide with no barriers, so such a
slide pays neither the copy nor the per-texel branch.

**Two kinds of wall, and rock has a surface.** Since `docs/CHEMISTRY.md` §10 a blocked square is
either *bedrock* — a hole in the world, permanent, holding nothing — or *rock*, made of minerals
that dissolve and deposit. The picture says which without a legend, in three ways that all come
from the same fact rather than from three decisions:

- **Colour.** A mineral square is painted the composition of what it holds, weighted by how much
  of each and normalised per square, so a silica bank reads cool and a phosphate outcrop warm,
  and a thin crust and a deep bed of the same mineral are the same colour. Darkened towards
  `BARRIER_RGB`, because a wall has to read as *solid* before it reads as made of anything: the
  first blend put a reef at about the value of the water behind it and the rock vanished.
- **Grit.** Loose grains along the exposed faces only, tinted from the same composition. This is
  where the raggedness comes from, and it has to: the barrier layer is nearest-sampled precisely
  so that a wall's edge is where the simulation put it, and softening the texture to make an
  edge look worn would draw the half-wall §1 forbids. Grains lying against a face are material
  that has come off the rock, which is what the edge of a real mineral bed looks like.
- **Surface.** `rock.wgsl` roughens the *inside* of a mineral square — grain and pitting in
  slide coordinates, so the surface stays on the rock as the view moves and runs continuously
  across the boundary between two rock squares. The silhouette is untouched; only the value of
  the colour within a square varies, which is a claim about nothing. Amplitude fades out below a
  few pixels a square, because grain finer than the pixels drawing it reads as noise that crawls.

Bedrock gets none of the three, and that falls out rather than being arranged: it holds no
mineral, so there is no composition to colour it with, nothing to shed as grit, and nothing to
weather. Smooth and plain means permanent; mottled and gritty means the water is working on it.

### Overlay visibility lives in the legend, not in a menu

Switching a chemical overlay was View ▸ Overlays ▸ item — **three levels of menu**, with the
menu itself covering the part of the plate you were trying to look at, and keys 1–9 reaching
only nine of the sixteen. Comparing two fields means seeing one, then the other, in the same
place at the same zoom, and that gesture was three levels deep and twice over.

The legend is already in the right rail, already open by default, and already knows every
chemical's colour. **Its rows are the control now**: all sixteen listed, one click each, swatch
filled when the overlay is on and outlined when it is off so the state reads down the column at
a glance. The scale stays on the rows that are on, because each layer is normalised against a
statistic of its own plane and the colours are legible and meaningless without it.

#### The ruler was flickering, not the field

The scale used to be the plane's **maximum**, and a maximum is decided by one square out of a
quarter of a million. Every overlay is normalised against it, so the whole picture's brightness
is that number's reciprocal: a cell dying and dumping its body into the square it occupied moved
the maximum, and the entire slide changed shade at once. What that looks like is the field
flickering. It is the ruler.

Measured rather than guessed, on a settled 128² slide with 3,586 cells, one reading per tick over
400 ticks (`tests/overlay_scale.rs`, which is kept as the regression test):

| statistic | worst step between ticks | mean step |
| --- | ---: | ---: |
| maximum | 43.8% | 5.36% |
| 99.9th percentile | 3.8% | 0.31% |
| 99th | 0.2% | 0.03% |
| 95th | 0.0% | 0.01% |
| **as shipped** — 99.9th, eased | **0.5%** | **0.06%** |

A 43.8% jump in the divisor is a 17% jump in the brightness of every texel, after the
square-root curve, once per tick. The numbers say plainly which of the two candidate fixes was
the real one: the top of the distribution is a thin tail — the maximum sits only 27% above the
99.9th percentile — so **the statistic was the bug and smoothing alone would only have slowed the
flash down**. `slide::SCALE_QUANTILE` is the fix and `slide::SCALE_EASE` handles what is left,
which is the honest movement: a bloom eating its way through the carbon really does change what
the scale should be, and that should arrive as a fade rather than a step.

Two consequences worth stating. **Squares above the mark saturate** — 262 of 262,144 at 512²,
which is the price of the other 99.9% holding still. And `Slide::frame` takes `&mut self` now,
for the carried exposure alone; the world is still only read, which is what M4's guarantee is
about and what `a_watched_world_matches_a_headless_one` checks.

The quantile is estimated from a 512-bucket histogram rather than a sort. `Slide::frame` runs on
the simulation thread under the lock a tick takes, so a frame that costs 20 ms is 20 ms the world
is not being stepped in; sorting a quarter of a million `i32`s per overlay per frame is not
affordable and neither is the megabyte of scratch. The added pass does not show above run-to-run
noise on `tests/frame_cost.rs`.

The comparison gesture is `[` and `]`, and it **solos** rather than toggling: whatever was on
goes off and exactly one thing comes on, so holding a key steps through the chemicals one at a
time and the picture only ever shows one of them. The cycle includes an **off** position, which
makes it a loop rather than a wall — sixteen chemicals then bare slide, and round again. Bare
slide is a reading too: it is the one that says which of what you are looking at is the cells
and which is the water. `ui::step_solo` is a pure function over the mask, so the cycle is a
table of cases rather than something you have to hold a key down to check.

**One button does all and none**, labelled with whichever it will do. It says `none` while
anything is showing, so it is always the way *out* of what you are looking at and never a
surprise that turns sixteen layers on when you meant to turn one off; it says `all` only from a
bare slide, where there is nothing to be surprised by.

`all` turns out to be a census rather than a picture. Sixteen layers each contribute a
sixteenth, so the plate is a muddy wash — but the legend beside it lists every chemical with its
scale, and the ones reading `0.0` are the ones nothing in the world is using. It answers "which
of the sixteen does this scenario actually touch" in one click, which is a question `docs/CHEMISTRY.md`
had to be written to answer.

It costs what it looks like it costs: `Frame::overlays` carries a normalised plane per switched-on
chemical, so all sixteen at 512² is 16.8 MB a frame against 1.0 MB for one. Fine for a look,
and not a thing to leave on.

The menu stays. It is the discoverable path and it lists the keys; the legend is the fast one.

### The flow overlay

`Frame` carries the velocity field, coarsened to one sample per `slide::FLOW_STRIDE` squares
each way and gathered only when the overlay is on — the full field is two `i32` planes, two
megabytes a frame at 512×512, to draw a few hundred arrows from. Each sample is the **mean** of
its block rather than one square of it, so an arrow says what the water in that block is doing.

Arrows are laid on a lattice chosen in **screen** space, not in substrate squares, so the field
reads the same at every zoom: a substrate-spaced lattice is a solid hedge at whole-slide
magnification and one arrow somewhere off the window at full. Each is anchored at its sample
point and extends *downstream* with a head at the tip, because a lattice of centred dashes is a
texture and only an offset one with a head is a direction. Length reads speed **relative to
`fluid::MAX_VELOCITY`** rather than a literal distance travelled — the literal version was built
first and is unreadable, since a gentle eighth of a square per step at six pixels to the square
is a six-pixel dash whatever the flow is doing. Nothing is drawn below a floor, so a bare
region means still water rather than an arrow too small to see.

**What the overlay shows about channels is not what one might expect, and it is the engine
being honest.** SPEC §7.4 has no pressure projection and no incompressibility solve, by an
explicit scope decision. `CurrentField` is *prescribed*: it writes a velocity at every square
from a closed form and zeroes it inside barriers. So walls stop flux across their edges — the
fluid does not cross them — but they do **not** steer or accelerate the flow around them. Put
two walls a channel's width apart and set a uniform current, and the water inside the channel
moves at exactly the speed the water outside it does. There is no venturi and there is no
wake. A channel is therefore a place cells and particulate cannot leave, and not a place the
flow is faster. Anything that wants the second needs the projection §7.4 declined to build.

**A stroke has a width**, `1..=10` and 3 by default, set by a slider in the Tools menu and
reported in the status bar beside the tool. One setting covers drawing *and* erasing, because
an eraser narrower than the pen cannot take back its own stroke. The brush is a disc rather
than a box — a box mitres every corner of a freehand curve and the eye reads those as mistakes
— and three is the default because it is the narrowest brush that makes a *diagonal* stroke
solid: at one square a diagonal run touches only at its corners, and the barrier mask treats
that as two walls with a gap between them exactly wide enough for a cell.

Drawing a wide stroke made a batched write necessary in `mm-core`. `Substrate::set_blocked`
rebuilds the fluid's edge masks, which walks the whole slide, so blocking `n` squares cost
`n × width × height` — invisible at one square per click and a stall of seconds for a ten-wide
brush dragged across 512×512. `World::set_barriers` takes the whole stroke and rebuilds once;
`set_barrier` is now one call to it, and scenario setup uses the same deferred path, where it
had been rebuilding the masks once per square of every wall it raised.

**Barriers are a second texture, and the reason is the sampler.** They were a layer of the
field texture for exactly one commit and came out visibly blurred — at high magnification a
one-square wall was a soft band several pixels wider than the square it stood on. The field is
sampled **linearly**, and that is right for what was in it: a diffusion field is a continuous
quantity sampled on a grid, so interpolating between two measured squares is a more faithful
picture of it than hard blocks are. A barrier is not a sampled continuum. It is blocked or not,
with nothing in between, so interpolating it draws half a wall — a value the simulation never
held and a thing the world does not contain, which is precisely what §1 says must never be
drawn. One sampler cannot serve both, so the barrier mask gets its own grid-sized RGBA texture,
**nearest**-sampled, alpha-composited over the field at `z = 0.25` — above the field, below the
junctions at `0.5` and the cells above them, because a wall is part of the slide and everything
alive sits on top of it. The chemistry keeps its smooth reconstruction and the wall gets the
hard edge it actually has.

The soft glow that remains beside a wall is not the wall. It is the field: `set_barrier` evicts
the square's chemistry into its neighbours, so the squares next to a wall really do hold more of
it, and that is measured data drawn smoothly rather than an artefact.

**Drawing one is a drag, not a click.** It was one square per right-click, edge-triggered, with
no stored last square and no fill between samples — so the dividing wall in `archipelago.ron`,
about a hundred and fifty squares, was a hundred and fifty separate clicks, each taking the
simulation lock and each re-running the ring eviction. The stroke now paints for as long as the
button is down and fills the gap between one frame's sample and the next with
`ui::line_squares`, because the pointer is sampled once a frame and a hand moving at any speed
skips squares — and a barrier with gaps in it is not a barrier, since the fluid and now the
cells both go straight through the holes. The right button latches its owner like the left one
does, so a wall dragged towards the edge of the plate is not abandoned when the pointer touches
a rail. A megabyte of upload per frame is nothing;
262,143 entities are not. `Frame` already carries exactly the right data — `field: Vec<f32>`
normalised, per-layer `rgb` and `scale` — so this is a change of destination, not of content.
The `sqrt` presentation curve moves into the shader where it belongs.

**Cells become one draw call.** *Built at M10.5, and not as instancing.* A custom instanced
pipeline means a `SpecializedMeshPipeline`, a custom `RenderCommand`, and your own extract,
prepare and queue systems — the part of Bevy with the least documentation and the most churn.
The same result comes out of `Material2d` with **custom vertex attributes**: one mesh carrying
the whole population, four vertices per cell, one draw call, no entities, and a supported
stable API. It costs bandwidth rather than boilerplate — about 7 MB a frame at fifty thousand
cells, a fifth of what the field texture already costs and nobody noticed.

Organelles went into the same mesh. Left as atlas sprites they were the one soft thing in a
sharp picture: a 64-pixel tile magnified to a cell at 1400× is visibly blurred while the SDF
beside it is not.

32 bytes per instance, so 50,000 cells is a 1.6 MB buffer:

```wgsl
struct Cell {
    pos:    vec2<f32>,   // slide coordinates
    radius: f32,
    colour: u32,         // packed RGBA8, from the species/loadout colour
    seed:   u32,         // hash of cell id — the wobble must be stable frame to frame
    state:  u32,         // damage, membrane integrity, selected, tracked
    depth:  f32,         // distance from the focal plane
}
```

`seed` is the one that matters for the look. A cell's irregularity must not shimmer, so it is
derived from the cell's identity and not from time — which also means a cell keeps its own
silhouette as you follow it, and a division produces a daughter that looks related but not
identical.

> **Two things about this that were wrong for a long time, and are worth knowing before touching
> it:** the material must ask for `AlphaMode2d::Blend` or the antialiased edge below is computed
> and thrown away, and every per-cell attribute must be `@interpolate(flat)` or the packed seam
> normals arrive corrupted. See `docs/OVERLAPS.md`.

**The fragment shader**, in the order the pixel is built:

```
p       = quad-local coordinate, −1..1
θ       = atan2(p.y, p.x)
wobble  = a₁·sin(3θ + φ₁) + a₂·sin(5θ + φ₂) + a₃·sin(7θ + φ₃)     // amplitudes and phases
                                                                   // hashed from `seed`
R       = 1 + wobble                       // the lumpy silhouette; three harmonics is plenty
r       = length(p)

edge    = fwidth(r) + defocus(depth)       // depth of field for free: the same smoothstep
alpha   = 1 − smoothstep(R − edge, R + edge, r)

n       = vec3(p, sqrt(max(0, 1 − r·r)))   // hemisphere normal → this is what reads as round
lambert = max(0, dot(n, L))                // L fixed upper-left, as a microscope's condenser
rim     = pow(1 − n.z, 3)                  // the bright edge that makes a cell look wet
grain   = hash(p·k + seed)                 // faint interior granularity; a flat disc reads
                                           // as a sprite, a grainy one as cytoplasm
membrane= smoothstep(R − w, R, r) · integrity   // thin bright ring, broken where damaged
```

Two things worth noting about that. First, **depth of field costs one addend.** Widening the
smoothstep is a real blur of the silhouette, so the defocused cells in the Astomes footage come
free rather than needing a post-process pass. Second, **the hemisphere normal is the whole
trick** — `sqrt(1 − r²)` plus a fixed light is what turns a disc into a ball, and it is three
instructions.

**Organelles are a second instanced pass** at `Lod::Organelles` and closer, using the ring
layout `inspector::placements` already computes, drawn as smaller shaded blobs inside the
parent with the same shader and a different parameter set. Below that LOD they are not drawn,
as now.

**Junctions** stay lines, in a third pass. There are at most tens of thousands and they are
one segment each.

Expected result: three or four draw calls for the whole slide instead of a third of a million
entities, and a look that is a large step closer to the reference.

### The particulate stipple

Detritus is drawn as a scatter of small sandy specks drifting in the water, and **it is not an
overlay.** There is no checkbox for it and it is not in the legend, because it is not an
instrument reading: the particulate is *there*, the way cells are there, and a slide with
detritus flowing down it that looked like clear water would be lying about what is on it. The
chemical overlay for detritus still exists and still answers "how much" by washing each square
in colour — that one is a reading, and it is a question a stipple should not be asked.

**A speck is not a particle**, and nothing must ever make it one. Detritus is a chemical field;
the specks are a way of drawing a scalar that reads as suspended matter rather than as fog. So
they carry no identity, cannot be clicked, selected, counted or followed, and `Frame` gives them
nothing but the lattice index that shuffles them. What the picture means is *density*: how many
specks are in a region is what the concentration says there, and which speck is which means
nothing at all. `art::speck` is a pure function of `(index, tick)` for the same reason
`optics::motes` is — the alternative is a pile of positions the renderer integrates and keeps,
which drifts out of step with the simulation the moment a frame is dropped, has to be rebuilt
whenever the camera or the slide changes, and would be the one piece of the view with a memory.

Each speck starts at its lattice point, is carried along the local velocity for `SPECK_LIFE`
ticks and then begins again, fading in and out across that life so the restart is invisible.
The velocity is the water's **geared down by the species' advection coupling**, which matters:
detritus travels at a third of the current, that lag is the whole of what makes it particulate
rather than dissolved, and specks drawn at the water's speed would say the flow carries food
three times faster than it does.

Density is set from two directions and needs both. `skip` thins the lattice when blocks crowd
together on screen, exactly as the arrows do. But `skip` can only ever make a lattice coarser,
so the lattice is a *floor* on density: at high magnification a block is wider than the pitch,
one speck in it is all there is, and the first version of this had the stipple thinning out as
the view zoomed in — precisely backwards, since magnifying should reveal more of the water and
not less of it. So `per_side` fills a block when it spreads. Only one of the pair is ever above
one, and speck indices are stable across the change so zooming adds specks to those already on
screen rather than dealing a new hand.

The gather is gated on `Substrate::present()`, the fluid solver's own per-plane emptiness flag.
Without it, every slide in existence pays a full pass over a quarter of a million squares each
published frame to discover that it has no particulate on it.

**Nothing is drawn on the far side of a wall from the water holding it**, and this is §1 rather
than a nicety. Both halves of the drawing above are approximations that ignore the barrier
layout: the lattice is four squares coarse, so a block straddling a wall scatters motes into the
half of it that is stone; and a mote is carried along one velocity sample for its whole life, so
one near a wall is carried straight over it. Seen on a slide as particulate flowing through a
sealed reef while the overlay showed the concentration piling up behind it — the solver and the
picture disagreeing about whether there was a wall there, with the solver right.

So a mote's birth position and its current position are the ends of a **path**, and every square
that path crosses must be open water or the mote is not drawn. A path and not a point: a flake is
carried for `FLECK_LIFE` ticks and clears a two-square wall between one frame and the next, so
asking only "is it standing on rock" lets it teleport across. The walk is a supercover of the grid
rather than a sampled line, because every wall it has to catch is one or two squares thick and a
sampled line steps over those.

The gather lives in `Frame::drifting` — on the simulation side of the wall in `slide.rs`, not in
the renderer — precisely so this is testable without a graphics stack. `mm-app/tests/particulate_walls.rs`
seals a room, stirs the water outside it, and asserts that nothing is ever drawn beyond the walls;
`scenarios/the_box.ron` is the same slide to look at. What stays in the renderer is the camera's
half: how crowded the lattice should be on screen, and where a mote lands in the window.

### Bevy version

*Done: 0.14.2 → 0.19, in five commits, one per version, each verified by rendering a frame.*

The advice was to do the renderer first and upgrade afterwards, so that no compile error is
ambiguous between "my rendering is wrong" and "the API moved". That held: `Material2d`,
`MeshVertexAttribute` and `specialize` are the same API at 0.19 that they were at 0.14, so the
SDF work ported for the cost of some import paths and a two-line spawn.

What the upgrade actually cost, for the next time:

| step | what broke |
| --- | --- |
| 0.15 | Bundles became required components. `SpriteBundle`→`Sprite`, with the image and atlas as *fields*; `Camera2dBundle`→`Camera2d`; `MaterialMesh2dBundle`→`Mesh2d`+`MeshMaterial2d`. Screenshots became an entity with an observer. |
| 0.16 | `ctx_mut` fallible, `Image::data` an `Option`, and **egui moved to its own schedule** — `EguiPrimaryContextPass`, because multi-pass mode may run the interface twice a frame. |
| 0.17 | The renderer split into `bevy_mesh`, `bevy_shader`, `bevy_camera`, `bevy_sprite_render`. Import paths only — plus two feature flags that compile and abort at startup. |
| 0.18 | Nothing. |
| 0.19 | egui replaced `SidePanel` and `TopBottomPanel` with one `Panel`, shown **inside a `Ui`** rather than against a `Context`. Needs `rustc` 1.95. |

The whole migration was one file. `main.rs` is the only thing in the crate that knows Bevy
exists, and all 154 `mm-app` library tests passed untouched through every step because none of
them can see a renderer. That is the wall in `slide.rs` paying for itself, and it is the
strongest argument for keeping it exactly where it is.

### Docking

**Hand-rolled with egui's own panels** — `egui::Panel` since 0.35 unified `SidePanel` and
`TopBottomPanel` — not `egui_dock`. The layout in §2 is exactly what those constructs produce natively, it adds
no dependency and no version-compatibility surface against `bevy_egui 0.29`'s pinned egui, and
it gives the central viewport rect that §3 needs. Rearrangeable tabs are worth revisiting once
the layout has been lived with; they are not worth a dependency before then.

### The simulation thread

Not a graphics change, but it is in this milestone because nothing above can be measured
without it.

`advance_simulation` and `redraw` are chained in the same `Update` schedule, so at 1× the world
advances exactly once per frame and the tick rate *is* the frame rate. That makes M4's
decoupling test ("dropping the render to 5fps does not change tick output") unfalsifiable, and
it makes any frame-budget figure a measurement of the two costs added together.

The split: the world moves to its own thread behind one mutex, and publishes a bundle into a
slot the render thread empties and the simulation refills. `Frame` was already the entire
render-side view of the world and already a plain owned value with no borrows into `World`, so
the wall in `slide.rs` is precisely where the thread boundary wants to go.

The bundle is more than a `Frame`, and the reason is worth writing down. The simulation thread
holds the lock for the duration of each tick, so **anything the render thread asks the world
for once a frame will wait up to a whole tick** — thirty milliseconds at fifty thousand cells,
which is a dropped frame every frame. So everything the always-on panels need is gathered on
the simulation side and handed over with the frame: the selected cell's reading, its species
name, the metric history, the food web, the objective's settings. The panels that are *not*
covered — wiki, editor, debugger, and the tools — do take the lock, because they are opened
deliberately to look at one thing and an occasional stutter is an honest price. Publishing
their data is the fix if that stops being true, and M10.4 owns the ecology half of it.

The presentation controls go the other way, and have the same problem in reverse: the zoom is
set on every frame the wheel moves, and it lives on `Slide` because `frame()` reads it. Those
cross as atomics and are applied by the simulation thread under the lock it is taking anyway.

The M4 guarantee gets *stronger* — the renderer stops being able to reach the simulation
because it is not on the same thread — and is still checked the same way, by hashing against a
world advanced in one go.

---

## 8. The chrome (M10.6)

§7 is about the renderer. This is about everything that is not the renderer: the panels, the
menus, the type and the colour. They are separate sections because they are separate concerns
with a hard boundary between them, and the boundary is the point of this one.

The complaint that started it: *the simulation rendering is fine, the interface looks like it
is from the 1990s.* Both halves are true, and the second one has a single cause. `main.rs` sets
no `Visuals`, no fonts, no `Style` — in six thousand lines the only styling anywhere in the
crate is three lines making one `DragValue`'s background transparent. Every dated thing in the
window is egui's default dark theme and egui's default proportional font, drawing an interface
that has never asked egui for anything. That is worth writing down because it decides the
answer to "should we replace egui": no. Nothing designed below is out of egui's reach, and the
things egui genuinely cannot do — CSS-grade layout, transitions, `:hover` rules — are not in it.

### 8.1 What this does not touch

**Nothing that draws the slide.** The renderer is finished work: the cell shader, the packing
tiers, the seam solve, the optics. It cost days and three of them are written up in
`docs/OVERLAPS.md`. So M10.6 does not open:

```
cellpipe.rs  cellmesh.rs  cell.wgsl  art.rs  optics.rs  phantom.rs  slide.rs
```

nor `main.rs`'s `redraw`, its materials, or anything reachable from them. If a chrome change
appears to need one of those files, the change is wrong. The one thing the chrome draws over
the viewport — the slide caption, the active-overlay chips, the selection ring — is egui
painting on top of a finished picture and touches none of it.

This is a boundary and not a preference. `tests/shader_probe.rs`, `tests/packing_probe.rs` and
`tests/nine_cells.rs` fix the renderer's behaviour; they must pass untouched, and if one of
them so much as needs re-recording then the boundary has been crossed.

### 8.2 The palette

Panels are near-black so the slide carries the only saturated colour in the window — which §1
already required and the build does not do. One accent, reserved for **selection and state**;
where it appears, something is on or something is chosen. It is never decoration.

| token | value | what it is |
| --- | --- | --- |
| `SLIDE` | `#08090b` | behind the viewport, and the drawer's darkest ground |
| `PANEL` | `#0e1013` | every rail and the drawer |
| `SUNK` | `#12151a` | the trough of a bar, the well of a value field |
| `RAISED` | `#1b1f25` | a hovered menu item, a pressed chip |
| `RULE` | `#23272e` | a border between regions |
| `HAIR` | `#1a1e24` | a rule inside a region |
| `INK` | `#e6e9ed` | a number that matters, a selected label |
| `BODY` | `#c6cdd5` | ordinary text |
| `LABEL` | `#79828d` | the name of a thing whose value is next to it |
| `DIM` | `#4e565f` | section headers, units, keys, anything you read second |
| `ACCENT` | `#7fa8c0` | **selection and state, and nothing else** |
| `GOOD` | `#8ab098` | income, a clean assemble, a healthy reading |
| `WARN` | `#c8a25a` | changed and not yet applied; an intervention |
| `BAD` | `#c4736e` | damage, a drain, an error, a loop that is not closing |

Chemical colours are not in this table. They come from the scenario and are the slide's, not
the interface's; the chrome shows them in swatches and never restyles them.

### 8.3 Type

Two families with two jobs, and the rule that does most of the work:

> **Every number is monospace.** Every one, everywhere — a tick count, a byte offset, a Q10
> reading, a key in a menu, a coordinate in a flux row. Prose and control labels are
> proportional. Nothing else is a judgement call.

The reason is column alignment. A rail full of readings that do not line up looks like a form
and reads like one; the same numbers in a mono column read like an instrument, and you can
compare two of them by eye without moving your head. This is the largest single change in the
section and it costs one `FontId`.

**No font is vendored.** egui already carries `Hack` for `Monospace` and `Ubuntu-Light` for
`Proportional`, and Hack is a perfectly good instrument face. Swapping the proportional side
for something with more character — Archivo, Inter — is a one-line change at `theme::FONTS`
and a ~200 KB OFL binary in the repo, which is a decision about the repo rather than about the
design, and is deliberately not being made here.

| role | size | family | colour |
| --- | --- | --- | --- |
| `Section` | 10 | mono, letterspaced, upper | `DIM` |
| `Body` | 12 | proportional | `BODY` |
| `Label` | 11 | mono | `LABEL` |
| `Value` | 11 | mono | `INK` |
| `Small` | 10.5 | proportional | `DIM` |
| `Code` | 11.5 | mono | per `inspector::Ink` |

Nothing goes below 10. egui rasterises through `ab_glyph` with no hinting, so dim mono at 9px
on a 1× display is mush; the mock's 9.5px is a browser's number and does not survive the port.

### 8.4 The row grammar

Three columns, and every quantity in the interface uses them:

```
label ······· ▁▁▁▁▁▁▁▁▁▁▁▁ ····· 89.0
LABEL/mono     bar, 5px          INK/mono, right
```

The bar is a 5px hairline in `SUNK` with the value in its own colour, and **the label is
outside it**. `bar()` today paints a 16px slab with the label inside — which is a Windows 95
progress bar, and is the single most dated object in the window. Where there is no meaningful
maximum there is no bar, just label and value; a bar against an invented full scale is a lie
told in a straight line.

### 8.5 Section headers

Small letterspaced mono caps in `DIM`, above a group, with a hairline under the group and not
over it: `CELL`, `LOADOUT`, `MACHINE`, `INTERIOR CHEMISTRY`, `OVERLAYS`, `BREAKPOINTS`,
`SOURCES AND DRAINS ON THE SLIDE`. They are the cheapest legibility in the design — a rail of
undifferentiated rows becomes four named groups for the price of four labels — and they are why
the panels read as a notebook rather than a settings dialogue.

### 8.6 The drawer has one shape

Every drawer tab is **a wide work area plus a fixed 300px context column on the right**, and
the context column holds whatever the work area cannot say about itself:

| pane | work area | context column |
| --- | --- | --- |
| genome | the listing | genes, and the diagnostics |
| ecology | the tree, web, timeline or budget | the selected species |
| toolbox | tools, settings, the flux table | why this is a panel; what a source looks like on the slide |
| parameters | the field table | *(the group list, on the left instead)* |
| editor | the buffer | diagnostics, live |
| debugger | the trace | breakpoints, and the step controls |

Five of those six left the drawer at M10.10 (§12) and **all six kept the shape**, which is the
argument for having had one: `skin::drawer_split` did not care that its `Ui` stopped being a
panel, and the width rule that drops the context column below `380 + CONTEXT_COLUMN` is the same
rule that makes a window useful at the size you dragged it to. The one thing that had to change
was a gap it was spending twice — see §12.4.

The toolbox is the one that needs it most and has it least. It is a vertical stack of a
`Slider`, a `ComboBox`, a `DragValue` and a `TextEdit` with four paragraphs of prose
interleaved *between the controls* — a narrow column in a wide space, which is the exact
failure `ui::Panel::dock` exists to prevent and which its test asserts against. The prose is
worth keeping and worth reading once; it does not belong between two settings you are trying to
compare. It moves to the column.

The split is `skin::drawer_split` and is computed in exactly one place, because writing it out
by hand got it wrong the first time: the work area took `available - CONTEXT_COLUMN` and the
column then asked for `CONTEXT_COLUMN.min(available * 0.4)` of *what was left*, which is a
fraction of a fraction, came out at 120 points, and wrapped its own headings. Below
`380 + CONTEXT_COLUMN` the column is dropped rather than squeezed — a tab that has lost its
work area is useless, one that has lost its notes is merely quieter.

All six tabs are on it.

### 8.7 The menus

The menu bar is discovery, and it is the only place the fourteen single-key bindings are
written down. Three changes:

- **A shortcut column**, right-aligned, mono, in `DIM`. `Button::shortcut_text` already does
  this and is already used; what is missing is that every item has one.
- **Group captions** — `SLIDE FILES` above the first block of File — and hairline rules
  between groups rather than egui's full-width separators.
- **Speed is a live control, not a submenu.** `Simulation ▸ Speed ▸ 1×` is three levels to
  change one thing, and the menu shuts on the click, so comparing ½× against 8× means opening
  it twice. It becomes a segmented row inline in the menu, for the same reason the toolbox is a
  panel: *a thing you adjust while watching has to stay where you can reach it.* This overrides
  the `Speed ▸` in §2, which was written before the transport existed in the bar.

A disabled item says why it is disabled, in its hover text. An item that does nothing and does
not say why is worse than an absent one.

### 8.8 Parameters says what you changed

The parameter editor is 51 fields under six collapsing headers, and its only signal that you
have edited anything is one `not applied` label at the top. It cannot tell you *which* field,
so the way to find your own edit is to open all six headers and read. Three additions:

- a **`default` column** beside the value, so a drift is visible without being remembered;
- **per-row marking** — `WARN` value, `WARN` left edge, warm row ground — and a footer that
  counts them: *2 fields changed from the scenario*;
- the **explanation as a column** rather than a tooltip. Every one of those sentences is
  already written, in `params.rs`, and is currently reachable one hover at a time.

The group list moves to a 150px rail on the left with a count per group, which pins the column
header and makes finding a field a click instead of an expand-scan-collapse.

### 8.9 The scale bar is in squares, and §2 is wrong about it

§2 sketches the status bar's scale bar as `├─── 200 µm ───┤`, and it is not drawn that way.

**Nothing anywhere says how large a substrate square is.** `SPEC.md` mentions microns once, in
prose about anoxic cores, and defines no conversion. Printing a micron figure here would invent
a physical scale for a world that has not got one and then display it as a measurement — in the
one widget whose entire job is to say honestly how big things are.

So the bar is `├────┤ 20 squares`, in the unit the simulation actually has, stepped 1 · 2 · 5 ·
10 · 20 · 50 so it is always a number somebody would say out loud. The arithmetic is
`ui::scale_bar`, which is pure and tested without a graphics stack like the rest of that module.

If a physical size for a square is ever wanted, it belongs in `SPEC.md` as a stated conversion
and this can then use it. **This is a decision made mid-implementation and it needs review.**

### 8.10 What is not taken from the design

- **The mock's slide.** Its warm blooms and vignette are painted decoration, and §1 forbids
  drawing what the simulation did not produce. The renderer already does this properly and is
  out of scope besides.
- **9.5px text and 44px drop shadows.** Browser numbers. See §8.3; shadows come down to
  something that does not look like a web modal at a 23px row height.
- **Fixed rail widths.** The rails stay resizable, as §2 has them.

### 8.11 Where the tokens live

`theme.rs`, in `mm-app`, **with no egui in it**. The palette is `[u8; 3]`, the type scale is
`f32`, and the roles are an enum — so the whole thing compiles and is tested without a graphics
stack, exactly as `ui.rs` and `slide.rs` are, and `main.rs` converts to `Color32` and `FontId`
at the boundary and nowhere else. This is not ceremony: it is what lets "the accent colour has
exactly one meaning" and "no two text roles are the same size" be tests rather than intentions.

---

## 9. Building a scenario (M10.7)

§4 already says what a scenario is, what the ten tools do, and that a scenario built while
*stopped* and then played is exactly reproducible while one edited mid-run is not. The code does
all of it. **What the interface does not do is say any of it**, and that is what this section is
for: M10.7 adds almost no mechanism, and a great deal of telling.

### 9.1 Authoring is a state, not a mode

A slide stopped at tick 0 is a recipe you are writing. The moment it has run a tick it is a
state you are watching, and the tools stop being reproducible — §4's last paragraph. Nothing in
the window says which of those you are looking at, and it is the difference between a scenario
that replays and one that does not.

So: `ui::authoring(tick, running)` — `tick == 0 && !running` — and a caption in the menu bar
saying so, in `WARN`. **A predicate and not a flag**, because a flag is a lie the first time you
press play and forget to clear it.

**The transport stays live**, which is a deliberate departure from the design. The mock greys it
out while authoring, and that is backwards: playing a stopped scenario from tick 0 is precisely
the reproducible path, so the control that starts it is the last one to disable. The caption is
the information; taking the button away is a different and wronger claim.

### 9.2 The scenario pane

A drawer tab, `scenario` (`S`), on the drawer shape of §8.6: the outline in the work area, and
**what Save will write** in the context column, as the actual RON, live.

> **§12.3 (M10.10) made it the second view of the build window**, on the same shape and with the
> same content. It was always the half of the toolbox that tells you whether the toolbox worked,
> and having to close the brush to read it was the whole argument for merging them. `S` still
> reaches it; it now opens the build window on that view rather than a tab of its own.

The design puts these in the two rails instead, replacing the cell inspector and the metrics
while authoring. That is a second layout, and §2 has already spent a milestone establishing that
the rails are *what is selected* and *what the world is doing*. A tab costs nothing, reuses
`skin::drawer_split`, and does not make the rails mean two different things depending on a mode.

The RON preview is the item worth building this pane for. "A scenario is a recipe and not a
state" is asserted in §4, asserted again in the pane's own footnote, and until you can see the
file it would write it is only ever an assertion. `library::save` already serialises; this shows
the string first.

> **And the string was four hundred and thirty-six lines.** Of which four hundred were the
> chemical table, which no scenario changes — the chip that folds it away is in this pane because
> of that, and folding a thing away is not the same as not writing it. Every file in `scenarios/`
> is sparse; `soup.ron` is fifteen lines. So a scenario opened in the microscope and saved again
> came back as something nobody could read, and, worse, stopped inheriting: a file that restates
> every parameter has nothing left for a ruleset to reach.
>
> **Save writes the delta now, and that is the default.** Only what this world changes, against
> the engine's numbers with the ruleset it names resolved into them —
> `Scenario::to_ron_sparse`. The world half through `serde`, because that half has enums and
> fixed-size arrays whose RON syntax only the derived serialiser knows; the rules half as dotted
> paths into `Scenario::set`, because a nested block cannot name one chemical without writing all
> sixteen. A `complete` chip beside the preview writes the old form, which is what a `.mmslide`
> embeds and what to use for a file that must go on meaning the same thing after somebody edits a
> ruleset.
>
> **Which ruleset the file names is a row of chips above Save**, listing the library. It is not
> applied to the running world — see §4 — but it decides what the delta is written against and
> what the file inherits when it is opened again. This is where "switch the economy" belongs,
> because a scenario is a recipe: changing what it inherits changes what it will be, not what it
> is.
>
> One thing this exposed rather than introduced: `library::load` looked for `rulesets/` beside the
> scenario and then in the *working directory* only, so a scenario saved anywhere outside the
> project tree could not find its set and would not reopen. Invisible while every saved file was
> complete and inherited nothing. It now falls back to `mm_asm::locate`, which is what answers
> the same question for `genomes/`; `mm-cli` had the identical hole and has the identical fix.

### 9.3 What is on the slide

The toolbox lists sources and drains because a source that has not filled up yet is invisible,
and without a list a rectangle in the wrong place cannot be found again (§4.3). Every word of
that argument applies to a hand-placed founder and to an authored barrier, neither of which is
listed. One table: kind, what, where, and the one number that matters to it.

### 9.4 Sheets, not submenus

`New slide…` is a submenu holding two sliders, and it closes if you look away. This is the
complaint that moved the toolbox out of the Tools menu in §4.3, still unaddressed one menu over.
Both `New…` items and the scenario library become windows that stay until dismissed, each
saying what it will destroy before it does it.

The library's window is a list beside a detail pane, and the detail is read from the `.ron` when
you pick one — a directory listing is genuinely all it knows before that. **No thumbnail and no
description field**: the design shows both, and a description means a new `Scenario` field, which
is a format change and does not belong in a chrome milestone.

### 9.5 Not in this milestone

**The preview run** (design 3b) — running a genome for 10k–500k ticks and setting two genomes'
populations side by side. Deferred deliberately. The design's own footnote concedes the problem
and CLAUDE.md is unambiguous: there is no fitness function in this codebase and there must never
be one. A person comparing two runs is not selection pressure inside the simulation, so the idea
is defensible — but a panel that headlines one number per genome and colours the winner teaches
people to read it as a score, and that is worth more thought than a milestone deadline gives it.

### 9.6 What kind of world it is

M10.7 gave the interface everything needed to author a slide *square by square* — a brush, walls,
sources, founders — and the three things that decide what kind of world it is are not properties
of any square. They were reachable as follows:

| | where it lived | what it cost to reach |
| --- | --- | --- |
| light | Parameters ▸ environment | a different window from the one you author in |
| current | Parameters ▸ environment | likewise |
| starting chemistry | **nowhere** | a text editor |

The third is the one that matters. `Seeding::Uniform` has been in the format since M1 and had no
caller in the front end at all, because the brush records a `Seeding::Spike` *per square*
(§4.3) — so washing a 270-square slide in carbon is 72,900 entries in a file whose entire job is
to be read by a person. "Start this world short of carbon" is the first thing anybody wants from
a scenario, because scarcity is what there is to compete over, and it was the one thing the
editor could not say.

**So `build` gets a third view, `world`,** on the drawer shape of §8.6: the light regime, the
current, and a table of what one square of water starts with. `World::set_uniform_seeding` is the
mechanism — a *level* and not a dose, so lowering it takes matter off the slide through the
ledger in both currencies, exactly as `extract` does. It replaces everything the recipe said
about that chemical, hand-painted spikes included, because a wash with spikes still listed
underneath is a file that reopens as a different slide than the one that was saved.

It joins `set_light`, `set_current` and the barrier tools in **not being on the intervention
record**, and the apply button says so. That hole is still one piece of work, still owed, and now
owed by four callers rather than three.

#### The sheet asks, rather than describing

`New scenario…` offered a size and a table of five hardcoded strings saying what you would get.
One of them read `light  Uniform(intensity: 0)` while the code built the slide at `Q10_ONE` —
full daylight. Nothing could have caught it: there was nothing to compare a description against.

The table is replaced by the controls it was describing — size, light, the three chemicals a cell
cannot do without, and who to put on it — which is the only version that cannot go out of step
with what Create does. `library::NewWorld` is the type, in the library rather than in `main.rs`
for the reason that module's header gives, and "the controls produce the scenario they describe"
is a test.

It is deliberately not the whole of the `world` view: one uniform intensity against six regimes,
three chemicals against sixteen. A starting point that can be got wrong and corrected is worth
having, and a second full editor for the same fields would be a second thing to keep in step.

**And the two `New…` sheets now say how they differ**, which was the actual complaint: both
opened on a size and a paragraph about themselves, and nothing said that one is a dish that runs
and the other is a recipe stopped at tick 0. That difference — §9.1's authoring state — is the
entire reason there are two.

#### Two labels that were wrong

- **`270 squares` is an edge.** Both sheets offer one number, because the slide is square, and
  suffixed it ` squares` — so 72,900 squares announced itself as 270 of them. `library::size_reading`
  says `270 × 270 — 72,900 squares in all`.
- **A light intensity had no reading.** `819` is how the file says it and "80% light" is how a
  person does, and working out that full daylight is 1024 meant reading SPEC §7. The picker
  carries the percentage beside it, off the *brightest* the regime ever reaches — the question it
  answers is whether a chloroplast can live here, and that is set by the best it ever gets.

#### The seeding tool names one genome out of eighteen

`PlaceCell` takes a genome by name and the interface offered a text box hinting `ancestor.mm`.
Eighteen genomes ship in `genomes/`; seventeen of them were reachable only by somebody who had
gone and listed the directory. `library::genomes()` lists them and the picker offers them, with
the box kept beside it — `Inhabitant.genome` is resolved by `mm_asm::locate` against whatever is
on disk, so a genome written five minutes ago is a legal thing to type and a picker that had
eaten the field would have made it unseedable.

---

## 10. The cell editor (M10.8)

Writing a genome and finding out what it does. The pieces exist and are scattered: the editor
is a text buffer with diagnostics, the debugger is a sandbox with step controls, the genome pane
is a reading of a live cell. What is missing is that none of them can answer "what does the line
I am looking at do", and you cannot run what you are writing without leaving it.

### 10.1 It is not a mode

The design draws the editor as the whole window with a rail either side. That is a state the
layout already reaches: the drawer goes to full height, and since M10.6 the rails get out of the
way when it does. So there is no editor mode and no second layout — there is a **width rule**,
the same one `skin::drawer_split` already applies to the context column: the editor shows its
left rail when there is room for it and drops it when there is not. One tab that is useful at
three hundred points and at eight hundred.

> **Still true as a window (§12.2).** The width rule is what made moving it cheap: a window
> whose layout already survives being narrowed is a window that survives being dragged. Its
> default size is set wide enough to clear the rule's threshold, because a pane that opens in
> its own degraded form reads as broken rather than as compact.

### 10.2 The scratch cell is in the editor's left rail

The editor and the debugger are drawer tabs and therefore exclusive — opening one closes the
other, which is intolerable when the loop is *edit, run, look, edit*. The design solves it by
promoting the editor to the window and demoting the debugger to a drawer.

Instead: **the scratch cell lives in the editor's left rail.** The editor becomes
self-sufficient, the debugger tab stays exactly what it is — breakpoints over the *live world*,
which is a different job — and no tab has to close for another to be useful.

> **M10.10 removed the exclusivity this was working around, and the rail stays anyway (§12.2).**
> Both are windows now and either can be open beside the other, so the constraint is gone; but
> the editor being able to run what you are writing without reaching for a second pane was
> always the better property, and it is the one this section actually argued for. What has
> changed is that the genome pane's `edit` chip no longer costs you the listing it opened from.

### 10.3 Source is edited; reading is rendered

The design's editor has a comment column, aligned, beside the operands. That cannot live in the
editable view: egui lays a `TextEdit` out from its buffer and maps the caret to buffer indices,
so inserting padding to align a column breaks the caret. It does not have to. The design's own
header carries a `source / reading` toggle, and the aligned columns belong to **reading**, which
is rendered rather than edited — which is what the genome pane has done since M10.3b.

**Wrapping goes off.** A genome line is short and wrapping helps nobody, and with it off line *n*
is always visual row *n* — which is what makes a gutter, an instruction-pointer marker and a
per-line error band correct rather than approximately correct.

### 10.4 What the caret is on

`mm_asm::SourceMap` already exists, and `Build::Ok(Assembled)` already carries it: `Span { byte,
len, line, col }`, `lookup(byte)` and `byte_of_line(line)`. Every caret-aware panel the design
asks for is one lookup away from data the editor is already holding and discarding.

So the right rail says what the line under the caret *is*: its opcode, its operand resolved, and
where a jump would land. Plus the opcode's entry from `Op::note`.

It says so **only while the buffer assembles**, and says that instead when it does not. There is
no source map for a program that did not compile, and guessing one would be a reading of a genome
that does not exist.

### 10.5 What it builds, and what it did

Two questions a scratch run can honestly answer, and one it cannot.

- **What it builds** comes from `mm_core::host::RecordingHost`, which already exists for exactly
  this and whose docstring already says so. It records what the program *asked the world for* —
  which is the honest claim, and the one the design's own caption makes: *not a promise;
  expression is gated on internal chemistry.* It needs one field, for `BUILD`, whose default
  implementation is a no-op so recording it changes no behaviour.
- **Gene hits** need no change to `mm-core` at all, and in particular none to the VM's hot loop.
  The sandbox steps the real `Vm`, so where `EXPRESS` went is observable from outside: if the
  instruction just stepped was `EXPRESS`, the new `ip` either equals some `promoters()[i].entry`
  or it does not. Attribution by observation, using the real matching rule because it is the real
  rule that ran. No second implementation to drift, no world state, nothing to serialise.
- **Divisions cannot be counted**, and the design's "3 divisions" is not true of a scratch cell:
  there is no world to divide into. What is knowable is `RecordingHost::splits` — *reached SPLIT
  three times* — and that is what it will say. The difference is the whole point of a sandbox.

`Sandbox::of` already builds a VM that runs against a `NullHost` with no world; it reads five
things out of a `World` to construct itself, so running an editor buffer instead is a second
constructor rather than new machinery.

### 10.6 Not in this milestone

**The preview run** (design 3b), for the reasons in §9.5. Deferring it a second time is
deliberate rather than forgetful.

---

## 11. The window itself (M10.9)

The window manager's title bar is a light GTK strip with an X11 default icon in it, sitting on
top of a near-black instrument. It belongs to a different application to look at, and §1 asks
that the chrome be dark so the eye stays on the plate — which it cannot be while thirty-two
points of somebody else's chrome are nailed to the top of it.

So: `decorations: false`, and **the bar the interface already has becomes the title bar.** Menus
on the left, window buttons hard against the right corner where every other window on the
desktop puts them, and the gap between the two is what you drag. That reclaims the strip rather
than adding one.

### 11.1 Moving and resizing are handed back to the compositor

`winit` has `drag_window` and `drag_resize_window`, both supported on X11 and Wayland (only
macOS lacks the second). They hand the gesture to the window manager, so snapping, tiling, the
drop shadow, the minimum size and the multi-monitor arithmetic stay somebody else's and stay
correct. What is ours is the decision of *when* to hand over, which is a hit test: `ui::resize_edge`.
Pure and tested, because a corner that answers "north" cannot be dragged diagonally and that is
not a thing anyone would find except by trying it.

**A corner is an L, not the little square where two thin bands cross.** Six points of band with
corners at the intersection worked and was a four-by-four-pixel target — findable only by
somebody who already knew the number. Sides are eight points; a corner reaches twenty along each
edge, capped at a third of it so there is always a middle that resizes in one axis. And the
cursor changes, which is the whole of how a frameless window's handles are discovered: without
it the band is a secret, whatever its width.

Two orderings matter. `window_chrome` runs **after** the interface, so it cannot steal a press
egui wanted — the rails go right to the edge, and six points is the difference between grabbing
the window and clicking the first overlay in the legend. And a pointer in the band is routed to
`Target::Panel`, so a drag that starts on the frame does not also pan the slide behind it.

**Handing over the gesture hands over the pointer grab, and with it the release.** The button
this application saw go down is never seen coming up: the window manager gets that event, not
us. Nothing said so, and two pieces of bookkeeping were left believing the button was still
held — `ButtonInput<MouseButton>` kept `Left` in its pressed set, so the *next* real press fired
no `just_pressed` at all, and `ui::Focus` kept its latch on whoever owned the press, so a frame
whose pointer was over the slide was still being handed to the title bar.

Between them that cost one whole press-and-release resynchronising after every window move or
resize, in whichever direction you went next: click the slide once before it would pan, click
the bar once before it would drag. **It reads as focus and it is arithmetic** — there is no
focus model in this interface, and the fix was not to add one.

`abandon_press` says it at the moment of handing over: *a gesture the compositor has taken over
is a gesture this application no longer owns.* It `reset`s the button rather than `release`ing
it, because a release posts a `just_released`, and the frame that reads one is the frame that
decides a short left-click on the slide selects a cell — so a window drag that happened to
finish over the plate would have picked whatever was under the pointer. `reset` says the press
never happened, which is the truth we are entitled to, and it makes the real release a no-op on
any platform that does deliver one.

It repairs a second, narrower race for free: `view.on_edge` is a frame stale, so a press that
lands in the resize band on the same frame the pointer arrives there latches the slide before
`window_chrome` gets to decide. The latch is now cleared by the same call that starts the
resize.

### 11.2 What this costs, and the way out

A borderless window is one where the usual escape hatches are the only escape hatches. On GNOME,
`Super`+drag moves and `Super`+middle-drag resizes whatever is under the pointer, decorations or
not, so a window that somehow becomes ungrabbable is still recoverable without a terminal.

`WinitWindows` is reached through a thread-local rather than a `NonSend` resource — that changed
in Bevy 0.19 and `bevy_winit`'s own comment calls it temporary. The consequence worth knowing is
that a system reading it **must** be pinned to the main thread with a `NonSendMarker`: off it,
the thread-local is a freshly constructed empty one, every window lookup silently misses, and
the window simply stops being movable with nothing anywhere saying why.

### 11.3 The transport is one control, not seven

A single bordered box with hairline divisions, as the design draws it. Separate chips with gaps
between them read as separate controls that happen to be adjacent, and these are seven positions
of one thing.

**Pause and play are two segments and not one toggle.** A toggle makes you read the glyph to
work out which state you are in; two segments lit differently makes you look at it. Two are lit
at once while it runs, and they are painted differently on purpose: play takes the **accent** —
*the world is going* — and the current speed takes a **raised ground** — *this is how fast*.
Two facts, both true, and a bar that painted them the same could not say so.

`skin::segmented_bar` is where this lives, and the ends keep the box's rounding on their outer
corners so a lit first or last segment does not square off the corner it sits in.

### 11.4 Glyphs are a gamble that has to be looked at

egui ships Hack and Ubuntu-Light and no more. `✕` (U+2715) is in neither and rendered as a tofu
box in the close button — which a screenshot showed and nothing else would have. The buttons use
`—`, `☐` and `×`, and anything outside Latin-1 needs a photograph before it is believed.

**And a second one was sitting in `Help ▸ Keys` the whole time**: `⌫` (U+232B), for the
backspace that sets the speed to `max`. It survived this rule being written because a menu was
the one surface a photograph could not reach — `MM_SHOT_VIEW=menu:help` fixed that (§12.6) and
the tofu box was in the first picture it took. It now reads `bksp`. The rule was right; it had
a blind spot, and the blind spot was the shape of the tooling.

---

## 12. Where a panel lives (M10.10)

The drawer had accumulated seven tabs and they were doing three unrelated jobs. A row of seven
equal chips is a claim that they are seven of one thing, and reading it as one — *these are the
things the drawer shows* — is how you end up looking for the parameter editor next to the tree
of life.

### 12.1 The drawer is what reads the slide

Two tabs, and the rule that admits them is not "wide" — it is **follows the selection, and is
read while the world runs.**

| tab | what it answers |
| --- | --- |
| genome | what is this cell running |
| ecology | where did it come from, what does it eat, what has happened |

Both are about *the thing you clicked*. Neither has a button that changes the world. That is
why they can live in a strip along the bottom with the slide above them: the slide is the
subject and the drawer is the caption.

The other five were an authoring surface, a settings form, a text editor and a step debugger.
None of them reads the selection. All of them are opened to *do* something and closed again,
and while one was open the slide was two hundred points shorter for no reason connected to what
was on it. `ui::Panel::dock` now returns `Dock::Window` for those, and `ui`'s test asserts the
drawer's tab list is exactly `[genome, ecology]` — so adding a ninth panel to the drawer is a
deliberate act with a test to change, which for five milestones it was not.

### 12.2 The windows, and what this overturns

**This reverses §4's decision and §10.2's workaround, and both were right when they were
made.** §4 chose a drawer tab for the parameter editor on two grounds. The first — *a window
over the slide has to be dragged aside to see what your change did* — is still true, and is the
price of this section; it is paid down by the windows being movable and never modal, not by
pretending it is not a cost. The second — *egui's window frame draws no fill in this build, so
the editor came out as ghost text with cells swimming through it* — is simply no longer true:
`skin::sheet_frame` exists, M10.7's sheets use it, and the photographs show a solid panel.

| window | key | what it is |
| --- | --- | --- |
| build | `B` | the tools, and the scenario they are writing |
| parameters | `,` | every cost, rate and mutation the world runs on |
| editor | `E` | the buffer, its scratch cell, and what the caret is on |
| debugger | `D` | breakpoints over the live world |

**Any number open at once, and that is the point.** §10.2 spent a design on the fact that the
editor and the debugger were exclusive tabs — `edit, run, look, edit` cannot survive one half
closing the other — and solved it by moving the scratch cell into the editor's own left rail.
That was the right fix for a drawer and it stays, because the editor being self-sufficient is
worth having; but as windows the problem it was solving does not arise, and the genome pane's
`edit` chip no longer throws away the listing you were reading in order to show you the buffer.

**`B`, not `T`.** The toolbox's key was `T`, and `T` is also `Follow selection` in §2 and in
`handle_input` — so the one keystroke opened a panel *and* set the camera chasing a cell. The
uniqueness test in `ui.rs` could not see it, because `follow` is not a panel. `B` was free.

### 12.2a A window can be a rail instead

§12.2 named the cost of windows out loud — *a window over the slide has to be dragged aside to
see what your change did* — and said it was paid down by them being movable and never modal.

For three of the four that holds. You open the debugger, read it, close it; the slide is waiting
underneath. For the **build** window it does not, and the reason is specific rather than a matter
of taste: the thing that window is *for* is drawing on the slide it is sitting on. Every stroke
is aimed at the surface the palette is covering, the palette has to stay open between strokes
(§4.3 is the argument, and it is why this window is not a menu and not modal), and "drag it
somewhere else" is not an answer when the slide is the whole screen.

A rail cannot be over anything. That is §10.1's first sentence — *whatever egui does not claim
is, by definition, the slide* — so this is not a second layout to keep in step with the first.
It is the panel asking to be a rail again, and the slide shrinks to make room exactly as it does
for the cell inspector.

**`ui::Panels::docked`, one at a time.** A rail is a strip; two of these stacked in one is two
strips of four hundred points, and at that width there is nothing left to be editing. The drawer
made the same call for the same reason, and the state has the same shape: *which*, not *whether*.
Docking a second window floats the first rather than closing it.

**It is a state, not a reclassifying.** `Panel::Build.dock()` still returns `Dock::Window` — that
is where these four live, the View menu still lists them under *windows*, and
`a_windows_home_is_still_a_window` is the test that stops a UI toggle quietly overturning §12.2.

| | |
| --- | --- |
| into the rail | the `dock left` chip at the top of any of the four windows, or View ▸ In the left rail |
| back out | the `float` chip in the rail's header, or the same menu |
| where | outermost on the left, so docking does not shove the cell inspector sideways |
| width | one `Panel` id for all four — the width you drag it to belongs to the *rail*, not to whichever window is sitting in it |

**A docked body degrades, and one of them degrades differently.** `skin::drawer_split` drops its
context column below about 690 points, which is what that rule is for: the build window in the
rail is the toolbox with its help text folded away, which is right for a palette. The parameter
editor is the exception, because its table lays columns out at absolute offsets from the row's
left edge — below the sum of them the `was` column is drawn off the edge and clipped, and half a
number is worse than no number. So `docked_min_width` gives that one panel a floor derived from
`param_column_x`, rather than a typed constant that can be left behind when a column is widened.
Docked narrow it loses the per-field detail panel, which is the context column; the hover text is
still there.

**`MM_SHOT_VIEW=dock:build`** arranges it, for §12.6's reason: a window that has become a rail is
a different picture of the same panel, and a state a script cannot arrange is a state nobody
reviews. `dock:none` puts it back.

### 12.3 The toolbox and the scenario pane are one window

`toolbox` and `scenario` were two tabs and the split was never on a seam. The toolbox is what
you draw with; the scenario pane is what the drawing came out as, and checking the second is how
you find out the first one worked — §9.2 says so in as many words, and §4 lists three edits that
reached the slide and not the recipe. Reading the RON meant closing the brush.

One window, two views behind one header, exactly as the ecology pane holds four. `ui::Build` is
the enum, `Build::Tools` and `Build::Scenario` the views. **The toolbox does not become a
menu**: §4.3's argument is untouched and is the reason this window is not modal either. A thing
you adjust between two strokes has to stay on screen while you stroke.

### 12.4 A window is as big as it is told to be

Every one of these bodies fills its height — `skin::drawer_split` under a header, with the
scroll areas at `auto_shrink([false, false])`. That is the drawer shape doing exactly what it is
for, and in a container that sizes itself to its content it is a runaway: the body asks for all
of it, the container offers however much was asked for, and there is no fixed point. Two of
these were live in the build before the windows existed:

- **The scenario tab, vertically.** Its save row was laid out *after* the outline's scroll area,
  so the content came out one footer taller than the drawer, every frame, against a drawer that
  had just grown by a footer. About thirty points a frame until it hit the `760` of
  `size_range` and had eaten the slide. The save row was never on screen — it was always the
  part hanging off the bottom, which is why the symptom read as "the pane grows" rather than
  "the pane overflows". Fixed by laying the footer out bottom-up first.
- **`drawer_split`, horizontally.** It handed its two columns `total` between them and then laid
  them out with an `item_spacing` gap, so the content was one gap wider than the space. A panel
  is as wide as the window and simply clipped it; a window is as wide as its content, so the
  build window crept six points wider every frame.

*It happened twice more, and neither was found by looking.* The parameter editor and the build
window's `world` view both reserved their footer's height as a **constant** — `FOOTER_HEIGHT`,
twenty-six points — and both were two points short, so the content came out two points taller
than the window every frame against a window that had just grown by two. Measured at 2px a
frame, which at 60fps is the editor filling the screen in about five seconds. A third arrived
with the `dock left` chip of §12.2a: a bare right-to-left layout takes `available_size`, and in a
`Ui` that has just been told it is the whole window that is the whole height, so the chip's row
claimed all of it and pushed the body off the bottom edge.

**None of the three is visible in a screenshot.** A window growing two points a frame does not
look like a bug in a still, it looks like a big window — which is why two of them shipped. What
finds it is a column of numbers, so `MM_WINDOW_PROBE=1` prints every open window's rect to stderr
each frame:

```text
MM_WINDOW_PROBE=1 MM_SHOT_VIEW=params:metabolism MM_SHOT=/tmp/p.png \
  MM_SHOT_AFTER=25 ./target/release/mm-app 2>&1 | grep PROBE
```

First line equal to last line is the whole test; anything monotonic is a body asking for more
than the window it is in. The constant is gone — both bodies now lay their footer out bottom-up
first and give the body the remainder, which is what `scenario_body` had always done and is the
one shape with a fixed point. **Do not reserve a height you have not measured.**

So: `default_size` for where a window starts, `set_min_size(available)` inside so the body's
`available_height` is the window's rather than infinity, and `resizable` then means what it
says. The defaults are also wide enough to clear each body's *own* width rule — the editor's
left rail drops below `EDITOR_RAIL + 420`, and a default that trips its own rule is a window
that looks broken the first time it opens.

### 12.5 A menu is as wide as it is told to be, too

The same failure as §12.4, in the surface next door, and in both directions at once:

- **`View` came out six hundred points wide**, a third of the window, for items thirty points
  long. `menu_rule` fills the width it is given, a popup gives whatever is left of the screen,
  and a menu sizes to its content — so one rule dragged the whole menu to the screen edge.
- **`Tools` came out sixty-eight**, because it had no rule in it and sized to its longest word,
  with the shortcut column jammed against the labels.

Neither width was chosen. `theme::MENU_WIDTH` is, and `skin::menu` pins every menu to it: wide
enough for the longest item and its shortcut, and the same for all four so the bar does not
change shape as you move along it. `menu_margin` gains four points either side — a menu item is
a full-width button and puts its shortcut against its own right edge, so at zero the keys were
painted on the popup's border — and `menu_caption` and `menu_rule` indent by `button_padding.x`,
so a caption, a label and a rule all begin at the same x.

**The shortcut on a switched-on item was the one you could not read.** `DIM` is chosen to sit
back from a near-black menu and vanishes on the raised fill of a selected row, which made every
item that was *on* the one whose key was invisible. `skin::menu_toggle` is the row that knows it
is a switch, and gives a selected row the label's own ink.

### 12.6 A menu can be photographed

None of the above was found by reading the code, and none of it could have been: `MM_SHOT_VIEW`
could arrange every panel, pane, window and sheet in this interface and not one menu, because a
menu is a popup that opens on a click. So two menu widths were wrong for a milestone, and §11.4's
tofu box had a sibling in `Help ▸ Keys` that outlived the rule written to catch it.

`MM_SHOT_VIEW=menu:view` holds one open. egui keeps a popup's open state in memory under an id
derived from its button's response, so the harness asks for exactly what a click would have
done — no second code path, and nothing that behaves differently when the variable is unset.
The repo's method is that a claim about the picture is settled by a photograph; a surface that
cannot be photographed is a surface where that method silently does not apply.

### 12.7 The menu says which is which

`View` is in two groups under captions: **on the slide** (the rails and the drawer's tabs) and
**windows**. Both are generated from `Panel::ALL` filtered on `dock()`, which is the whole
reason that list exists — the menu and the keyboard cannot drift apart if neither is written
out by hand. `Tools ▸ Build…` opens the build window on its tools view, and `File ▸ Save
scenario… S` opens it on the scenario view, which is what that item has always meant: *show me
the pane that holds the path field*, not *write the file*.

---

## 13. Order of work

| step | what | why it is here |
| --- | --- | --- |
| **M10.1** | shell, layout, input routing, simulation thread | Fixes the scroll and keyboard bugs, which are daily friction. Small. Unblocks measuring anything. |
| **M10.2** | configs into `Scenario`, parameter editor, open/save, interventions | The balancing work needs it and it is blocked on a `mm-core` change, so it should not wait. |
| **M10.3** | genome reading mode | Small, self-contained, high value per line. |
| **M10.4** | ecology pane | The data all exists; this is presentation. |
| **M10.5** | field texture, instanced cell shader, organelle pass | The biggest piece and the only one with real technical risk. Last, on a shell that is already stable. |
| **M10.6** | the chrome: theme, type, row grammar, menus, toolbox and parameters | §8. After the renderer, because it must not be moving while the renderer is; and it touches none of the same files, so it cannot be the reason a frame regresses. |
| **M10.9** | the window itself: borderless, with the menu bar as its title bar | §11. Independent of everything else and the smallest of the four, but last, because it is the only one whose failure mode is a window you cannot move. |
| **M10.8** | the cell editor: its rails, the caret's reading, the ISA reference, the scratch cell | §10. Last because it is the one that needs `mm-core` to say anything new, and the only new thing it needs is documentation. |
| **M10.7** | building a scenario: the authoring caption, the scenario pane and its RON preview, what is on the slide, sheets and the library | §9. Almost no new mechanism — §4 built it all and the interface never said so. Last because it is the smallest, and because it is the one whose absence is only ever confusing rather than wrong. |
| **M10.10** | where a panel lives: the drawer down to what reads the slide, the other five as windows, the toolbox and the scenario pane merged | §12. After all of them, because it is the step that could only be taken once there were seven tabs to look at: the shape of the mistake is not visible while you are making it one milestone at a time. |

10.1 first because it is the smallest thing that makes the application usable day to day.
10.5 before 10.6 because it is the one that can go wrong, and it should go wrong against a UI
that is otherwise finished rather than one that is also moving. 10.6 last because it is the
only step whose worst outcome is that something looks wrong, which is visible immediately and
costs nothing to revert.

---

## 14. What could go wrong

- **The instanced pipeline is the risky part.** Bevy's mid-level render API is the part that
  moves most between releases and has the least documentation. Mitigation: follow the upstream
  example closely, land the field texture first (bigger win, far less API surface), and keep
  the sprite path behind a flag until the instanced one is faster on the same scene.
- **Hoisting configs into `Scenario` bumps the snapshot format.** Every existing `.mmslide`
  stops loading. There are none outside the test suite yet, which is exactly why this should
  happen now rather than after people have runs they care about.
- **Interventions are a determinism surface.** Anything that can change world state from the
  UI must go through the recorded path or `I1` quietly stops being true. The rule is: if it
  writes to the world, it is an intervention, and the compiler should be made to enforce that
  by routing every such write through one function.
- **50,000 at 30fps may still not be met after all this.** The estimate is that the field
  texture and the instanced draw together are worth an order of magnitude, which should be
  ample — but it is an estimate, and the simulation half of the target is M9's problem and is
  currently the larger of the two. Measure the halves separately from M10.1 onward and do not
  let one hide behind the other.
- **A theme is a thousand small chances to touch the renderer.** Every panel that draws a
  swatch, a cell portrait or a slide overlay sits one function call away from code that is
  finished and must not move. The mitigation is §8.1's file list and the renderer's own probes:
  if `shader_probe`, `packing_probe` or `nine_cells` needs re-recording, the boundary was
  crossed and the commit is wrong regardless of how it looks.
- **Dark chrome hides low-contrast text.** The palette in §8.2 puts `DIM` at `#4e565f` against
  `#0e1013`, which is legible on the panel it was chosen against and marginal on the darker
  `SLIDE` ground. Every role/ground pair that actually occurs is checked for contrast in
  `theme.rs`'s tests rather than by eye on one monitor.
