//! Office window chrome: custom title bar with quick access toolbar,
//! ribbon (tabs, groups, large and small buttons), native child windows.

use eframe::egui::{self, Color32, CursorIcon, ResizeDirection, RichText, Sense, ViewportCommand};
use egui_phosphor::regular as ph;

use super::theme::Theme;

pub const IC_BLUE: Color32 = Color32::from_rgb(0, 114, 198);
pub const IC_RED: Color32 = Color32::from_rgb(209, 52, 56);
pub const IC_YELLOW: Color32 = Color32::from_rgb(224, 160, 40);
pub const IC_GREEN: Color32 = Color32::from_rgb(16, 124, 16);
pub const IC_PURPLE: Color32 = Color32::from_rgb(135, 100, 184);
pub const IC_GREY: Color32 = Color32::from_rgb(110, 110, 110);

pub const TITLE_H: f32 = 30.0;
pub const TABS_H: f32 = 28.0;
pub const RIBBON_H: f32 = 94.0;

/// Fonts: Segoe UI from the system when available (Windows), the Phosphor
/// icon font as fallback for icons, and its filled variant as family "fill".
pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Regular + bold of the same family: Segoe UI (Windows) or a common
    // system sans on Linux/macOS. Falls back to egui's built-in font.
    let pairs = [
        (
            "C:\\Windows\\Fonts\\segoeui.ttf",
            "C:\\Windows\\Fonts\\segoeuib.ttf",
        ),
        (
            "/usr/share/fonts/truetype/segoe-ui/segoeui.ttf",
            "/usr/share/fonts/truetype/segoe-ui/segoeuib.ttf",
        ),
        (
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
        ),
        (
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
        ),
        (
            "/usr/share/fonts/TTF/DejaVuSans.ttf",
            "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
        ),
        (
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
        ),
    ];
    let mut bold = None;
    for (regular, bold_path) in pairs {
        if let (Ok(r), Ok(b)) = (std::fs::read(regular), std::fs::read(bold_path)) {
            fonts
                .font_data
                .insert("ui".into(), egui::FontData::from_owned(r).into());
            fonts
                .font_data
                .insert("ui-bold".into(), egui::FontData::from_owned(b).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "ui".into());
            bold = Some("ui-bold".to_string());
            break;
        }
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    fonts.font_data.insert(
        "phosphor-fill".into(),
        egui_phosphor::Variant::Fill.font_data().into(),
    );
    let proportional = fonts.families[&egui::FontFamily::Proportional].clone();
    let mut fill = vec!["phosphor-fill".to_string()];
    fill.extend(proportional.iter().cloned());
    fonts
        .families
        .insert(egui::FontFamily::Name("fill".into()), fill);
    // "bold" always exists; without a bold system font it is the regular one.
    let mut bold_family: Vec<String> = bold.into_iter().collect();
    bold_family.extend(proportional);
    fonts
        .families
        .insert(egui::FontFamily::Name("bold".into()), bold_family);
    ctx.set_fonts(fonts);
}

pub fn bold_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("bold".into()))
}

pub fn fill_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("fill".into()))
}

/// Office-style title bar. `qat` draws the quick access toolbar at the left.
/// Returns true when the close button was clicked.
pub fn title_bar(
    ui: &mut egui::Ui,
    t: &Theme,
    title: &str,
    qat: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut close = false;
    let rect = ui.available_rect_before_wrap();
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), TITLE_H));
    ui.painter().rect_filled(bar, 0, t.band);
    let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));

    // Drag / double click on the empty area moves / maximizes the window.
    let drag = ui.interact(bar, ui.id().with("titlebar"), Sense::click_and_drag());
    if drag.double_clicked() {
        ui.ctx()
            .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    } else if drag.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    ui.painter().text(
        bar.center(),
        egui::Align2::CENTER_CENTER,
        title,
        egui::FontId::proportional(12.5),
        t.band_text,
    );

    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.add_space(6.0);
    child.style_mut().visuals.override_text_color = Some(t.band_text);
    qat(&mut child);

    let mut right = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(bar)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right.spacing_mut().item_spacing.x = 0.0;
    for (i, icon) in [
        ph::X,
        if maximized {
            ph::CORNERS_IN
        } else {
            ph::SQUARE
        },
        ph::MINUS,
    ]
    .into_iter()
    .enumerate()
    {
        let (r, resp) = right.allocate_exact_size(egui::vec2(46.0, TITLE_H), Sense::click());
        if resp.hovered() {
            right.painter().rect_filled(
                r,
                0,
                if i == 0 {
                    Color32::from_rgb(232, 17, 35)
                } else {
                    t.band_hover
                },
            );
        }
        right.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(14.0),
            t.band_text,
        );
        if resp.clicked() {
            match i {
                0 => close = true,
                1 => right
                    .ctx()
                    .send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
                _ => right
                    .ctx()
                    .send_viewport_cmd(ViewportCommand::Minimized(true)),
            }
        }
    }
    ui.advance_cursor_after_rect(bar);
    close
}

/// Draw a ribbon icon: the Fluent colour icon when one is mapped to the
/// glyph, otherwise the glyph itself in `color`.
pub fn paint_icon(
    ui: &egui::Ui,
    icon: &str,
    color: Color32,
    center: egui::Pos2,
    size: f32,
    alpha: f32,
) {
    match super::icons::svg(icon, size > 24.0) {
        Some((uri, bytes)) => {
            let rect = egui::Rect::from_center_size(center, egui::vec2(size, size));
            egui::Image::from_bytes(uri, bytes)
                .tint(Color32::WHITE.gamma_multiply(alpha))
                .paint_at(ui, rect);
        }
        None => {
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                icon,
                fill_font(size * 0.95),
                color.gamma_multiply(alpha),
            );
        }
    }
}

/// Small icon button for the quick access toolbar.
pub fn qat_button(ui: &mut egui::Ui, t: &Theme, icon: &str, tip: &str) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(26.0, TITLE_H - 4.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, 0, t.band_hover);
    }
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        egui::FontId::proportional(15.0),
        t.band_text,
    );
    resp.on_hover_text(tip).clicked()
}

/// Lets borderless windows be resized at their edges.
pub fn resize_edges(ctx: &egui::Context) {
    if ctx.input(|i| i.viewport().maximized.unwrap_or(false)) {
        return;
    }
    let Some(pos) = ctx.pointer_hover_pos() else {
        return;
    };
    let r = ctx.content_rect();
    let m = 5.0;
    let (l, rt, tp, b) = (
        pos.x - r.left() < m,
        r.right() - pos.x < m,
        pos.y - r.top() < m,
        r.bottom() - pos.y < m,
    );
    let dir = match (l, rt, tp, b) {
        (true, _, true, _) => Some((ResizeDirection::NorthWest, CursorIcon::ResizeNorthWest)),
        (_, true, true, _) => Some((ResizeDirection::NorthEast, CursorIcon::ResizeNorthEast)),
        (true, _, _, true) => Some((ResizeDirection::SouthWest, CursorIcon::ResizeSouthWest)),
        (_, true, _, true) => Some((ResizeDirection::SouthEast, CursorIcon::ResizeSouthEast)),
        (true, ..) => Some((ResizeDirection::West, CursorIcon::ResizeWest)),
        (_, true, ..) => Some((ResizeDirection::East, CursorIcon::ResizeEast)),
        (_, _, true, _) => Some((ResizeDirection::North, CursorIcon::ResizeNorth)),
        (.., true) => Some((ResizeDirection::South, CursorIcon::ResizeSouth)),
        _ => None,
    };
    if let Some((d, cursor)) = dir {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(d));
        }
    }
}

/// Ribbon tab band. Returns true when "Datei" was clicked.
pub fn ribbon_tabs(
    ui: &mut egui::Ui,
    t: &Theme,
    tabs: &[&str],
    selected: &mut usize,
    right: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut file = false;
    let rect = ui.available_rect_before_wrap();
    let band = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), TABS_H));
    ui.painter().rect_filled(band, 0, t.band);
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(band)
            .layout(egui::Layout::left_to_right(egui::Align::Max)),
    );
    row.spacing_mut().item_spacing.x = 0.0;
    row.add_space(4.0);
    let font = egui::FontId::proportional(13.0);
    let labels = std::iter::once("Datei").chain(tabs.iter().copied());
    for (i, label) in labels.enumerate() {
        let g = row
            .painter()
            .layout_no_wrap(label.to_string(), font.clone(), t.band_text);
        let size = egui::vec2(g.size().x + 26.0, TABS_H - 3.0);
        let (r, resp) = row.allocate_exact_size(size, Sense::click());
        let is_sel = i > 0 && *selected == i - 1;
        let color = if is_sel {
            row.painter().rect_filled(r, 0, t.ribbon);
            t.accent
        } else {
            if resp.hovered() {
                row.painter().rect_filled(r, 0, t.band_hover);
            }
            t.band_text
        };
        row.painter().text(
            r.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font.clone(),
            if is_sel && t.dark { t.text } else { color },
        );
        if resp.clicked() {
            if i == 0 {
                file = true;
            } else {
                *selected = i - 1;
            }
        }
    }
    row.add_space(14.0);
    row.scope(|ui| {
        ui.style_mut().visuals.override_text_color = Some(t.band_text);
        right(ui)
    });
    ui.advance_cursor_after_rect(band);
    file
}

/// Ribbon body; `add` lays out groups left to right.
pub fn ribbon_body(ui: &mut egui::Ui, t: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.available_rect_before_wrap();
    let body = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), RIBBON_H));
    ui.painter().rect_filled(body, 0, t.ribbon);
    ui.painter().line_segment(
        [body.left_bottom(), body.right_bottom()],
        egui::Stroke::new(1.0_f32, t.ribbon_border),
    );
    let mut row = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body.shrink2(egui::vec2(4.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    row.spacing_mut().item_spacing = egui::vec2(2.0, 0.0);
    row.style_mut().visuals.override_text_color = Some(t.text);
    add(&mut row);
    ui.advance_cursor_after_rect(body);
}

/// One ribbon group with its label at the bottom and a separator at the right.
pub fn group(ui: &mut egui::Ui, t: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    let top = ui.cursor().min;
    let h = RIBBON_H - 6.0;
    let inner = ui
        .allocate_ui_with_layout(
            egui::vec2(2000.0, h - 18.0),
            egui::Layout::left_to_right(egui::Align::Min),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(1.0, 0.0);
                ui.add_space(2.0);
                add(ui);
                ui.add_space(2.0);
            },
        )
        .response
        .rect;
    let g = ui.painter().layout_no_wrap(
        label.to_string(),
        egui::FontId::proportional(11.0),
        t.group_label,
    );
    let w = inner.width().max(g.size().x + 12.0);
    let label_pos = egui::pos2(top.x + w / 2.0 - g.size().x / 2.0, top.y + h - 15.0);
    ui.painter().galley(label_pos, g, t.group_label);
    let x = top.x + w + 2.0;
    ui.painter().line_segment(
        [egui::pos2(x, top.y + 2.0), egui::pos2(x, top.y + h - 2.0)],
        egui::Stroke::new(1.0_f32, t.ribbon_border),
    );
    let used = egui::Rect::from_min_max(top, egui::pos2(x + 3.0, top.y + h));
    ui.advance_cursor_after_rect(used);
}

/// Large ribbon button: 32px icon above a (possibly two-line) label.
pub fn large(
    ui: &mut egui::Ui,
    t: &Theme,
    icon: &str,
    color: Color32,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let label = label.replace('▾', ph::CARET_DOWN);
    let lines: Vec<&str> = label.splitn(2, '\n').collect();
    let w = lines
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap(l.to_string(), font.clone(), t.text)
                .size()
                .x
        })
        .fold(32.0_f32, f32::max)
        + 12.0;
    let (r, resp) = ui.allocate_exact_size(
        egui::vec2(w, RIBBON_H - 24.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    paint_button_bg(ui, t, r, &resp, enabled);
    let alpha = if enabled { 1.0 } else { 0.35 };
    paint_icon(
        ui,
        icon,
        color,
        egui::pos2(r.center().x, r.top() + 20.0),
        32.0,
        alpha,
    );
    for (i, l) in lines.iter().enumerate() {
        ui.painter().text(
            egui::pos2(r.center().x, r.top() + 44.0 + i as f32 * 14.0),
            egui::Align2::CENTER_CENTER,
            *l,
            font.clone(),
            t.text.gamma_multiply(alpha),
        );
    }
    resp
}

/// Small ribbon button: 16px icon + label in one row.
pub fn small(
    ui: &mut egui::Ui,
    t: &Theme,
    icon: &str,
    color: Color32,
    label: &str,
    enabled: bool,
) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let g = ui
        .painter()
        .layout_no_wrap(label.replace('▾', ph::CARET_DOWN), font.clone(), t.text);
    let (r, resp) = ui.allocate_exact_size(
        egui::vec2(g.size().x + 30.0, 22.0),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    paint_button_bg(ui, t, r, &resp, enabled);
    let alpha = if enabled { 1.0 } else { 0.35 };
    paint_icon(
        ui,
        icon,
        color,
        r.left_center() + egui::vec2(12.0, 0.0),
        16.0,
        alpha,
    );
    ui.painter().galley(
        r.left_center() + egui::vec2(24.0, -g.size().y / 2.0),
        g,
        t.text.gamma_multiply(alpha),
    );
    resp
}

/// Column of up to three small buttons.
pub fn column(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    ui.allocate_ui_with_layout(
        egui::vec2(400.0, RIBBON_H - 24.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            add(ui)
        },
    );
}

fn paint_button_bg(ui: &egui::Ui, t: &Theme, r: egui::Rect, resp: &egui::Response, enabled: bool) {
    if !enabled {
        return;
    }
    if resp.is_pointer_button_down_on() {
        ui.painter().rect_filled(r, 0, t.button_down);
    } else if resp.hovered()
        || egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(resp))
    {
        ui.painter().rect_filled(r, 0, t.button_hover);
    }
}

/// Drop-down menu attached to a ribbon button.
pub fn menu(resp: &egui::Response, add: impl FnOnce(&mut egui::Ui)) {
    egui::Popup::menu(resp).show(|ui| {
        ui.set_min_width(180.0);
        add(ui)
    });
}

/// Menu entry with an icon.
pub fn menu_item(ui: &mut egui::Ui, icon: &str, color: Color32, label: &str) -> bool {
    let button = match super::icons::svg(icon, false) {
        Some((uri, bytes)) => egui::Button::new((
            egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(16.0, 16.0)),
            label,
        )),
        None => egui::Button::new((RichText::new(icon).color(color).size(15.0), label)),
    };
    let clicked = ui.add(button.frame(false)).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Flat button with a 16 px Fluent icon and an optional label.
pub fn icon_button(ui: &mut egui::Ui, icon: &str, color: Color32, label: &str) -> egui::Response {
    let button = match super::icons::svg(icon, false) {
        Some((uri, bytes)) => egui::Button::new((
            egui::Image::from_bytes(uri, bytes).fit_to_exact_size(egui::vec2(16.0, 16.0)),
            label,
        )),
        None => egui::Button::new((RichText::new(icon).color(color).size(15.0), label)),
    };
    ui.add(button.frame(false))
}

/// A native top-level window (own OS window) with Office chrome.
/// Returns true when the user closed it.
pub fn native_window(
    ctx: &egui::Context,
    id: egui::ViewportId,
    title: &str,
    size: [f32; 2],
    t: &Theme,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let mut add = Some(add);
    ctx.show_viewport_immediate(
        id,
        egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size(size)
            .with_min_inner_size([480.0, 320.0])
            .with_decorations(false),
        |ctx, class| {
            let mut closed = false;
            if class == egui::ViewportClass::Embedded {
                let mut open = true;
                egui::Window::new(title)
                    .id(egui::Id::new(id))
                    .open(&mut open)
                    .default_size(size)
                    .show(ctx, |ui| {
                        if let Some(add) = add.take() {
                            add(ui)
                        }
                    });
                return !open;
            }
            resize_edges(ctx);
            ctx.send_viewport_cmd(ViewportCommand::Title(title.to_string()));
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(t.pane))
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    closed |= title_bar(ui, t, title, |_| {});
                    if let Some(add) = add.take() {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        add(ui);
                    }
                    ui.painter().rect_stroke(
                        ctx.content_rect(),
                        0,
                        egui::Stroke::new(1.0_f32, t.band),
                        egui::StrokeKind::Inside,
                    );
                });
            closed || ctx.input(|i| i.viewport().close_requested())
        },
    )
}

/// Single-tab ribbon used by item windows (message, appointment, contact, task).
pub fn item_ribbon(ui: &mut egui::Ui, t: &Theme, tab: &str, add: impl FnOnce(&mut egui::Ui)) {
    let mut sel = 0;
    ribbon_tabs(ui, t, &[tab], &mut sel, |_| {});
    ribbon_body(ui, t, add);
}

/// Ribbon with several tabs for item windows; `add` gets the selected tab.
/// Returns true when "Datei" was clicked.
pub fn item_ribbon_tabs(
    ui: &mut egui::Ui,
    t: &Theme,
    tabs: &[&str],
    selected: &mut usize,
    add: impl FnOnce(&mut egui::Ui, usize),
) -> bool {
    let file = ribbon_tabs(ui, t, tabs, selected, |_| {});
    let tab = *selected;
    ribbon_body(ui, t, |ui| add(ui, tab));
    file
}
