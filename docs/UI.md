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
│  GENOME │ ECOLOGY │ EDITOR │ DEBUGGER                              ▲ collapse │
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
| bottom drawer | `TopBottomPanel::bottom`, tabbed, collapsible | genome, ecology, editor, debugger — the four things that want width |
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
File      New slide…              Ctrl+N      from a scenario
          Open slide…             Ctrl+O      .mmslide, resumes where it was saved
          Save slide              Ctrl+S
          Save slide as…          Ctrl+Shift+S
          Recent slides           ▸
          ──
          Export                  ▸           species archive · metrics · genome · screenshot
          ──
          Quit                    Ctrl+Q

Slide     Scenario library        ▸           whatever is in scenarios/, by name
          Open scenario…                      .ron, by path
          Parameters…             ,           the editor described in §4
          Save parameters as…                 the running world's config, back out as .ron
          ──
          Reseed                  R

Simulation  Run / Pause           Space     resumes at the speed it was paused at, not 1×
            Step one tick         .
            Speed                 ▸           paused · ½× · 1× · 8× · unlimited
                                              (½× is 30 ticks a second, for watching a
                                              division rather than catching one)
            Breakpoints…
            ──
            Interventions…                    what has been changed mid-run, and when
                                              (opens the ecology pane on that view)

View      Panels                  ▸           cell · metrics · legend · genome · ecology ·
                                              parameters · editor · debugger
                                                                       (each a checkbox)
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
          Thickness  ──○────      1–10, default 3   how wide a wall stroke is

Help      Keys
          ISA reference
          About
```

Every keyboard shortcut already in `handle_input` appears next to its menu item, and no
shortcut exists that is not in a menu. The current build has fourteen single-key bindings and
no way to discover any of them.

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

They are now the first group in the parameter drawer: a variant picker each, the chosen
variant's fields beside it, and their own apply. Drafted rather than live because
`set_current` invalidates the entire prescribed velocity field, so a strength dragged through
a slider would rebuild it once per frame of the drag.

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

The tools are `Select, Move, Remove, DrawBarrier, EraseBarrier, Paint, Unpaint, Source, Drain,
PlaceCell`, on `F1`–`F10`. The last five are the new ones. `Paint` and `Unpaint` stroke like the
wall tools do, at the same brush width; `Source` and `Drain` are dragged as rectangles, because a
flux is an area and clicking a point cannot say how big; `PlaceCell` drops founders of a named
genome where you point.

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

`Slide → New scenario…` gives an empty stopped slide to build on — not `New slide` with no
founders, which is a petri dish, lit and seeded with three chemicals you did not ask for and
cannot see. Build it, then Save from the same menu.

For that to mean anything, everything the tools do has to reach the `Scenario`, and three things
did not:

- **Walls** lived in the substrate only. `place_barrier` never touched `scenario.barriers`, so
  drawing on a slide and saving gave back a scenario with no walls in it.
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

### Overlay visibility lives in the legend, not in a menu

Switching a chemical overlay was View ▸ Overlays ▸ item — **three levels of menu**, with the
menu itself covering the part of the plate you were trying to look at, and keys 1–9 reaching
only nine of the sixteen. Comparing two fields means seeing one, then the other, in the same
place at the same zoom, and that gesture was three levels deep and twice over.

The legend is already in the right rail, already open by default, and already knows every
chemical's colour. **Its rows are the control now**: all sixteen listed, one click each, swatch
filled when the overlay is on and outlined when it is off so the state reads down the column at
a glance. The peak stays on the rows that are on, because each layer is normalised against its
own maximum and the colours are legible and meaningless without it.

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
peak, and the ones reading `0.0` are the ones nothing in the world is using. It answers "which
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
normalised, per-layer `rgb` and `peak` — so this is a change of destination, not of content.
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

| tab | work area | context column |
| --- | --- | --- |
| genome | the listing | genes, and the diagnostics |
| ecology | the tree, web, timeline or budget | the selected species |
| toolbox | tools, settings, the flux table | why this is a panel; what a source looks like on the slide |
| parameters | the field table | *(the group list, on the left instead)* |
| editor | the buffer | diagnostics, live |
| debugger | the trace | breakpoints, and the step controls |

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

## 9. Order of work

| step | what | why it is here |
| --- | --- | --- |
| **M10.1** | shell, layout, input routing, simulation thread | Fixes the scroll and keyboard bugs, which are daily friction. Small. Unblocks measuring anything. |
| **M10.2** | configs into `Scenario`, parameter editor, open/save, interventions | The balancing work needs it and it is blocked on a `mm-core` change, so it should not wait. |
| **M10.3** | genome reading mode | Small, self-contained, high value per line. |
| **M10.4** | ecology pane | The data all exists; this is presentation. |
| **M10.5** | field texture, instanced cell shader, organelle pass | The biggest piece and the only one with real technical risk. Last, on a shell that is already stable. |
| **M10.6** | the chrome: theme, type, row grammar, menus, toolbox and parameters | §8. After the renderer, because it must not be moving while the renderer is; and it touches none of the same files, so it cannot be the reason a frame regresses. |

10.1 first because it is the smallest thing that makes the application usable day to day.
10.5 before 10.6 because it is the one that can go wrong, and it should go wrong against a UI
that is otherwise finished rather than one that is also moving. 10.6 last because it is the
only step whose worst outcome is that something looks wrong, which is visible immediately and
costs nothing to revert.

---

## 10. What could go wrong

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
