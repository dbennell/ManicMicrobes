//! [`crate::theme`] in egui's vocabulary: the style, the fonts and the six widgets everything
//! else is built from (M10.6, `docs/UI.md` §8).
//!
//! The whole of the conversion lives here. `theme.rs` is a table of numbers with no graphics
//! dependency and no opinion about egui; this is the one file allowed to turn one of its
//! `[u8; 3]`s into a `Color32`, and the reason for the split is that the rules worth testing —
//! the accent means one thing, no role is unreadable, every number is monospace — are rules
//! about the table and not about the toolkit.
//!
//! # Why the widgets are here and not in `main.rs`
//!
//! Because there are eleven panels and each of them draws rows of label-and-number, and the
//! way an interface stops looking like an instrument is that the eleventh one is written from
//! memory rather than from the ten before it. [`row`] and [`section`] are how a rail gets its
//! grammar without anyone having to remember the grammar.
//!
//! Nothing in here draws the slide. See UI.md §8.1: the renderer is finished work and this
//! milestone does not open it.

use crate::theme::{self, Ground, Mood, Rgb, Role};
use bevy_egui::egui;

/// A palette colour, as egui wants it.
#[must_use]
pub fn col(rgb: Rgb) -> egui::Color32 {
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// The font a role is set in.
///
/// This is where "every number is monospace" actually happens. `Role::mono` decides the family
/// and nothing downstream is allowed a second opinion.
#[must_use]
pub fn font(role: Role) -> egui::FontId {
    egui::FontId::new(
        role.size(),
        if role.mono() {
            egui::FontFamily::Monospace
        } else {
            egui::FontFamily::Proportional
        },
    )
}

/// Text in a role: its font, its ink, its tracking and — for a section header — its case.
///
/// Casing happens here rather than in the source so that the strings stay greppable: a header
/// is written `"interior chemistry"` where a reader would say it, and comes out as the caps the
/// design asks for.
#[must_use]
pub fn text(role: Role, s: impl Into<String>) -> egui::RichText {
    let s = s.into();
    let s = if role.caps() { s.to_uppercase() } else { s };
    let mut rich = egui::RichText::new(s).font(font(role));
    if let Some(ink) = role.ink() {
        rich = rich.color(col(ink));
    }
    if role.tracking() > 0.0 {
        rich = rich.extra_letter_spacing(role.tracking());
    }
    rich
}

/// The same, in a mood: a value that is good, unapplied, or wrong.
#[must_use]
pub fn moody(role: Role, mood: Mood, s: impl Into<String>) -> egui::RichText {
    text(role, s).color(col(mood.rgb()))
}

/// Dress the context in the theme. Called once, at startup.
///
/// Everything here was egui's default until M10.6, which is the entire explanation for how the
/// window looked. The defaults are good defaults; they are just somebody else's.
pub fn apply(ctx: &egui::Context) {
    // Pinned dark, and then written into *both* styles. The microscope is a dark instrument on
    // any desktop; following the system into a light theme would put white panels either side
    // of the plate, which is the one thing §1 asks the chrome not to do.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.all_styles_mut(dress);
}

fn dress(style: &mut egui::Style) {
    style.text_styles = [
        (egui::TextStyle::Small, font(Role::Small)),
        (egui::TextStyle::Body, font(Role::Body)),
        (egui::TextStyle::Monospace, font(Role::Value)),
        (egui::TextStyle::Button, font(Role::Body)),
        // Nothing in the interface uses a heading — the section header is `Role::Section` and
        // is drawn as a label — but egui resolves this style whether or not anything asks for
        // it, and a 20pt default left in place is a trap for the first person to type
        // `RichText::heading`.
        (egui::TextStyle::Heading, font(Role::Body)),
    ]
    .into();

    // No fades. egui animates a window in over its `animation_time`, and a sheet fading in over
    // a lit microscope slide is transparent for every frame of the fade — which is exactly the
    // ghost-text failure `docs/UI.md` §4 recorded when the parameter editor was a floating
    // window. Nothing else in this interface animates and nothing wants to: a panel you can
    // read the slide through is a panel you cannot read.
    //
    // It also makes a screenshot deterministic, which matters more here than in most
    // applications, because several of this project's tests *are* screenshots.
    style.animation_time = 0.0;

    let s = &mut style.spacing;
    // Tighter than egui's 8×3. The rails are lists of readings and the readings want to be
    // near each other; the air belongs between *groups*, which is what `SECTION_GAP` is for.
    s.item_spacing = egui::vec2(6.0, 3.0);
    s.button_padding = egui::vec2(7.0, 2.0);
    s.interact_size.y = 18.0;
    // Four points either side, not zero. A menu item is a full-width button and puts its
    // shortcut hard against its own right edge, so with no margin the keys were painted on the
    // popup's border. `menu_caption` and `menu_rule` then indent by `button_padding.x` on top of
    // this, so a caption, a label and a rule all begin at the same x.
    s.menu_margin = egui::Margin::symmetric(4, 4);
    s.slider_width = 130.0;
    s.slider_rail_height = theme::row::BAR;
    s.icon_width = 11.0;
    s.icon_width_inner = 6.0;

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = col(Ground::Panel.rgb());
    v.window_fill = col(Ground::Panel.rgb());
    v.extreme_bg_color = col(Ground::Sunk.rgb());
    v.faint_bg_color = col(Ground::Sunk.rgb());
    v.code_bg_color = col(Ground::Sunk.rgb());
    v.window_stroke = egui::Stroke::new(1.0, col(theme::RULE));
    v.window_corner_radius = egui::CornerRadius::same(4);
    v.menu_corner_radius = egui::CornerRadius::same(4);
    v.hyperlink_color = col(theme::selected());
    v.warn_fg_color = col(Mood::Warn.rgb());
    v.error_fg_color = col(Mood::Bad.rgb());
    // A hairline between menu groups rather than egui's full-width separator, which at this
    // row height cuts a menu into slices rather than grouping it.
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, col(theme::HAIR));

    // The design's 44px drop shadow is a web modal's shadow. At a 23px menu row it swamps the
    // menu it is under, so it comes down to something that reads as depth and not as weather.
    let shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 16,
        spread: 0,
        color: egui::Color32::from_black_alpha(160),
    };
    v.window_shadow = shadow;
    v.popup_shadow = shadow;

    // Selection is the accent, and it is the only thing that is.
    v.selection.bg_fill = col(theme::selected()).gamma_multiply(0.55);
    v.selection.stroke = egui::Stroke::new(1.0, col(theme::on_selected()));

    let w = &mut style.visuals.widgets;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.corner_radius = egui::CornerRadius::same(3);
        state.expansion = 0.0;
        state.fg_stroke.color = col(Role::Body.ink().unwrap_or(theme::DIM));
    }
    w.noninteractive.fg_stroke.color = col(theme::DIM);
    w.noninteractive.bg_fill = col(Ground::Panel.rgb());
    w.noninteractive.weak_bg_fill = col(Ground::Panel.rgb());

    // A slider's rail is drawn in `inactive.bg_fill`, so it is the trough and not the well —
    // same reasoning as `theme::TROUGH`, and the same fix: a track you cannot see turns the
    // handle into a dot with nothing behind it.
    w.inactive.bg_fill = col(theme::TROUGH);
    w.inactive.weak_bg_fill = col(Ground::Sunk.rgb());
    w.inactive.bg_stroke = egui::Stroke::new(1.0, col(theme::RULE));

    w.hovered.bg_fill = col(Ground::Raised.rgb());
    w.hovered.weak_bg_fill = col(Ground::Raised.rgb());
    w.hovered.bg_stroke = egui::Stroke::new(1.0, col(theme::selected()));
    w.hovered.fg_stroke.color = col(Role::Value.ink().unwrap_or(theme::DIM));

    w.active.bg_fill = col(Ground::Raised.rgb());
    w.active.weak_bg_fill = col(Ground::Raised.rgb());
    w.active.bg_stroke = egui::Stroke::new(1.0, col(theme::selected()));
    w.active.fg_stroke.color = col(Role::Value.ink().unwrap_or(theme::DIM));

    w.open.bg_fill = col(Ground::Raised.rgb());
    w.open.weak_bg_fill = col(Ground::Raised.rgb());
    w.open.bg_stroke = egui::Stroke::new(1.0, col(theme::RULE));
}

/// The frame a rail or the drawer is drawn in: the panel's fill, its border on the side that
/// faces the slide, and enough margin that the first section header is not against the glass.
///
/// egui's default panel frame has the window margin, which is eight points all round and looks
/// like a dialogue. Twelve across and nine down is the design's, and it is what makes a rail
/// read as a column of readings rather than as a form.
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(col(Ground::Panel.rgb()))
        .inner_margin(egui::Margin {
            left: 12,
            right: 12,
            top: 9,
            bottom: 8,
        })
}

/// The frame a sheet is drawn in: a filled, bordered, shadowed card over the slide.
///
/// **Explicit, because egui's default window frame draws no fill in this build.** `docs/UI.md`
/// §4 already recorded this once — the parameter editor started as a floating window and came
/// out as ghost text with cells swimming through it, over a lit microscope slide. A sheet is
/// the same shape of thing over the same slide and inherits the same problem, so it inherits
/// the same fix rather than rediscovering it.
pub fn sheet_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(col(Ground::Panel.rgb()))
        .stroke(egui::Stroke::new(1.0, col(theme::RULE)))
        .corner_radius(egui::CornerRadius::same(5))
        .inner_margin(egui::Margin::symmetric(14, 12))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 24,
            spread: 0,
            color: egui::Color32::from_black_alpha(180),
        })
}

/// A cell of exactly `width`, with its contents against the right of it.
///
/// For a reading that changes width while you watch. The status bar's right-hand run is laid
/// out right to left, so anything that grows or shrinks shoves everything to its left along
/// with it — and the frame rate changes every frame, which set the magnification, the level of
/// detail and the scale bar shuffling sideways continuously. A reading you cannot look at
/// because it will not hold still is not a reading.
///
/// An allocated width and not a padded string, which was the first attempt and does not work:
/// `format!("{:>19}", …)` pins the character count, but egui does not give the leading spaces
/// their width, so a two-digit frame rate still came out a character narrower than a
/// three-digit one and the whole run moved.
pub fn fixed_width<R>(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.allocate_ui_with_layout(
        egui::vec2(width, ui.available_height()),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            add(ui)
        },
    )
    .inner
}

/// A group's name, above the group.
///
/// The cheapest legibility in the design: a rail of undifferentiated rows becomes four named
/// groups for the price of four labels. `gap` is false for the first header in a panel, where
/// the space above it is the panel's own padding.
pub fn section(ui: &mut egui::Ui, title: &str, gap: bool) {
    if gap {
        ui.add_space(theme::SECTION_GAP);
    }
    ui.label(text(Role::Section, title));
    ui.add_space(2.0);
}

/// A rule inside a panel, between two groups. Fainter than a region border on purpose.
pub fn hairline(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 1.0),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, col(theme::HAIR));
}

/// The three-column row: `label │ bar │ value`.
///
/// ```text
/// energy ······ ▁▁▁▁▁▁▁▁▁▁▁▁ ····· 1.6
/// ```
///
/// `bar` is `None` where there is no meaningful maximum, and then nothing is drawn between the
/// label and the number — a bar against an invented full scale is a lie told in a straight
/// line, and energy and mass have no ceiling. Where there is one it carries its own colour,
/// which is **not** the value's: a chemical's bar is drawn in the chemical's colour, and those
/// come out of the scenario, where `carbon` is `#464650`. Painting the number in it too made
/// the reading unreadable, and the number is the part you came for.
pub fn row(ui: &mut egui::Ui, label: &str, bar: Option<(f32, Rgb)>, value: &str, ink: Rgb) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme::row::HEIGHT),
        egui::Sense::hover(),
    );
    let painter = ui.painter();

    // Clipped to its column, with an ellipsis. `carbon_dioxide` is fourteen characters and the
    // label column is not; unclipped it ran on under the bar beside it.
    let mut job = egui::text::LayoutJob::single_section(
        label.to_owned(),
        egui::TextFormat::simple(
            font(Role::Label),
            col(Role::Label.ink().unwrap_or(theme::DIM)),
        ),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: theme::row::LABEL,
        max_rows: 1,
        overflow_character: Some('…'),
        ..Default::default()
    };
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        egui::Color32::PLACEHOLDER,
    );

    painter.text(
        egui::pos2(rect.right(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        value,
        font(Role::Value),
        col(ink),
    );

    if let Some((fraction, colour)) = bar {
        let left = rect.left() + theme::row::LABEL + theme::row::GUTTER;
        let right = rect.right() - theme::row::VALUE - theme::row::GUTTER;
        if right <= left {
            return;
        }
        let trough = egui::Rect::from_min_size(
            egui::pos2(left, rect.center().y - theme::row::BAR / 2.0),
            egui::vec2(right - left, theme::row::BAR),
        );
        painter.rect_filled(trough, 2.0, col(theme::TROUGH));
        let mut filled = trough;
        filled.set_width(trough.width() * fraction.clamp(0.0, 1.0));
        painter.rect_filled(filled, 2.0, col(colour));
    }
}

/// A label and a number with nothing between them: the row grammar without the bar, for the
/// two-across grids of readings that have no scale to be shown against.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(text(Role::Label, label));
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| ui.label(text(Role::Value, value)),
        );
    });
}

/// A segmented-control button: a label, its key, and whether it is the one that is on.
///
/// The transport, the tool palette, the drawer tabs and the ecology views are all this widget,
/// which is why they finally look like each other.
pub fn chip(ui: &mut egui::Ui, label: &str, key: Option<&str>, on: bool) -> egui::Response {
    let mut button = egui::Button::new(text(Role::Label, label).color(if on {
        col(theme::on_selected())
    } else {
        col(Role::Body.ink().unwrap_or(theme::DIM))
    }))
    .corner_radius(egui::CornerRadius::same(3));
    if on {
        button = button.fill(col(theme::selected()));
    } else {
        button = button.fill(col(Ground::Sunk.rgb()));
    }
    if let Some(key) = key {
        button = button.shortcut_text(text(Role::Section, key).color(if on {
            col(theme::on_selected())
        } else {
            col(theme::DIM)
        }));
    }
    ui.add(button)
}

/// The line a plot takes when its reading is neither good news nor bad — a population, a count
/// of distinct genomes.
///
/// The label grey rather than a fourth [`Mood`], because inventing a mood for "no mood" is how a
/// palette acquires a colour that means nothing, and it must not be the accent: a plot line is
/// not a selection.
#[must_use]
pub fn plot_neutral() -> Rgb {
    Role::Label.ink().unwrap_or(theme::DIM)
}

/// A plot the width of the panel, with a baseline.
///
/// The baseline is the change. Without it a line at 6% of full scale and a line at 60% look the
/// same — a wobble in the middle of a grey box — and the rail reads as decoration. `values` are
/// already normalised to `0..=1` by the caller, which is where the maximum is known.
pub fn sparkline(ui: &mut egui::Ui, values: &[f32], colour: Rgb) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), 21.0),
        egui::Sense::hover(),
    );
    let painter = ui.painter();
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, col(theme::HAIR)),
    );
    if values.len() < 2 {
        return;
    }
    let step = rect.width() / (values.len() - 1) as f32;
    let points: Vec<egui::Pos2> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            egui::pos2(
                rect.left() + i as f32 * step,
                // Higher values draw higher up, which is the only way round anybody reads a
                // plot and the opposite of how screen y runs.
                rect.bottom() - v.clamp(0.0, 1.0) * (rect.height() - 1.0),
            )
        })
        .collect();
    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(1.2, col(colour)),
    ));
}

/// A row of chips, of which exactly one is on: the transport, the speed control, the tool
/// palette, the drawer's tabs.
///
/// Returns the option that was clicked, or `None`. Each entry is `(label, key, value)`, and the
/// key is what the menu would print next to it — so a segmented control teaches its own
/// shortcuts, which is most of why the keyboard is worth having at all.
pub fn segmented<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    options: &[(&str, &str, T)],
    current: T,
) -> Option<T> {
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (label, key, value) in options {
            let key = (!key.is_empty()).then_some(*key);
            if chip(ui, label, key, *value == current).clicked() {
                picked = Some(*value);
            }
        }
    });
    picked
}

/// A menu item: its label, and its key right-aligned in the shortcut column.
///
/// Every item has one, which is the point. The menu bar is where the fourteen single-key
/// bindings are discovered, and a binding that is not written next to the thing it does is a
/// binding nobody has.
pub fn menu_item(ui: &mut egui::Ui, label: &str, key: &str) -> egui::Response {
    ui.add(menu_button(label, key, false))
}

/// A menu item that is also a switch: the same row, plus whether the thing is on.
///
/// The `on` state is what makes the shortcut colour a decision rather than a default. `DIM` is
/// chosen to sit back from a near-black menu, and on the raised fill of a selected row it
/// disappears entirely — so every item that was *switched on* was the one whose key you could
/// not read, which is precisely backwards. Selected rows get the label's own ink.
pub fn menu_toggle(ui: &mut egui::Ui, label: &str, key: &str, on: bool) -> egui::Response {
    ui.add(menu_button(label, key, on).selected(on))
}

fn menu_button<'a>(label: &'a str, key: &'a str, on: bool) -> egui::Button<'a> {
    let button = egui::Button::new(text(Role::Body, label));
    if key.is_empty() {
        return button;
    }
    let ink = if on {
        Role::Label.ink().unwrap_or(theme::DIM)
    } else {
        theme::DIM
    };
    button.shortcut_text(text(Role::Label, key).color(col(ink)))
}

/// The name of a group of menu items, above them.
///
/// Indented to `button_padding.x`, because that is where the items below it start: a caption
/// flush against the popup's edge and a label seven points in read as two columns, and a menu
/// has one.
pub fn menu_caption(ui: &mut egui::Ui, caption: &str) {
    let pad = ui.spacing().button_padding.x as i8;
    ui.add_space(2.0);
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: pad,
            right: pad,
            ..egui::Margin::ZERO
        })
        .show(ui, |ui| ui.label(text(Role::Section, caption)));
}

/// Pin a menu to [`theme::MENU_WIDTH`]. First thing in every menu's closure.
///
/// A menu is a popup and a popup is as wide as its content, which for a menu is the wrong rule
/// twice over: a rule fills whatever it is offered and dragged `View` out to six hundred
/// points, and a menu with no rule in it shrank to its longest word and jammed `Tools`' shortcut
/// column against the labels. Neither width was chosen; both were a side effect of what happened
/// to be in the menu. Setting both bounds means the menu is the width it is meant to be
/// regardless of what is in it, and [`menu_rule`] can go on filling the width it is given
/// because the width it is given is now a decision.
pub fn menu(ui: &mut egui::Ui) {
    ui.set_min_width(theme::MENU_WIDTH);
    ui.set_max_width(theme::MENU_WIDTH);
}

/// A rule between two groups of menu items.
///
/// A hairline inset from both edges rather than egui's full-width separator, which at a
/// twenty-three pixel row height cuts a menu into slices instead of grouping it.
///
/// Fills the width it is given, so it must be given one — see [`menu`].
pub fn menu_rule(ui: &mut egui::Ui) {
    // Inset to where the labels start and where the shortcuts end, so the rule spans the column
    // rather than the popup. Taken from `button_padding` rather than written as a number,
    // because a menu item is a button and this is the same edge.
    let pad = ui.spacing().button_padding.x;
    ui.add_space(3.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + pad, rect.top()),
            egui::vec2((width - pad * 2.0).max(0.0), 1.0),
        ),
        0.0,
        col(theme::RULE),
    );
    ui.add_space(3.0);
}

/// A swatch in a chemical's own colour: filled when the layer is on, outlined when it is not.
///
/// The chemical colours are the slide's and not the interface's — they come out of the scenario
/// — so this is the one widget that takes a colour from outside the palette and does not
/// restyle it.
pub fn swatch(ui: &mut egui::Ui, rgb: Rgb, on: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    let rect = rect.shrink(0.5);
    if on {
        ui.painter().rect_filled(rect, 2.0, col(rgb));
    } else {
        ui.painter().rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, col(rgb)),
            egui::StrokeKind::Inside,
        );
    }
}

/// The fixed strip on the right of a drawer tab, holding whatever the work area cannot say
/// about itself (UI.md §8.6).
///
/// One shape for all six tabs, so that the genome's gene list, the web's species card and the
/// toolbox's prose all sit in the same place and the drawer stops being six layouts.
/// The drawer's shape, in one place: a wide work area, and the context column beside it.
///
/// `id` salts both scroll areas. The split is computed once here rather than in each tab
/// because it was wrong the first time it was written out by hand — the work area took
/// `available - CONTEXT_COLUMN`, and then the column asked for `CONTEXT_COLUMN.min(available *
/// 0.4)` of what was left, which is a fraction of a fraction and came out at 120 points with the
/// headings wrapping inside it. A measurement that has to be got right in six places is a
/// measurement that will be got wrong in one.
///
/// Below `MIN_WORK + CONTEXT_COLUMN` there is no room for both, and the column goes rather than
/// squeezing: a tab that has lost its work area is useless, and one that has lost its notes is
/// merely quieter.
pub fn drawer_split(
    ui: &mut egui::Ui,
    id: &str,
    work: impl FnOnce(&mut egui::Ui),
    context: impl FnOnce(&mut egui::Ui),
) {
    /// The narrowest a work area may be before the context column is dropped instead.
    const MIN_WORK: f32 = 380.0;

    let height = ui.available_height();
    let total = ui.available_width();
    // The gap `horizontal_top` puts *between* the two columns, which is part of the width and
    // was being spent twice: the two children were given `total` between them and then laid out
    // with a gap, so the content came out one `item_spacing` wider than the space it was handed.
    //
    // In the drawer that was invisible — a panel's width is the window's and it simply clipped.
    // In a window (M10.10) a window is as wide as its content, so the overflow made it wider,
    // which made `total` bigger, which made the overflow again: the build window crept about six
    // points wider every frame until it ran off the screen. The same shape of runaway as the
    // scenario tab's height, in the other axis, and the reason both are worth a note.
    let gap = ui.spacing().item_spacing.x;
    let column = if total >= MIN_WORK + theme::CONTEXT_COLUMN + gap {
        theme::CONTEXT_COLUMN
    } else {
        0.0
    };
    let work_width = if column > 0.0 {
        total - column - gap
    } else {
        total
    };
    // **Both columns at absolute offsets, neither able to move the other.**
    //
    // This was two `allocate_ui_with_layout` calls in a `horizontal_top`, which places the second
    // wherever the first *finished* — and `allocate_ui_with_layout` reports the rect its content
    // used, not the rect it was given. So a work area whose content would not fit in `work_width`
    // pushed the context column right by the overflow, off the end of the panel, where it was
    // clipped mid-word. Measured in the docked build rail: work wanted 430, used 482, and the
    // notes lost fifty-two points off their right-hand edge.
    //
    // The same failure `param_cell` documents in the parameter table and solves the same way:
    // laid out from the container's left edge rather than by following the previous child,
    // because absolute offsets cannot drift. The work area is clipped to its own rectangle, so
    // content too wide for it is cut off inside the column that owns it rather than painted over
    // the one next door.
    let outer = ui.available_rect_before_wrap();
    let work_rect =
        egui::Rect::from_min_size(outer.min, egui::vec2(work_width, height));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(work_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            // Both dimensions, not just the height. Without it a work area whose content is one
            // short label — the debugger before a sandbox is taken — shrank to the width of that
            // label and handed the rest of the drawer to the context column.
            ui.set_min_size(work_rect.size());
            ui.shrink_clip_rect(work_rect);
            work(ui);
        },
    );
    if column > 0.0 {
        let context_rect = egui::Rect::from_min_size(
            egui::pos2(outer.min.x + work_width + gap, outer.min.y),
            egui::vec2(column, height),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(context_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
            |ui| {
                ui.set_min_size(context_rect.size());
                ui.shrink_clip_rect(context_rect);
                ui.painter().vline(
                    context_rect.left(),
                    context_rect.y_range(),
                    egui::Stroke::new(1.0, col(theme::HAIR)),
                );
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false])
                    .show(ui, context);
            },
        );
    }
    // Claim the space both columns were laid out in, so whatever follows this call starts below
    // rather than on top of it. `scope_builder` paints where it is told and allocates nothing.
    ui.allocate_rect(
        egui::Rect::from_min_size(outer.min, egui::vec2(total, height)),
        egui::Sense::hover(),
    );
}

/// One segment of a [`segmented_bar`].
pub struct Segment<'a> {
    pub label: &'a str,
    /// Whether this segment is the one in force.
    pub on: bool,
    /// Whether being on means the accent rather than a raised ground.
    ///
    /// The distinction carries meaning in the transport and is the reason this is a field
    /// rather than a constant: the accent says *the world is running*, and the raised ground
    /// says *this is the speed it is running at*. Two different facts, both true at once, and a
    /// bar that painted them the same could not say so.
    pub accent: bool,
    pub hover: String,
    /// A fixed width, for the icon segments that would otherwise be as narrow as their glyph.
    pub width: Option<f32>,
}

/// A row of segments inside one bordered box, divided by hairlines.
///
/// The transport, drawn the way the design draws it: a single control with divisions rather
/// than a handful of separate chips with gaps between them. The difference is not decoration —
/// separate chips read as separate controls that happen to be adjacent, and these are seven
/// positions of one thing.
///
/// Returns the index of the segment that was clicked.
pub fn segmented_bar(ui: &mut egui::Ui, segments: &[Segment]) -> Option<usize> {
    const HEIGHT: f32 = 22.0;
    /// Space either side of a text segment's label.
    const PAD: f32 = 9.0;

    let font = font(Role::Label);
    let widths: Vec<f32> = segments
        .iter()
        .map(|seg| {
            seg.width.unwrap_or_else(|| {
                let galley = ui.fonts_mut(|f| {
                    f.layout_no_wrap(seg.label.to_owned(), font.clone(), egui::Color32::WHITE)
                });
                galley.size().x + PAD * 2.0
            })
        })
        .collect();
    // One hairline between each pair, so the box is exactly as wide as its contents.
    let total: f32 = widths.iter().sum::<f32>() + (segments.len().saturating_sub(1)) as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(total, HEIGHT), egui::Sense::hover());
    let painter = ui.painter();
    let radius = egui::CornerRadius::same(4);
    painter.rect_filled(rect, radius, col(Ground::Sunk.rgb()));

    let mut clicked = None;
    let mut x = rect.left();
    for (i, (seg, width)) in segments.iter().zip(&widths).enumerate() {
        let here = egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(*width, rect.height()));
        let response = ui.interact(here, ui.id().with(("seg", i)), egui::Sense::click());
        // The ends keep the box's rounding on their outer corners, so a filled first or last
        // segment does not square off the corner it is sitting in.
        let mut corners = egui::CornerRadius::ZERO;
        if i == 0 {
            corners.nw = radius.nw;
            corners.sw = radius.sw;
        }
        if i + 1 == segments.len() {
            corners.ne = radius.ne;
            corners.se = radius.se;
        }
        let fill = if seg.on && seg.accent {
            Some(col(theme::selected()))
        } else if seg.on {
            Some(col(Ground::Raised.rgb()))
        } else if response.hovered() {
            Some(col(Ground::Raised.rgb()).gamma_multiply(0.6))
        } else {
            None
        };
        if let Some(fill) = fill {
            painter.rect_filled(here, corners, fill);
        }
        let ink = if seg.on && seg.accent {
            theme::on_selected()
        } else if seg.on || response.hovered() {
            Role::Value.ink().unwrap_or(theme::DIM)
        } else {
            Role::Label.ink().unwrap_or(theme::DIM)
        };
        painter.text(
            here.center(),
            egui::Align2::CENTER_CENTER,
            seg.label,
            font.clone(),
            col(ink),
        );
        if i + 1 < segments.len() {
            let edge = here.right();
            painter.vline(
                edge,
                egui::Rangef::new(rect.top() + 1.0, rect.bottom() - 1.0),
                egui::Stroke::new(1.0, col(theme::RULE)),
            );
        }
        if !seg.hover.is_empty() {
            response.clone().on_hover_text(&seg.hover);
        }
        if response.clicked() {
            clicked = Some(i);
        }
        x += width + 1.0;
    }
    // The border last, over the fills, so a lit end segment sits inside the box rather than on
    // top of its edge.
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(1.0, col(theme::RULE)),
        egui::StrokeKind::Inside,
    );
    clicked
}
