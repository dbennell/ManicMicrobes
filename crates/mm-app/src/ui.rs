//! The shell: which region owns the pointer, which panels are open, and where the camera goes
//! when you zoom (M10.1).
//!
//! None of this knows Bevy or egui exists, for the same reason [`crate::slide`] does not: the
//! decisions worth getting right are decisions about arithmetic and about who owns an event,
//! and neither needs a window to check. `main.rs` supplies the rectangles and applies the
//! answers.
//!
//! # The bug this module exists to fix
//!
//! Scrolling the genome listing zoomed the microscope. The cause was not a missing check but a
//! missing *concept*: every panel was a floating window over a viewport that was the whole
//! window, so the only question the input code could ask was "is the pointer over some egui
//! area", and it asked it in one place out of four. Giving the slide a rectangle of its own
//! turns four scattered conditions into one decision, made once a frame, in [`route`].

/// A rectangle in window pixels, y increasing downward as the cursor does.
///
/// Its own type rather than `egui::Rect` or `bevy::Rect` so that the routing tests need
/// neither. `main.rs` converts at the boundary and nowhere else.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Rect {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl Rect {
    #[must_use]
    pub fn new(min_x: f32, min_y: f32, max_x: f32, max_y: f32) -> Rect {
        Rect {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    /// Half-open on the maximum edge, so two rectangles sharing a border cannot both claim the
    /// pixel on it.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.min_x && x < self.max_x && y >= self.min_y && y < self.max_y
    }

    #[must_use]
    pub fn centre(&self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) / 2.0,
            (self.min_y + self.max_y) / 2.0,
        )
    }

    #[must_use]
    pub fn width(&self) -> f32 {
        (self.max_x - self.min_x).max(0.0)
    }

    #[must_use]
    pub fn height(&self) -> f32 {
        (self.max_y - self.min_y).max(0.0)
    }

    /// A rectangle with no area claims nothing. Guards the frame before egui has laid anything
    /// out, where the viewport is still zero-sized and every click would otherwise be a miss
    /// reported as a hit on the slide.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }
}

/// Who owns the pointer, and therefore what the next wheel, drag or click does.
///
/// Exactly one of these, never two. The rule that fixes the scroll bug is that a wheel event
/// over a scrollbar is a `Panel` event and the slide never hears about it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    /// The slide. Wheel zooms, drag pans, click selects, tools apply.
    Slide,
    /// A panel: a docked rail, the drawer, or a floating window over the slide.
    Panel,
    /// Outside the window, or no pointer at all.
    Nowhere,
}

/// Decide who owns the pointer this frame.
///
/// `egui_wants_pointer` covers the windows that float *over* the viewport — a torn-off panel,
/// a menu that has dropped down, a combo box — which are inside the viewport rectangle and are
/// still not the slide. The rectangle covers the docked rails and the drawer, which egui does
/// not report as "areas" at all.
///
/// Both are needed. Either alone leaves a hole, and the hole is the bug.
#[must_use]
pub fn route(pointer: Option<(f32, f32)>, viewport: Rect, egui_wants_pointer: bool) -> Target {
    let Some((x, y)) = pointer else {
        return Target::Nowhere;
    };
    if egui_wants_pointer {
        return Target::Panel;
    }
    if viewport.is_empty() || !viewport.contains(x, y) {
        return Target::Panel;
    }
    Target::Slide
}

/// Which region a press belongs to, held until the button comes up.
///
/// A drag that starts on the slide stays on the slide even when the pointer crosses a panel.
/// Without this, panning to the left edge hands the plate to the cell rail halfway through the
/// gesture and the slide stops under your hand — which is worse than the bug being fixed,
/// because it happens during the one interaction the microscope is most used for.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Focus {
    held: Option<Target>,
}

impl Focus {
    /// The button went down. Latch whoever is under the pointer and report them.
    pub fn press(&mut self, live: Target) -> Target {
        self.held = Some(live);
        live
    }

    /// The button came up. The latch is released and the live answer applies again.
    pub fn release(&mut self) {
        self.held = None;
    }

    /// Who gets this frame's events: the latched owner during a drag, otherwise whoever is
    /// under the pointer now.
    #[must_use]
    pub fn resolve(&self, live: Target) -> Target {
        self.held.unwrap_or(live)
    }

    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.held.is_some()
    }
}

/// The most squares one drag sample may paint.
///
/// A guard rail rather than a design choice. The pointer's slide coordinate is unbounded — a
/// zoomed-out camera and a pointer at the window edge can produce a square index far outside
/// any grid — and the fill below walks one square at a time. The largest legitimate line on
/// the largest slide the New-slide dialog offers is about 1,450 squares, so anything past this
/// is a coordinate that has gone wrong and the honest response is to stop walking rather than
/// to spend a second on it.
pub const MAX_STROKE: usize = 4096;

/// Every square on the line between two, inclusive of both ends.
///
/// Integer Bresenham, and the reason a barrier tool needs it at all: the pointer is sampled
/// once a frame, so a hand moving at any speed skips squares between one sample and the next.
/// Painting only where the pointer *was* leaves a dotted line with gaps that get wider the
/// faster you draw — and a barrier with gaps in it is not a barrier, because the fluid and now
/// the cells both go straight through the holes.
///
/// Arithmetic in `i64` so that two far-apart coordinates cannot overflow the difference, with
/// [`MAX_STROKE`] bounding the walk. Pure, so the interesting cases are a table rather than a
/// thing you have to draw with a mouse to check.
#[must_use]
pub fn line_squares(from: (i32, i32), to: (i32, i32)) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (i64::from(from.0), i64::from(from.1));
    let (x1, y1) = (i64::from(to.0), i64::from(to.1));
    let dx = (x1 - x).abs();
    // Negative, which is what lets one error term serve both axes.
    let dy = -(y1 - y).abs();
    let sx = if x < x1 { 1 } else { -1 };
    let sy = if y < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut out = Vec::new();
    loop {
        out.push((x as i32, y as i32));
        if (x == x1 && y == y1) || out.len() >= MAX_STROKE {
            return out;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

/// Step the overlay selection to the next chemical shown *on its own*.
///
/// The gesture the menu could not offer. Comparing two chemical fields means looking at one,
/// then the other, in the same place at the same zoom — and doing that through
/// View ▸ Overlays ▸ item means three levels of menu twice per comparison, with the plate
/// hidden behind the menu while you aim at it.
///
/// So this is *solo*, not toggle: whatever was on goes off and exactly one thing comes on, so
/// holding a key steps through the chemicals one at a time and the picture is only ever showing
/// one of them. `step` of 1 goes forwards, -1 back.
///
/// The cycle includes an **off** position, which is what makes it a loop rather than a wall —
/// sixteen chemicals and then bare slide, and round again. Bare slide is a reading too: it is
/// the one that says which of what you are looking at is the cells and which is the water.
///
/// A mask with several bits set has no single "current", so stepping from one starts the cycle
/// at the lowest chemical that is on rather than pretending to know which you meant.
#[must_use]
pub fn step_solo(mask: u32, count: usize, step: i32) -> u32 {
    let count = count.min(32).max(1) as i32;
    // Where the cycle is now: `0..count` for a chemical, `count` for off.
    let current = if mask == 0 {
        count
    } else {
        mask.trailing_zeros() as i32
    };
    // One past the last chemical is the off position, so the ring is `count + 1` long.
    let ring = count + 1;
    let next = (current + step).rem_euclid(ring);
    if next == count {
        0
    } else {
        1u32 << next
    }
}

/// Every overlay switched on, as a bit per chemical.
///
/// Clamped at 32 because the mask is a `u32`, and written as a branch rather than
/// `(1 << n) - 1` because that shift is undefined at 32 and this is reached from a chemical
/// count that comes out of a scenario file.
#[must_use]
pub fn all_overlays(count: usize) -> u32 {
    match count.min(32) {
        32 => u32::MAX,
        n => (1u32 << n) - 1,
    }
}

/// All of them, or none of them, whichever the mask is not.
///
/// One button for both directions, because they are the same intention seen from either side:
/// clear what is on the plate, or put everything on it. Anything on at all means the button
/// clears — so it is always the way *out* of whatever you were looking at, and never a surprise
/// that turns sixteen layers on when you meant to turn one off.
#[must_use]
pub fn toggle_all(mask: u32, count: usize) -> u32 {
    if mask == 0 {
        all_overlays(count)
    } else {
        0
    }
}

/// Narrowest and widest a barrier brush may be, in squares.
pub const BRUSH_MIN: u32 = 1;
pub const BRUSH_MAX: u32 = 10;

/// What a barrier brush is by default, in squares.
///
/// Three rather than one. One square is a line the fluid can still see round — and now that a
/// stroke is dragged rather than clicked, the common thing to want is a wall, not a pixel. It
/// is also the narrowest brush that makes a *diagonal* stroke solid: at one square a diagonal
/// run touches only at its corners, which the barrier mask treats as two separate walls with a
/// gap between them, and the gap is exactly wide enough for a cell.
pub const BRUSH_DEFAULT: u32 = 3;

/// Every square a brush of the given width covers when stamped on one square.
///
/// A disc, not a box, because a box brush leaves mitred corners wherever a freehand stroke
/// changes direction and the eye reads those as mistakes. `width` is the diameter, so 1 is the
/// single square under the pointer and 10 is a disc ten squares across.
///
/// The test is `4 * (dx² + dy²) <= width²`, which is the disc of that diameter in integers —
/// no square roots and no rounding to argue about. Widths are clamped into
/// `BRUSH_MIN..=BRUSH_MAX` rather than trusted, because this is reached from a saved setting.
#[must_use]
pub fn brush_squares(centre: (i32, i32), width: u32) -> Vec<(i32, i32)> {
    let w = width.clamp(BRUSH_MIN, BRUSH_MAX) as i32;
    let reach = w / 2;
    let mut out = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            if 4 * (dx * dx + dy * dy) <= w * w {
                out.push((centre.0.saturating_add(dx), centre.1.saturating_add(dy)));
            }
        }
    }
    out
}

/// Where the camera must move so that the point under the pointer stays under the pointer.
///
/// The difference between zooming a microscope and operating a slider. `offset` is the
/// pointer's displacement from the *viewport's* centre, not the window's — with a wide rail
/// open those are not the same place, and using the window's would make the slide creep
/// sideways on every scroll.
///
/// Both axes increase in the same direction as the slide's own, so there is no flip here; the
/// flip belongs to the Bevy world transform and lives in `main.rs` with it.
#[must_use]
pub fn zoom_about(
    centre: (f32, f32),
    offset: (f32, f32),
    old_scale: f32,
    new_scale: f32,
) -> (f32, f32) {
    // Guard the degenerate scales rather than trusting the caller's clamp: a zero here is a
    // silent NaN that propagates into the camera and blanks the window.
    if old_scale <= 0.0 || new_scale <= 0.0 {
        return centre;
    }
    let shift = 1.0 / old_scale - 1.0 / new_scale;
    (centre.0 + offset.0 * shift, centre.1 + offset.1 * shift)
}

/// The least height a docked rail can be given and still be worth drawing.
///
/// Two rows of readings and a section header. Below this a rail is a sliver you cannot read and
/// cannot resize, and its scrollbar is longer than its contents.
pub const RAIL_MIN_HEIGHT: f32 = 48.0;

/// Whether the rails should be drawn at all, given what the drawer has left them.
///
/// # The artefact this exists to prevent
///
/// The drawer can be dragged until it fills the window, which is the right thing to be able to
/// do — a genome listing or a parameter table wants every pixel. But the rails are laid out
/// *after* the drawer, so when it takes everything they are handed a region of zero or negative
/// height, and a rectangle whose top is below its bottom is inverted rather than empty.
///
/// egui draws a resizable panel's separator along that rectangle's cross range, and an inverted
/// range still has two ends: the line comes out spanning the full height of the window,
/// straight down through the drawer, at the x the rail would have had. A stripe through the
/// middle of the pane you just expanded, belonging to a panel that is not on screen.
///
/// So the rail goes rather than shrinking. A rail with no room is not a thin rail, it is no
/// rail, and saying so here is what keeps `panels` from drawing one.
#[must_use]
pub fn rails_fit(available_height: f32) -> bool {
    available_height >= RAIL_MIN_HEIGHT
}

/// How wide a scale bar should be, and what it measures.
///
/// Returns `(squares, pixels)`: how many substrate squares the bar spans, and how long to draw
/// it. The bar is the longest of `1, 2, 5, 10, 20, 50, …` squares that fits in `max_pixels`, so
/// it changes length as you zoom but never reads as an awkward number.
///
/// # Squares, not microns
///
/// `docs/UI.md` §2 sketches this bar as `├─── 200 µm ───┤`, and it is not drawn in microns,
/// because nothing anywhere says how large a substrate square is. Picking a figure here would
/// invent a physical scale for a world that does not have one and then print it as though it
/// were measured — and a scale bar whose whole job is to say how big things are is the last
/// place to do that. The square is the unit the simulation actually has. If a physical size is
/// ever wanted it belongs in `SPEC.md` as a stated conversion, and this can then use it.
///
/// A degenerate scale returns one square and no length rather than dividing by it: the caller's
/// zoom is clamped, but this is reached from a saved view file.
#[must_use]
// `!(x > 0.0)` and not `x <= 0.0`, which clippy would prefer and which is not the same
// statement: a NaN fails every comparison, so `<=` lets it straight through into the loop and
// out the other side as a bar of NaN pixels. `a_degenerate_scale_does_not_divide_by_it` covers
// it, and would fail on the "tidier" form.
#[allow(clippy::neg_cmp_op_on_partial_ord)]
pub fn scale_bar(pixels_per_square: f32, max_pixels: f32) -> (u32, f32) {
    if !(pixels_per_square > 0.0) || !(max_pixels > 0.0) {
        return (1, 0.0);
    }
    let mut best = (1u32, pixels_per_square);
    // 1, 2, 5 and then the same again ten times bigger, which is how every scale bar and every
    // axis anybody has ever read is stepped.
    for decade in 0..8u32 {
        for step in [1u32, 2, 5] {
            let Some(squares) = step.checked_mul(10u32.pow(decade)) else {
                return best;
            };
            let pixels = squares as f32 * pixels_per_square;
            if pixels > max_pixels {
                return best;
            }
            best = (squares, pixels);
        }
    }
    best
}

/// Everything that can be shown.
///
/// An enum rather than a pile of booleans so the View menu is generated from the same list the
/// keyboard uses. The menu and the keys drifting apart is the normal fate of a shortcut table
/// maintained in two places, and this build already had fourteen bindings and no menu at all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Panel {
    Cell,
    Metrics,
    Legend,
    Genome,
    /// Every cost, rate and mutation the world runs on (M10.2).
    Parameters,
    /// The tree of life, the food web and the timeline, sharing one selection (M10.4).
    ///
    /// Was two panels, `Wiki` and `FoodWeb`, which put the tree and the web on opposite sides
    /// of the screen when the question they answer — who is eating whom, and where did they
    /// come from — is one question.
    Ecology,
    Editor,
    Debugger,
    /// The tools and their settings, for building a slide rather than watching one.
    ///
    /// A panel and not a menu, and that is the whole reason it exists. The settings began in the
    /// Tools menu, which closes the moment you click the slide — so adjusting a dose meant open,
    /// change, close, paint, and open again for the next stroke. A thing you adjust *while*
    /// working has to stay on screen while you work.
    Toolbox,
}

/// Which view the ecology pane is showing.
///
/// They share a selection: click a species in the tree and its page follows, click a guild in
/// the web and the tree highlights it. Separate panels could not do that.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ecology {
    /// The phylogenetic tree, drawn as a tree.
    Tree,
    /// Who eats whom, as a layered graph.
    Web,
    /// What happened, and when, and what you changed.
    Timeline,
    /// The parameter changes on the timeline, field by field.
    Interventions,
    /// The world's books: energy in against energy out, and where the matter is.
    ///
    /// In the ecology pane rather than in a pane of its own because it is the same question
    /// the other three ask from different sides — the tree is who is here, the web is who eats
    /// whom, and this is what the whole thing is running on.
    Budget,
}

impl Ecology {
    pub const ALL: [Ecology; 5] = [
        Ecology::Tree,
        Ecology::Web,
        Ecology::Timeline,
        Ecology::Interventions,
        Ecology::Budget,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Ecology::Tree => "tree of life",
            Ecology::Web => "food web",
            Ecology::Timeline => "timeline",
            Ecology::Interventions => "interventions",
            Ecology::Budget => "budget",
        }
    }
}

/// Where a panel sits when it is docked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dock {
    /// The left rail: what is selected.
    Left,
    /// The right rail: what the world is doing.
    Right,
    /// The bottom drawer, one tab at a time. Everything that wants width rather than height.
    Drawer,
}

impl Panel {
    pub const ALL: [Panel; 9] = [
        Panel::Cell,
        Panel::Metrics,
        Panel::Legend,
        Panel::Genome,
        Panel::Ecology,
        Panel::Toolbox,
        Panel::Parameters,
        Panel::Editor,
        Panel::Debugger,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Panel::Cell => "cell",
            Panel::Metrics => "metrics",
            Panel::Legend => "legend",
            Panel::Genome => "genome",
            Panel::Ecology => "ecology",
            Panel::Toolbox => "toolbox",
            Panel::Parameters => "parameters",
            Panel::Editor => "editor",
            Panel::Debugger => "debugger",
        }
    }

    /// The key that toggles it, as the menu spells it.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Panel::Cell => "I",
            Panel::Metrics => "P",
            Panel::Legend => "L",
            Panel::Genome => "G",
            Panel::Ecology => "W",
            Panel::Parameters => ",",
            Panel::Editor => "E",
            Panel::Debugger => "D",
            Panel::Toolbox => "T",
        }
    }

    #[must_use]
    pub fn dock(self) -> Dock {
        match self {
            Panel::Cell => Dock::Left,
            Panel::Metrics | Panel::Legend => Dock::Right,
            Panel::Genome
            | Panel::Ecology
            | Panel::Parameters
            | Panel::Editor
            | Panel::Debugger
            | Panel::Toolbox => Dock::Drawer,
        }
    }
}

/// Which panels are showing.
///
/// The rails are independent switches. The drawer holds one tab at a time, so its state is
/// *which* tab rather than a switch each — pressing `g` shows the genome, pressing it again
/// puts the drawer away, and pressing `e` swaps to the editor without a second keystroke to
/// close the first.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Panels {
    pub cell: bool,
    pub metrics: bool,
    pub legend: bool,
    pub drawer: Option<Panel>,
}

impl Default for Panels {
    fn default() -> Self {
        Panels {
            cell: false,
            metrics: true,
            legend: true,
            drawer: None,
        }
    }
}

impl Panels {
    #[must_use]
    pub fn is_open(&self, panel: Panel) -> bool {
        match panel {
            Panel::Cell => self.cell,
            Panel::Metrics => self.metrics,
            Panel::Legend => self.legend,
            _ => self.drawer == Some(panel),
        }
    }

    pub fn set(&mut self, panel: Panel, open: bool) {
        match panel {
            Panel::Cell => self.cell = open,
            Panel::Metrics => self.metrics = open,
            Panel::Legend => self.legend = open,
            _ if open => self.drawer = Some(panel),
            // Closing a drawer tab only puts the drawer away if it is the tab on show.
            // Otherwise it is a request to close something that is already closed.
            _ => {
                if self.drawer == Some(panel) {
                    self.drawer = None;
                }
            }
        }
    }

    pub fn toggle(&mut self, panel: Panel) {
        self.set(panel, !self.is_open(panel));
    }

    /// Whether the bottom drawer is showing at all.
    #[must_use]
    pub fn drawer_open(&self) -> bool {
        self.drawer.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: Rect = Rect {
        min_x: 200.0,
        min_y: 30.0,
        max_x: 1000.0,
        max_y: 700.0,
    };

    #[test]
    fn a_pointer_in_the_viewport_belongs_to_the_slide() {
        assert_eq!(route(Some((500.0, 400.0)), VIEWPORT, false), Target::Slide);
    }

    #[test]
    fn a_pointer_over_a_rail_never_reaches_the_slide() {
        // The left rail, the right rail, the menu bar and the drawer, in that order. This is
        // the whole complaint: a wheel event here used to zoom the microscope.
        for (x, y) in [
            (100.0, 400.0),
            (1200.0, 400.0),
            (500.0, 10.0),
            (500.0, 900.0),
        ] {
            assert_eq!(
                route(Some((x, y)), VIEWPORT, false),
                Target::Panel,
                "({x}, {y}) leaked to the slide"
            );
        }
    }

    #[test]
    fn a_floating_window_over_the_slide_still_wins() {
        // Inside the viewport rectangle, so the rectangle alone would hand it to the slide.
        // egui is the only thing that knows a torn-off panel is sitting there.
        assert_eq!(route(Some((500.0, 400.0)), VIEWPORT, true), Target::Panel);
    }

    #[test]
    fn no_pointer_is_nobodys() {
        assert_eq!(route(None, VIEWPORT, false), Target::Nowhere);
        // Even when egui claims it: there is nothing to claim.
        assert_eq!(route(None, VIEWPORT, true), Target::Nowhere);
    }

    #[test]
    fn a_viewport_with_no_area_claims_nothing() {
        // The first frame, before egui has laid anything out. Every click would otherwise be
        // reported as a hit on a slide that is not being drawn yet.
        let empty = Rect::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(route(Some((0.0, 0.0)), empty, false), Target::Panel);
    }

    #[test]
    fn the_edges_belong_to_exactly_one_side() {
        // Half-open: the minimum edge is inside, the maximum edge is not. Two panels sharing a
        // border must not both claim the pixel on it.
        assert!(VIEWPORT.contains(VIEWPORT.min_x, VIEWPORT.min_y));
        assert!(!VIEWPORT.contains(VIEWPORT.max_x, VIEWPORT.min_y));
        assert!(!VIEWPORT.contains(VIEWPORT.min_x, VIEWPORT.max_y));
    }

    #[test]
    fn a_drag_that_starts_on_the_slide_keeps_the_slide() {
        let mut focus = Focus::default();
        assert_eq!(focus.press(Target::Slide), Target::Slide);
        // The pointer has wandered over the metrics rail mid-pan. The plate comes with it.
        assert_eq!(focus.resolve(Target::Panel), Target::Slide);
        assert!(focus.is_dragging());
        focus.release();
        assert_eq!(focus.resolve(Target::Panel), Target::Panel);
    }

    #[test]
    fn a_drag_that_starts_on_a_panel_never_pans_the_slide() {
        // The other direction, and the one that was actually broken: dragging a scrollbar
        // used to drag the plate as well.
        let mut focus = Focus::default();
        focus.press(Target::Panel);
        assert_eq!(focus.resolve(Target::Slide), Target::Panel);
    }

    #[test]
    fn zooming_keeps_the_point_under_the_pointer() {
        let centre = (48.0, 48.0);
        let offset = (200.0, -120.0);
        let (old, new) = (8.0, 12.0);
        // What the pointer was over before, in slide coordinates.
        let before = (centre.0 + offset.0 / old, centre.1 + offset.1 / old);
        let moved = zoom_about(centre, offset, old, new);
        let after = (moved.0 + offset.0 / new, moved.1 + offset.1 / new);
        assert!((before.0 - after.0).abs() < 1e-4, "{before:?} {after:?}");
        assert!((before.1 - after.1).abs() < 1e-4, "{before:?} {after:?}");
    }

    #[test]
    fn zooming_from_the_centre_moves_nothing() {
        assert_eq!(
            zoom_about((48.0, 48.0), (0.0, 0.0), 8.0, 20.0),
            (48.0, 48.0)
        );
    }

    #[test]
    fn a_degenerate_scale_leaves_the_camera_alone() {
        // Rather than dividing by it and blanking the window with a NaN centre.
        assert_eq!(zoom_about((1.0, 2.0), (50.0, 50.0), 0.0, 8.0), (1.0, 2.0));
        assert_eq!(zoom_about((1.0, 2.0), (50.0, 50.0), 8.0, 0.0), (1.0, 2.0));
    }

    #[test]
    fn every_panel_has_a_unique_key_and_title() {
        let mut keys: Vec<&str> = Panel::ALL.iter().map(|p| p.key()).collect();
        let count = keys.len();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), count, "two panels share a shortcut");

        let mut titles: Vec<&str> = Panel::ALL.iter().map(|p| p.title()).collect();
        titles.sort_unstable();
        titles.dedup();
        assert_eq!(titles.len(), count, "two panels share a title");
    }

    #[test]
    fn the_drawer_shows_one_tab_at_a_time() {
        let mut panels = Panels::default();
        assert!(!panels.drawer_open());

        panels.toggle(Panel::Genome);
        assert!(panels.is_open(Panel::Genome));

        // Opening another tab swaps, rather than needing the first to be closed by hand.
        panels.toggle(Panel::Editor);
        assert!(panels.is_open(Panel::Editor));
        assert!(!panels.is_open(Panel::Genome));

        // And toggling the tab on show puts the drawer away.
        panels.toggle(Panel::Editor);
        assert!(!panels.drawer_open());
    }

    #[test]
    fn closing_a_tab_that_is_not_showing_does_not_shut_the_drawer() {
        let mut panels = Panels::default();
        panels.set(Panel::Genome, true);
        panels.set(Panel::Editor, false);
        assert!(panels.is_open(Panel::Genome), "the drawer closed itself");
    }

    #[test]
    fn the_rails_are_independent() {
        let mut panels = Panels::default();
        panels.set(Panel::Cell, true);
        panels.set(Panel::Metrics, true);
        panels.toggle(Panel::Metrics);
        assert!(panels.is_open(Panel::Cell));
        assert!(!panels.is_open(Panel::Metrics));
    }

    #[test]
    fn every_panel_docks_somewhere_it_fits() {
        // The drawer is for anything that wants width — a listing, a tree, a source buffer.
        // The rails are for anything that wants height. Getting this backwards is how a
        // genome listing ends up forty characters wide in a side panel.
        for panel in Panel::ALL {
            let dock = panel.dock();
            match panel {
                Panel::Cell => assert_eq!(dock, Dock::Left),
                Panel::Metrics | Panel::Legend => assert_eq!(dock, Dock::Right),
                _ => assert_eq!(dock, Dock::Drawer, "{} is not in the drawer", panel.title()),
            }
        }
    }
}

#[cfg(test)]
mod rail_tests {
    use super::*;

    #[test]
    fn a_rail_with_no_room_is_not_drawn_at_all() {
        // The drawer dragged to fill the window. Zero is the boundary case; negative is what
        // actually happens, because the drawer is allowed to ask for more than is left.
        assert!(!rails_fit(0.0));
        assert!(!rails_fit(-120.0));
        assert!(!rails_fit(RAIL_MIN_HEIGHT - 1.0));
    }

    #[test]
    fn an_ordinary_window_has_room_for_its_rails() {
        assert!(rails_fit(RAIL_MIN_HEIGHT));
        assert!(rails_fit(640.0));
    }
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn a_bar_never_exceeds_the_room_it_is_given() {
        for ppq in [0.4f32, 1.0, 8.0, 63.5, 320.0] {
            for room in [40.0f32, 112.0, 300.0] {
                let (squares, pixels) = scale_bar(ppq, room);
                assert!(pixels <= room || squares == 1, "{ppq} in {room}: {pixels}");
                assert!(squares >= 1);
            }
        }
    }

    #[test]
    fn a_bar_is_a_number_somebody_would_say() {
        // 1, 2, 5 and the same again ten times bigger. A bar of 37 squares is arithmetic
        // showing through, and the whole point of the thing is to be read at a glance.
        for ppq in [0.05f32, 0.7, 3.0, 8.0, 100.0] {
            let (squares, _) = scale_bar(ppq, 112.0);
            let mantissa = {
                let mut s = squares;
                while s % 10 == 0 && s > 1 {
                    s /= 10;
                }
                s
            };
            assert!(
                matches!(mantissa, 1 | 2 | 5),
                "{squares} squares is not a round number"
            );
        }
    }

    #[test]
    fn zooming_in_shortens_the_bar_in_squares() {
        // The bar measures the same *distance* until it cannot, so magnifying can only ever
        // reduce how many squares fit in it.
        let mut previous = u32::MAX;
        for ppq in [0.5f32, 1.0, 2.0, 4.0, 8.0, 16.0, 64.0, 256.0] {
            let (squares, _) = scale_bar(ppq, 112.0);
            assert!(squares <= previous, "{ppq} gave {squares} after {previous}");
            previous = squares;
        }
    }

    #[test]
    fn a_degenerate_scale_does_not_divide_by_it() {
        // Reached from a saved view, so it cannot assume the caller's clamp held.
        assert_eq!(scale_bar(0.0, 112.0), (1, 0.0));
        assert_eq!(scale_bar(-1.0, 112.0), (1, 0.0));
        assert_eq!(scale_bar(8.0, 0.0), (1, 0.0));
        assert_eq!(scale_bar(f32::NAN, 112.0), (1, 0.0));
    }

    #[test]
    fn a_square_wider_than_the_bar_still_reports_one() {
        // Zoomed until one square is wider than the whole status bar. One square is the
        // smallest true statement available, so it is what gets made.
        let (squares, pixels) = scale_bar(400.0, 112.0);
        assert_eq!(squares, 1);
        assert_eq!(pixels, 400.0);
    }
}

#[cfg(test)]
mod stroke_tests {
    use super::*;

    #[test]
    fn a_single_square_is_its_own_line() {
        assert_eq!(line_squares((4, 7), (4, 7)), vec![(4, 7)]);
    }

    #[test]
    fn a_straight_run_has_no_gaps_in_it() {
        // The failure this exists to prevent: a barrier with holes is not a barrier, because
        // both the fluid and now the cells go straight through them.
        let run = line_squares((0, 3), (5, 3));
        assert_eq!(run, vec![(0, 3), (1, 3), (2, 3), (3, 3), (4, 3), (5, 3)]);

        let down = line_squares((2, 0), (2, 4));
        assert_eq!(down, vec![(2, 0), (2, 1), (2, 2), (2, 3), (2, 4)]);
    }

    #[test]
    fn a_diagonal_is_contiguous_and_ends_where_it_was_told_to() {
        let d = line_squares((0, 0), (4, 4));
        assert_eq!(d.first(), Some(&(0, 0)));
        assert_eq!(d.last(), Some(&(4, 4)));
        // Every step moves by one square in at least one axis and never more than one in
        // either, which is what "contiguous" means for a wall.
        for pair in d.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(
                (a.0 - b.0).abs() <= 1 && (a.1 - b.1).abs() <= 1,
                "{a:?} to {b:?} jumps"
            );
        }
    }

    #[test]
    fn it_runs_in_every_direction() {
        // Backwards and upwards are the cases an off-by-one in the sign gets wrong, and a
        // barrier tool is dragged in all four.
        for (from, to) in [
            ((5, 5), (0, 5)),
            ((5, 5), (5, 0)),
            ((5, 5), (0, 0)),
            ((0, 5), (5, 0)),
        ] {
            let line = line_squares(from, to);
            assert_eq!(line.first(), Some(&from), "{from:?} -> {to:?}");
            assert_eq!(line.last(), Some(&to), "{from:?} -> {to:?}");
        }
    }

    #[test]
    fn a_shallow_line_steps_the_long_axis_every_square() {
        // Two along for one down. The long axis must not stall, or the wall gets a gap.
        let line = line_squares((0, 0), (6, 2));
        let xs: Vec<i32> = line.iter().map(|p| p.0).collect();
        assert_eq!(xs, vec![0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_wild_coordinate_stops_rather_than_hanging() {
        // The pointer's slide coordinate is unbounded; the walk must not be.
        let line = line_squares((i32::MIN, 0), (i32::MAX, 0));
        assert_eq!(line.len(), MAX_STROKE);
        assert_eq!(line.first(), Some(&(i32::MIN, 0)));
    }
}

#[cfg(test)]
mod brush_tests {
    use super::*;

    fn width_at_row(squares: &[(i32, i32)], row: i32) -> i32 {
        let xs: Vec<i32> = squares
            .iter()
            .filter(|(_, y)| *y == row)
            .map(|(x, _)| *x)
            .collect();
        match (xs.iter().min(), xs.iter().max()) {
            (Some(lo), Some(hi)) => hi - lo + 1,
            _ => 0,
        }
    }

    #[test]
    fn a_brush_of_one_is_the_square_under_the_pointer() {
        assert_eq!(brush_squares((5, 9), 1), vec![(5, 9)]);
    }

    #[test]
    fn a_brush_is_as_wide_as_it_says_through_its_middle() {
        for width in BRUSH_MIN..=BRUSH_MAX {
            let s = brush_squares((0, 0), width);
            let across = width_at_row(&s, 0);
            // Even widths cannot be centred on a square, so they come out one wider through the
            // middle. Odd ones are exact, and those are the ones the default sits among.
            let want = if width % 2 == 0 { width + 1 } else { width } as i32;
            assert_eq!(
                across, want,
                "a brush of {width} is {across} squares across"
            );
        }
    }

    #[test]
    fn a_brush_is_a_disc_and_not_a_box() {
        // A box brush mitres every corner of a freehand stroke. At ten across, a disc drops the
        // extreme corners; a box would keep them.
        let s = brush_squares((0, 0), 10);
        assert!(!s.contains(&(5, 5)), "the corner is in it, so it is a box");
        assert!(
            s.contains(&(5, 0)) && s.contains(&(0, 5)),
            "the axes are missing"
        );
        // And it is symmetric, or a stroke would drift as it is stamped.
        for &(x, y) in &s {
            assert!(
                s.contains(&(-x, y)) && s.contains(&(x, -y)),
                "({x},{y}) is not mirrored"
            );
        }
    }

    #[test]
    fn a_brush_is_solid_with_no_holes_in_it() {
        // A wall with a hole is not a wall. Every row the brush touches must be one unbroken
        // run, or the fluid and the cells go through the gap.
        for width in BRUSH_MIN..=BRUSH_MAX {
            let s = brush_squares((0, 0), width);
            let rows: std::collections::BTreeSet<i32> = s.iter().map(|(_, y)| *y).collect();
            for row in rows {
                let mut xs: Vec<i32> = s
                    .iter()
                    .filter(|(_, y)| *y == row)
                    .map(|(x, _)| *x)
                    .collect();
                xs.sort_unstable();
                for pair in xs.windows(2) {
                    assert_eq!(pair[1] - pair[0], 1, "brush {width} row {row} has a gap");
                }
            }
        }
    }

    #[test]
    fn a_width_out_of_range_is_clamped_rather_than_believed() {
        // Reached from a saved setting, so it cannot assume the caller was reasonable.
        assert_eq!(brush_squares((0, 0), 0), brush_squares((0, 0), BRUSH_MIN));
        assert_eq!(
            brush_squares((0, 0), 9_999),
            brush_squares((0, 0), BRUSH_MAX)
        );
    }

    #[test]
    fn a_brush_at_the_edge_of_the_world_does_not_overflow() {
        // The caller drops the negatives; getting here must not panic on the way.
        let s = brush_squares((i32::MIN, i32::MAX), BRUSH_MAX);
        assert!(!s.is_empty());
    }
}

#[cfg(test)]
mod overlay_tests {
    use super::*;

    #[test]
    fn stepping_forward_walks_the_chemicals_and_then_goes_dark() {
        let mut mask = 0u32;
        // Off -> the first chemical.
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0b0001);
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0b0010);
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0b0100);
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0b1000);
        // Past the last one is bare slide, which is a reading and not a dead end.
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0, "the cycle has no off position");
        mask = step_solo(mask, 4, 1);
        assert_eq!(mask, 0b0001, "the cycle did not come round");
    }

    #[test]
    fn stepping_back_is_the_exact_reverse() {
        for count in 1..=16usize {
            let mut mask = 0u32;
            for _ in 0..count + 1 {
                mask = step_solo(mask, count, 1);
            }
            let forward = mask;
            let back = step_solo(step_solo(forward, count, 1), count, -1);
            assert_eq!(back, forward, "count {count} does not step back cleanly");
        }
    }

    #[test]
    fn it_solos_rather_than_toggling() {
        // Three on at once; one step leaves exactly one on. Comparing two fields means seeing
        // one at a time, and a step that added to the set would never get you there.
        let next = step_solo(0b1011, 8, 1);
        assert_eq!(
            next.count_ones(),
            1,
            "stepping left more than one overlay on"
        );
    }

    #[test]
    fn a_mask_with_several_bits_starts_from_the_lowest() {
        // No single "current" to advance from, so it picks the lowest rather than guessing.
        assert_eq!(step_solo(0b1010, 8, 1), 1 << 2);
    }

    #[test]
    fn a_silly_count_does_not_panic_or_shift_off_the_end() {
        for count in [0usize, 1, 32, 999] {
            for step in [-1i32, 1] {
                let m = step_solo(0, count, step);
                assert!(m.count_ones() <= 1);
            }
        }
    }
}

#[cfg(test)]
mod all_none_tests {
    use super::*;

    #[test]
    fn nothing_on_turns_everything_on() {
        assert_eq!(toggle_all(0, 16), 0b1111_1111_1111_1111);
        assert_eq!(toggle_all(0, 4), 0b1111);
    }

    #[test]
    fn anything_on_clears_the_lot() {
        // Any at all, not just a full set — the button is always the way out of whatever is on
        // the plate, so one overlay showing does not turn the other fifteen on.
        assert_eq!(toggle_all(0b0001, 16), 0);
        assert_eq!(toggle_all(0b1010_0000, 16), 0);
        assert_eq!(toggle_all(all_overlays(16), 16), 0);
    }

    #[test]
    fn it_round_trips() {
        for count in [1usize, 4, 9, 16] {
            let all = toggle_all(0, count);
            assert_eq!(all.count_ones() as usize, count);
            assert_eq!(toggle_all(all, count), 0);
        }
    }

    #[test]
    fn a_count_at_or_past_the_width_of_the_mask_does_not_shift_off_the_end() {
        // `1 << 32` is undefined, and the count comes out of a scenario file.
        assert_eq!(all_overlays(32), u32::MAX);
        assert_eq!(all_overlays(999), u32::MAX);
        assert_eq!(all_overlays(0), 0);
    }
}
