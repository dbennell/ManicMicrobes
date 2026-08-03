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
    pub const ALL: [Panel; 8] = [
        Panel::Cell,
        Panel::Metrics,
        Panel::Legend,
        Panel::Genome,
        Panel::Ecology,
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
            | Panel::Debugger => Dock::Drawer,
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
