use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    ForensicDark,
    ForensicLight,
}

pub struct Palette {
    pub bg: egui::Color32,
    pub panel: egui::Color32,
    pub surface: egui::Color32,
    pub text: egui::Color32,
    pub weak: egui::Color32,
    pub accent: egui::Color32,
    pub success: egui::Color32,
    pub warn: egui::Color32,
    pub error: egui::Color32,
    pub info: egui::Color32,
}

impl Theme {
    pub fn palette(self) -> Palette {
        match self {
            Theme::ForensicDark => Palette {
                bg: egui::Color32::from_rgb(10, 10, 12), // Deep near-black workspace
                panel: egui::Color32::from_rgb(16, 16, 18), // Slightly elevated panels
                surface: egui::Color32::from_rgb(22, 22, 24), // Interactive surfaces
                text: egui::Color32::from_rgb(240, 240, 245), // Crisp off-white text
                weak: egui::Color32::from_rgb(130, 135, 150), // Muted technical text
                accent: egui::Color32::from_rgb(0, 191, 255), // Vibrant cyan/blue accent
                success: egui::Color32::from_rgb(34, 197, 94), // Clinical green
                warn: egui::Color32::from_rgb(245, 158, 11), // Alert amber
                error: egui::Color32::from_rgb(239, 68, 68), // Critical red
                info: egui::Color32::from_rgb(0, 191, 255), // Info cyan aligned with accent
            },
            Theme::ForensicLight => Palette {
                bg: egui::Color32::from_rgb(248, 250, 252),
                panel: egui::Color32::from_rgb(255, 255, 255),
                surface: egui::Color32::from_rgb(241, 245, 249),
                text: egui::Color32::from_rgb(15, 23, 42),
                weak: egui::Color32::from_rgb(100, 116, 139),
                accent: egui::Color32::from_rgb(37, 99, 235),
                success: egui::Color32::from_rgb(22, 163, 74),
                warn: egui::Color32::from_rgb(217, 119, 6),
                error: egui::Color32::from_rgb(220, 38, 38),
                info: egui::Color32::from_rgb(2, 132, 199),
            },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::ForensicDark => "Forensic Terminal (Dark)",
            Theme::ForensicLight => "Laboratory (Light)",
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        let p = self.palette();

        let mut visuals = if self == Theme::ForensicDark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        // Premium Enterprise Aesthetic overrides
        visuals.window_rounding = egui::Rounding::same(6.0); // Sharper, more professional corners
        visuals.menu_rounding = egui::Rounding::same(4.0);
        visuals.window_shadow.color = egui::Color32::from_black_alpha(200);
        visuals.window_shadow.spread = 8.0;
        visuals.window_shadow.blur = 48.0; // Deep ambient shadow for glassmorphism illusion
        visuals.popup_shadow.color = egui::Color32::from_black_alpha(180);
        visuals.popup_shadow.spread = 4.0;
        visuals.popup_shadow.blur = 24.0;

        visuals.panel_fill = p.panel;
        visuals.window_fill = p.panel;
        visuals.extreme_bg_color = p.bg;
        visuals.faint_bg_color = p.surface;

        // Non-interactive surfaces
        visuals.widgets.noninteractive.bg_fill = p.surface;
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, p.surface);
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(4.0);

        // Interactive states
        visuals.widgets.inactive.bg_fill = p.surface;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.surface);
        visuals.widgets.inactive.rounding = egui::Rounding::same(4.0);

        visuals.widgets.hovered.bg_fill = p.accent.linear_multiply(0.15);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.accent.linear_multiply(0.5));
        visuals.widgets.hovered.rounding = egui::Rounding::same(4.0);

        visuals.widgets.active.bg_fill = p.accent.linear_multiply(0.3);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, p.accent);
        visuals.widgets.active.rounding = egui::Rounding::same(4.0);

        visuals.hyperlink_color = p.accent;
        visuals.selection.bg_fill = p.accent.linear_multiply(0.2);
        visuals.selection.stroke.color = p.accent;
        visuals.warn_fg_color = p.warn;
        visuals.error_fg_color = p.error;

        ctx.set_visuals(visuals);

        // Global layout & typography spacing
        let mut style = (*ctx.style()).clone();
        style.spacing.item_spacing = egui::vec2(16.0, 16.0); // More breathing room
        style.spacing.button_padding = egui::vec2(18.0, 12.0);
        style.spacing.window_margin = egui::Margin::same(24.0);
        style.spacing.menu_margin = egui::Margin::same(12.0);
        style.spacing.icon_width = 16.0;

        // Modern typography sizing
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(22.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(12.0, egui::FontFamily::Proportional),
        );
        ctx.set_style(style);
    }
}
