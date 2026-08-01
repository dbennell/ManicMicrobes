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
}

impl Ecology {
    pub const ALL: [Ecology; 4] = [
        Ecology::Tree,
        Ecology::Web,
        Ecology::Timeline,
        Ecology::Interventions,
    ];

    #[must_use]
    pub fn title(self) -> &'static str {
        match self {
            Ecology::Tree => "tree of life",
            Ecology::Web => "food web",
            Ecology::Timeline => "timeline",
            Ecology::Interventions => "interventions",
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
