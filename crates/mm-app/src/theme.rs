//! The palette, the type scale and the measurements of a row (M10.6, `docs/UI.md` §8).
//!
//! **No egui in here**, for the same reason [`crate::ui`] has none. A palette is a table of
//! numbers and a set of rules about which number goes where, and none of that needs a window
//! to check — which matters because the rules are exactly the kind that rot silently. Nobody
//! notices that the accent has quietly become "the colour of a heading I wanted to stand out"
//! until it means nothing anywhere. Here it is a test.
//!
//! `main.rs` converts to `egui::Color32` and `egui::FontId` at the boundary and nowhere else.
//!
//! # Why this module exists at all
//!
//! The interface had no theme. Not a sparse one — none: in six thousand lines of `main.rs`
//! there was no `set_visuals`, no `set_fonts` and no `Style`, and the only styling anywhere in
//! the crate was three lines making one `DragValue`'s background transparent. Everything that
//! looked dated about the window was egui's default dark theme drawing an interface that had
//! never asked it for anything.

/// A colour, as the interface stores it. `main.rs` turns these into `Color32`.
pub type Rgb = [u8; 3];

/// What a thing is drawn *on*.
///
/// Four, and they are ordered darkest first, because the contrast floor has to hold against
/// the worst of them and it is useful to be able to see at a glance which that is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ground {
    /// Behind the viewport, and the drawer's darkest ground.
    Slide,
    /// Every rail and the drawer.
    Panel,
    /// The trough of a bar, the well of a value field.
    Sunk,
    /// A hovered menu item, a pressed chip.
    Raised,
}

impl Ground {
    pub const ALL: [Ground; 4] = [Ground::Slide, Ground::Panel, Ground::Sunk, Ground::Raised];

    #[must_use]
    pub const fn rgb(self) -> Rgb {
        match self {
            Ground::Slide => [0x08, 0x09, 0x0b],
            Ground::Panel => [0x0e, 0x10, 0x13],
            Ground::Sunk => [0x12, 0x15, 0x1a],
            Ground::Raised => [0x1b, 0x1f, 0x25],
        }
    }
}

/// A border between two regions — a rail against the viewport, the drawer against the rails.
pub const RULE: Rgb = [0x23, 0x27, 0x2e];

/// A rule *inside* a region, between two groups of the same panel. Fainter than [`RULE`] on
/// purpose: a hairline that is as strong as a region border reads as a region border, and a
/// rail with six of them reads as six panels.
pub const HAIR: Rgb = [0x1a, 0x1e, 0x24];

/// What a piece of text is for.
///
/// A role is a *job*, not a size. Two roles may share a size — [`Role::Label`] and
/// [`Role::Value`] do, because they sit on the same line and a step between them would make
/// the line look broken — but no two may be identical in all of size, family and ink, because
/// a role you cannot tell from another role is not doing a job.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// A group's name, above it: `LOADOUT`, `INTERIOR CHEMISTRY`. Letterspaced mono caps.
    Section,
    /// Prose. The only role that is allowed to be a sentence.
    Body,
    /// The name of a thing whose value is next to it.
    Label,
    /// The value. Every number in the interface is this or [`Role::Code`].
    Value,
    /// An aside, a unit, a hint under a control.
    Small,
    /// A genome listing or a source buffer, where `inspector::Ink` picks the colour instead.
    Code,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Section,
        Role::Body,
        Role::Label,
        Role::Value,
        Role::Small,
        Role::Code,
    ];

    /// Points, before egui's `pixels_per_point`.
    ///
    /// Nothing below 10. egui rasterises through `ab_glyph` with no hinting, so the design's
    /// 9.5px — a browser's number, and fine in a browser — is mush on a 1× display, and the
    /// text it was proposed for is the dim text that is hardest to read already.
    #[must_use]
    pub const fn size(self) -> f32 {
        match self {
            Role::Section => 10.0,
            Role::Small => 10.5,
            Role::Label | Role::Value => 11.0,
            Role::Code => 11.5,
            Role::Body => 12.0,
        }
    }

    /// Whether it is set in the monospace family.
    ///
    /// The rule this encodes: **every number is monospace**. A rail of readings that do not
    /// line up looks like a form; the same numbers in a mono column can be compared down the
    /// page without moving your head, which is the whole difference between a settings dialogue
    /// and an instrument. [`Role::Body`] and [`Role::Small`] are the prose roles and are the
    /// only two that are not mono.
    #[must_use]
    pub const fn mono(self) -> bool {
        !matches!(self, Role::Body | Role::Small)
    }

    /// Extra space between letters, in points. Only [`Role::Section`] has any: it is what makes
    /// ten-point caps read as a heading rather than as shouting.
    #[must_use]
    pub const fn tracking(self) -> f32 {
        match self {
            Role::Section => 1.0,
            _ => 0.0,
        }
    }

    /// Whether the text is upper-cased before it is drawn. Section headers are written in the
    /// source the way they are read aloud, and cased here, so that the strings stay greppable.
    #[must_use]
    pub const fn caps(self) -> bool {
        matches!(self, Role::Section)
    }

    /// The colour it is drawn in.
    ///
    /// [`Role::Code`] has none — a listing's colour comes from `inspector::Ink`, which knows
    /// what each token *is*, and a second opinion here would only be able to disagree with it.
    #[must_use]
    pub const fn ink(self) -> Option<Rgb> {
        match self {
            Role::Value => Some([0xe6, 0xe9, 0xed]),
            Role::Body => Some([0xc6, 0xcd, 0xd5]),
            Role::Label => Some([0x79, 0x82, 0x8d]),
            Role::Section | Role::Small => Some(DIM),
            Role::Code => None,
        }
    }
}

/// Anything you read second: section headers, units, keys, the tick a menu shortcut names.
///
/// Lighter than the design's `#4e565f`, which came out at 2.6:1 against the panel it sits on
/// and 2.2:1 against a hovered one — atmospheric in a browser mock on a good monitor, and not
/// text on a laptop at an angle. This is the darkest grey that clears 3:1 against all four
/// grounds, and `no_role_is_unreadable_on_any_ground_it_occurs_on` is what found that out.
pub const DIM: Rgb = [0x64, 0x6d, 0x78];

/// The accent, and the only place it is written down.
///
/// Private, and reachable only through [`selected`], because a palette dies the day its accent
/// becomes "the colour of things I want to stand out". Here it has one meaning — *this is on,
/// or this is chosen* — and [`PALETTE`] plus the test below are what keep it that way.
const ACCENT: Rgb = [0x7f, 0xa8, 0xc0];

/// The colour of a thing that is on, chosen, or selected. See [`ACCENT`].
#[must_use]
pub const fn selected() -> Rgb {
    ACCENT
}

/// What to write *on* a selected chip, whose ground is [`selected`]. Near-black, because the
/// accent is light enough that ink on it has to go the other way.
#[must_use]
pub const fn on_selected() -> Rgb {
    Ground::Slide.rgb()
}

/// How a reading feels, where a reading can feel like anything.
///
/// Three, and they are about the world rather than about severity: `Good` is income and a clean
/// assemble, `Warn` is an edit you have not applied and an intervention you did make, `Bad` is
/// damage and a loop that is not closing. A fourth would be a fourth thing to have an opinion
/// about, and there is no fourth thing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mood {
    Good,
    Warn,
    Bad,
}

impl Mood {
    pub const ALL: [Mood; 3] = [Mood::Good, Mood::Warn, Mood::Bad];

    #[must_use]
    pub const fn rgb(self) -> Rgb {
        match self {
            Mood::Good => [0x8a, 0xb0, 0x98],
            Mood::Warn => [0xc8, 0xa2, 0x5a],
            Mood::Bad => [0xc4, 0x73, 0x6e],
        }
    }
}

/// Every colour this module hands out, named.
///
/// It exists so that "the accent means exactly one thing" can be checked rather than believed:
/// a colour that appears here twice is a colour with two meanings, and the test says so. Keep
/// it in step when adding a colour — a colour that is not in this list is not checked by
/// anything.
pub const PALETTE: [(&str, Rgb); 13] = [
    ("slide", Ground::Slide.rgb()),
    ("panel", Ground::Panel.rgb()),
    ("sunk", Ground::Sunk.rgb()),
    ("raised", Ground::Raised.rgb()),
    ("rule", RULE),
    ("hair", HAIR),
    ("value", [0xe6, 0xe9, 0xed]),
    ("body", [0xc6, 0xcd, 0xd5]),
    ("label", [0x79, 0x82, 0x8d]),
    ("dim", DIM),
    ("good", [0x8a, 0xb0, 0x98]),
    ("warn", [0xc8, 0xa2, 0x5a]),
    ("bad", [0xc4, 0x73, 0x6e]),
];

/// The smallest contrast ratio any text in the interface is allowed to have against the thing
/// it is drawn on.
///
/// WCAG's floor for text that is not body copy. Everything that *is* body copy — [`Role::Body`]
/// and the values you actually read numbers off — clears 4.5 with room to spare; the floor is
/// here for the dim end, which is where a near-black theme goes wrong.
pub const CONTRAST_FLOOR: f32 = 3.0;

/// WCAG relative luminance, and then the ratio between two of them.
///
/// Floats, which are fine here: this is `mm-app`. The formula is the published one rather than
/// a cheaper approximation because the whole value of the check is that it agrees with what
/// everyone else means by contrast.
#[must_use]
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    fn luminance(c: Rgb) -> f32 {
        fn channel(v: u8) -> f32 {
            let v = f32::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])
    }
    let (a, b) = (luminance(a), luminance(b));
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };
    (hi + 0.05) / (lo + 0.05)
}

/// The measurements of the three-column row that every quantity in the interface uses.
///
/// ```text
/// label ······· ▁▁▁▁▁▁▁▁▁▁▁▁ ····· 89.0
/// LABEL          bar, 5px          VALUE, right
/// ```
///
/// The label is *outside* the bar. It was inside, on a sixteen-pixel slab, which is a Windows
/// 95 progress bar and was the most dated object in the window.
pub mod row {
    /// How tall a bar is. A hairline, because it is a reading and not a container.
    pub const BAR: f32 = 5.0;
    /// How tall a row of label-bar-value is, bar included.
    pub const HEIGHT: f32 = 16.0;
    /// The label column.
    pub const LABEL: f32 = 62.0;
    /// The value column, right-aligned inside it.
    pub const VALUE: f32 = 52.0;
    /// Between any two of the three columns.
    pub const GUTTER: f32 = 8.0;

    /// The change this module exists to make: five points of bar inside a sixteen-point row,
    /// rather than a sixteen-point slab with the label written on top of it. A compile error
    /// rather than a test, because it is a statement about two constants and can be settled
    /// where they are written.
    const _: () = assert!(BAR < HEIGHT / 2.0, "the bar is a slab again");
}

/// Above a section header, so groups have air between them without needing a rule each.
pub const SECTION_GAP: f32 = 10.0;

/// The drawer's context column: the fixed strip on the right of every drawer tab that holds
/// whatever the work area cannot say about itself. See `docs/UI.md` §8.6.
pub const CONTEXT_COLUMN: f32 = 300.0;

/// The parameter editor's group rail, on the left, which is narrower because it holds one word.
pub const GROUP_COLUMN: f32 = 150.0;

#[cfg(test)]
mod tests {
    use super::*;

    /// Every role/ground pair the interface actually draws.
    ///
    /// Not the cross product: [`Role::Code`] has no ink of its own, and a section header is
    /// never drawn on a hovered menu item. The list is the honest one — a floor that holds for
    /// pairs nobody draws is not evidence about the ones they do.
    const PAIRS: [(Role, Ground); 16] = [
        (Role::Section, Ground::Slide),
        (Role::Section, Ground::Panel),
        (Role::Body, Ground::Slide),
        (Role::Body, Ground::Panel),
        (Role::Body, Ground::Raised),
        (Role::Label, Ground::Slide),
        (Role::Label, Ground::Panel),
        (Role::Label, Ground::Sunk),
        (Role::Label, Ground::Raised),
        (Role::Value, Ground::Slide),
        (Role::Value, Ground::Panel),
        (Role::Value, Ground::Sunk),
        (Role::Value, Ground::Raised),
        (Role::Small, Ground::Slide),
        (Role::Small, Ground::Panel),
        (Role::Small, Ground::Raised),
    ];

    #[test]
    fn no_role_is_unreadable_on_any_ground_it_occurs_on() {
        // The failure a near-black theme actually has. The design's dim grey came out at 2.56
        // against the panel it sits on, which is how `DIM` ended up lighter than the mock's.
        for (role, ground) in PAIRS {
            let Some(ink) = role.ink() else { continue };
            let ratio = contrast(ink, ground.rgb());
            assert!(
                ratio >= CONTRAST_FLOOR,
                "{role:?} on {ground:?} is {ratio:.2}:1, below the {CONTRAST_FLOOR}:1 floor"
            );
        }
    }

    #[test]
    fn the_text_you_read_numbers_off_clears_the_body_floor_too() {
        // 3:1 is the floor for incidental text. A value is not incidental — it is the thing
        // the panel exists to show — so it clears 4.5 on every ground it is drawn on.
        for ground in Ground::ALL {
            for role in [Role::Value, Role::Body] {
                let ink = role.ink().expect("a text role");
                let ratio = contrast(ink, ground.rgb());
                assert!(ratio >= 4.5, "{role:?} on {ground:?} is only {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_accent_means_one_thing() {
        // The way a palette dies: the accent becomes "the colour of things I want to stand
        // out", and within a month it is on a heading, a hover and a chart line, and a
        // selected row no longer reads as selected because four other things look the same.
        assert!(
            !PALETTE.iter().any(|(_, rgb)| *rgb == ACCENT),
            "the accent has been given a second name in PALETTE"
        );
        for role in Role::ALL {
            assert_ne!(role.ink(), Some(ACCENT), "{role:?} is drawn in the accent");
        }
        for ground in Ground::ALL {
            assert_ne!(ground.rgb(), ACCENT, "{ground:?} is the accent");
        }
        for mood in Mood::ALL {
            assert_ne!(mood.rgb(), ACCENT, "{mood:?} is the accent");
        }
    }

    #[test]
    fn no_two_colours_share_a_name_or_a_value() {
        // A duplicated value in PALETTE is two names for one colour, which means one of them is
        // about to drift; a duplicated name is a rename half-done.
        for (i, (name, rgb)) in PALETTE.iter().enumerate() {
            for (other, other_rgb) in PALETTE.iter().skip(i + 1) {
                assert_ne!(name, other, "two entries called {name}");
                assert_ne!(rgb, other_rgb, "{name} and {other} are the same colour");
            }
        }
    }

    #[test]
    fn no_role_is_indistinguishable_from_another() {
        // Two roles may share a size — a label and its value sit on one line and a step between
        // them would make the line look broken — but two that match in size, family *and* ink
        // are one role with two names, and the second one will be used at random.
        for (i, role) in Role::ALL.iter().enumerate() {
            for other in Role::ALL.iter().skip(i + 1) {
                let same = role.size() == other.size()
                    && role.mono() == other.mono()
                    && role.ink() == other.ink();
                assert!(!same, "{role:?} and {other:?} draw identically");
            }
        }
    }

    #[test]
    fn every_number_is_monospace() {
        // The rule the whole type scale is for. The prose roles are the only two exceptions,
        // and they are exceptions because prose in a mono face is what a terminal looks like.
        for role in Role::ALL {
            assert_eq!(
                role.mono(),
                !matches!(role, Role::Body | Role::Small),
                "{role:?} is on the wrong side of the mono rule"
            );
        }
    }

    #[test]
    fn nothing_is_set_below_ten_points() {
        // egui rasterises unhinted, so this is not a taste floor — it is where dim text stops
        // being text on a 1× display.
        for role in Role::ALL {
            assert!(role.size() >= 10.0, "{role:?} is {}pt", role.size());
        }
    }

    #[test]
    fn only_a_section_header_is_letterspaced_and_capsed() {
        for role in Role::ALL {
            let heading = role == Role::Section;
            assert_eq!(role.tracking() > 0.0, heading, "{role:?} tracking");
            assert_eq!(role.caps(), heading, "{role:?} caps");
        }
    }

    #[test]
    fn a_rule_between_regions_is_stronger_than_one_inside_a_region() {
        // Six hairlines as strong as a region border make a rail read as six panels.
        let panel = Ground::Panel.rgb();
        assert!(
            contrast(RULE, panel) > contrast(HAIR, panel),
            "the inner hairline is as loud as the region border"
        );
    }

    #[test]
    fn contrast_is_the_published_formula() {
        // Anchored against the two values everyone can check: black on white is 21:1, and a
        // colour against itself is 1:1.
        let ratio = contrast([0, 0, 0], [255, 255, 255]);
        assert!((ratio - 21.0).abs() < 0.01, "{ratio}");
        assert!((contrast(DIM, DIM) - 1.0).abs() < 1e-6);
        // And it does not care which way round it is asked.
        assert_eq!(contrast([0, 0, 0], DIM), contrast(DIM, [0, 0, 0]));
    }

    #[test]
    fn the_grounds_get_lighter_in_the_order_they_are_listed() {
        // `Ground::ALL` is darkest first, and the contrast floor is checked against the worst
        // of them; if the order stops being true the check stops meaning what it says.
        let mut previous = 0.0;
        for ground in Ground::ALL {
            let against_white = contrast(ground.rgb(), [255, 255, 255]);
            assert!(
                against_white < previous || previous == 0.0,
                "{ground:?} is not darker than the one before it"
            );
            previous = against_white;
        }
    }

}
