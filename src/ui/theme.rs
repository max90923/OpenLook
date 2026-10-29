//! Colours of the classic Office "Bunt" (colourful) and "Schwarz" designs.

use eframe::egui::{self, Color32};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Design {
    Bunt,
    Schwarz,
}

impl Design {
    pub fn parse(s: &str) -> Self {
        if s == "schwarz" {
            Design::Schwarz
        } else {
            Design::Bunt
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Design::Bunt => "bunt",
            Design::Schwarz => "schwarz",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    /// Title bar, ribbon tab band and status bar.
    pub band: Color32,
    pub band_hover: Color32,
    pub band_text: Color32,
    /// Ribbon body and selected ribbon tab.
    pub ribbon: Color32,
    pub ribbon_border: Color32,
    pub group_label: Color32,
    pub button_hover: Color32,
    pub button_down: Color32,
    /// Panes (folder pane, list, reading pane).
    pub pane: Color32,
    pub pane_alt: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_weak: Color32,
    pub accent: Color32,
    pub selected: Color32,
    pub selected_focus: Color32,
    pub hover: Color32,
    pub danger: Color32,
    pub flag: Color32,
    pub today: Color32,
}

pub const OUTLOOK_BLUE: Color32 = Color32::from_rgb(0, 114, 198);

impl Theme {
    pub fn new(d: Design) -> Self {
        match d {
            Design::Bunt => Theme {
                dark: false,
                band: OUTLOOK_BLUE,
                band_hover: Color32::from_rgb(25, 133, 211),
                band_text: Color32::WHITE,
                ribbon: Color32::from_rgb(243, 243, 243),
                ribbon_border: Color32::from_rgb(212, 212, 212),
                group_label: Color32::from_rgb(102, 102, 102),
                button_hover: Color32::from_rgb(197, 197, 197),
                button_down: Color32::from_rgb(179, 179, 179),
                pane: Color32::WHITE,
                pane_alt: Color32::from_rgb(248, 248, 248),
                border: Color32::from_rgb(212, 212, 212),
                text: Color32::from_rgb(38, 38, 38),
                text_weak: Color32::from_rgb(102, 102, 102),
                accent: OUTLOOK_BLUE,
                selected: Color32::from_rgb(205, 230, 247),
                selected_focus: Color32::from_rgb(177, 214, 240),
                hover: Color32::from_rgb(229, 241, 251),
                danger: Color32::from_rgb(197, 15, 31),
                flag: Color32::from_rgb(214, 54, 54),
                today: Color32::from_rgb(0, 114, 198),
            },
            Design::Schwarz => Theme {
                dark: true,
                band: Color32::from_rgb(38, 38, 38),
                band_hover: Color32::from_rgb(62, 62, 62),
                band_text: Color32::from_rgb(240, 240, 240),
                ribbon: Color32::from_rgb(54, 54, 54),
                ribbon_border: Color32::from_rgb(80, 80, 80),
                group_label: Color32::from_rgb(180, 180, 180),
                button_hover: Color32::from_rgb(80, 80, 80),
                button_down: Color32::from_rgb(100, 100, 100),
                pane: Color32::from_rgb(30, 30, 30),
                pane_alt: Color32::from_rgb(40, 40, 40),
                border: Color32::from_rgb(70, 70, 70),
                text: Color32::from_rgb(235, 235, 235),
                text_weak: Color32::from_rgb(170, 170, 170),
                accent: Color32::from_rgb(80, 165, 230),
                selected: Color32::from_rgb(40, 74, 104),
                selected_focus: Color32::from_rgb(0, 90, 158),
                hover: Color32::from_rgb(52, 52, 52),
                danger: Color32::from_rgb(241, 112, 123),
                flag: Color32::from_rgb(241, 112, 123),
                today: Color32::from_rgb(80, 165, 230),
            },
        }
    }

    pub fn visuals(&self) -> egui::Visuals {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = self.pane;
        v.window_fill = self.pane;
        v.extreme_bg_color = self.pane;
        v.faint_bg_color = self.pane_alt;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.selected_focus;
        v.selection.stroke.color = self.text;
        v.hyperlink_color = self.accent;
        v.window_corner_radius = egui::CornerRadius::ZERO;
        v.menu_corner_radius = egui::CornerRadius::ZERO;
        v.window_stroke = egui::Stroke::new(1.0_f32, self.border);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = egui::CornerRadius::ZERO;
        }
        v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, self.border);
        v.widgets.inactive.bg_fill = self.pane;
        v.widgets.inactive.weak_bg_fill = self.ribbon;
        v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, self.border);
        v.widgets.hovered.weak_bg_fill = self.hover;
        v.widgets.hovered.bg_fill = self.hover;
        v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, self.accent);
        v.widgets.active.weak_bg_fill = self.selected;
        v.widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, self.accent);
        v.widgets.open.weak_bg_fill = self.selected;
        v
    }
}
