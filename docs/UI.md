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
│ File   Slide   Simulation   View   Tools   Help          ⏸ ▶ 1× 8× 256× ⏭    │  menu bar
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

Slide     Scenario library        ▸           soup · photosynthesis or die · the long dusk ·
                                              predator introduction · archipelago ·
                                              archipelago control · seasons · the vent
          Open scenario…                      .ron
          Parameters…             Ctrl+,      the editor described in §4
          Save parameters as…                 the running world's config, back out as .ron
          ──
          Reseed                  R

Simulation  Run / Pause           Space
            Step one tick         .
            Speed                 ▸           1× · 8× · 256× · unlimited
            Breakpoints…
            ──
            Interventions…                    what has been changed mid-run, and when

View      Panels                  ▸           cell · metrics · legend · genome · ecology ·
                                              editor · debugger        (each a checkbox)
          Overlays                ▸           one per chemical, 1–9
          Optics                  O           vignette, defocus, dust
          ──
          Follow selection        T
          Reset camera            Home

Tools     Select                  F1
          Move                    F2
          Remove                  F3
          Draw barrier            F4
          Erase barrier           F5

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

### The parameter editor

One `Parameters…` window, tabbed by group (world · chemistry · light · VM · metabolism ·
biology · ecology · junctions), each row a labelled numeric field with its unit, its default,
and the doc comment from the struct as hover text — those comments are unusually good and
throwing them away in favour of hand-written tooltips would be a waste. Fields that changed
from the scenario's value are marked, with a per-field revert.

Applying is explicit — an `Apply` button, not live-on-keystroke — because each apply is an
intervention that goes on the record, and one per keystroke would be a useless record.

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

---

## 6. The ecology pane

The tree of life exists. `wiki::layout` produces a proper dendrogram — rows, depths, parent
links, contiguous subtrees, with three tests asserting the layout is readable. It is then drawn
as an **indented text list inside a 140-pixel scroll area** in the corner of the wiki window
(`main.rs:1436`). That is why it does not feel like a tree of life: the data is right and the
presentation is a footnote.

The ecology pane is a drawer tab with three views, sharing one selection — click a species
anywhere and all three follow it.

**Tree of life.** Painted, not listed. Horizontal axis is founding tick so the shape means
something; vertical is `TreeNode::row`. Branch thickness from peak population, colour from
trophic guild (producer green, scavenger amber, predator red, osmotroph blue — the same
colours the food web and the metrics use). Extinct lineages fade out and stop at their
extinction tick rather than running to the right edge. Living ones reach the present. Hover
for the name and the population; click for the species page. A "prune below *n* individuals"
slider, because a long run has thousands of species and most of them are noise.

**Food web.** `foodweb::web()` already returns nodes with trophic levels and edges with
measured flows, and `Edge::is_death`/`is_recycling` already distinguish the loop. Draw it as a
layered graph, level on the vertical, edge width from flow, the recycling back-edge dashed.
The existing `foodweb_panel` renders it as text and should keep doing so behind a toggle —
the numbers are useful and a graph hides them.

**Timeline.** The existing timeline, full drawer width instead of 26 pixels in a corner.
Scrubbable, first-occurrence events as flags, mass extinctions as bands, **and interventions
from §4 in a distinct colour**, because "the population crashed" and "you tripled the repair
cost ninety ticks earlier" belong on the same axis.

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
per active chemical, blended in the shader), written from `Frame::overlays` and `Frame::light`
each frame and drawn as one quad under everything. A megabyte of upload per frame is nothing;
262,143 entities are not. `Frame` already carries exactly the right data — `field: Vec<f32>`
normalised, per-layer `rgb` and `peak` — so this is a change of destination, not of content.
The `sqrt` presentation curve moves into the shader where it belongs.

**Cells become one instanced draw.** One quad mesh, one draw call, an instance buffer of
50,000 records. Bevy's own `shader_instancing` example is the reference implementation for
this shape of thing on 0.14 (custom `RenderCommand`, `SpecializedMeshPipeline`, per-instance
vertex buffer) and following it is the low-risk path.

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

### Bevy version

We are on 0.14.2. **Do the renderer on 0.14 and treat a Bevy upgrade as its own separate
commit afterwards.** Doing both at once means every compile error is ambiguous between "my
instancing is wrong" and "the API moved", and the instancing example we are following is the
0.14 one. There is no feature we need that 0.14 lacks.

### Docking

**Hand-rolled with egui's own `SidePanel` / `TopBottomPanel` / `CentralPanel`**, not
`egui_dock`. The layout in §2 is exactly what those three constructs produce natively, it adds
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

## 8. Order of work

| step | what | why it is here |
| --- | --- | --- |
| **M10.1** | shell, layout, input routing, simulation thread | Fixes the scroll and keyboard bugs, which are daily friction. Small. Unblocks measuring anything. |
| **M10.2** | configs into `Scenario`, parameter editor, open/save, interventions | The balancing work needs it and it is blocked on a `mm-core` change, so it should not wait. |
| **M10.3** | genome reading mode | Small, self-contained, high value per line. |
| **M10.4** | ecology pane | The data all exists; this is presentation. |
| **M10.5** | field texture, instanced cell shader, organelle pass | The biggest piece and the only one with real technical risk. Last, on a shell that is already stable. |

10.1 first because it is the smallest thing that makes the application usable day to day.
10.5 last because it is the one that can go wrong, and it should go wrong against a UI that is
otherwise finished rather than one that is also moving.

---

## 9. What could go wrong

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
