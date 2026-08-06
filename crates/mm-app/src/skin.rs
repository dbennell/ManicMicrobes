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

    let s = &mut style.spacing;
    // Tighter than egui's 8×3. The rails are lists of readings and the readings want to be
    // near each other; the air belongs between *groups*, which is what `SECTION_GAP` is for.
    s.item_spacing = egui::vec2(6.0, 3.0);
    s.button_padding = egui::vec2(7.0, 2.0);
    s.interact_size.y = 18.0;
    s.menu_margin = egui::Margin::symmetric(0, 4);
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
    let mut button = egui::Button::new(text(Role::Body, label));
    if !key.is_empty() {
        button = button.shortcut_text(text(Role::Label, key).color(col(theme::DIM)));
    }
    ui.add(button)
}

/// The name of a group of menu items, above them.
pub fn menu_caption(ui: &mut egui::Ui, caption: &str) {
    ui.add_space(2.0);
    ui.label(text(Role::Section, caption));
}

/// A rule between two groups of menu items.
///
/// A hairline inset from both edges rather than egui's full-width separator, which at a
/// twenty-three pixel row height cuts a menu into slices instead of grouping it.
pub fn menu_rule(ui: &mut egui::Ui) {
    ui.add_space(3.0);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, rect.top()),
            egui::vec2((width - 16.0).max(0.0), 1.0),
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
    let column = if total >= MIN_WORK + theme::CONTEXT_COLUMN {
        theme::CONTEXT_COLUMN
    } else {
        0.0
    };
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(total - column, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                // Both dimensions, not just the height. `allocate_ui_with_layout` reports the
                // rect its content *used*, so a work area whose content is one short label —
                // the debugger before a sandbox is taken — shrank to the width of that label
                // and handed the rest of the drawer to the context column.
                ui.set_min_size(egui::vec2(total - column, height));
                work(ui);
            },
        );
        if column <= 0.0 {
            return;
        }
        ui.allocate_ui_with_layout(
            egui::vec2(column, height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_size(egui::vec2(column, height));
                let rect = ui.max_rect();
                ui.painter().vline(
                    rect.left(),
                    rect.y_range(),
                    egui::Stroke::new(1.0, col(theme::HAIR)),
                );
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .id_salt(id)
                    .auto_shrink([false, false])
                    .show(ui, context);
            },
        );
    });
}
