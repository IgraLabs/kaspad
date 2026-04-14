use eframe::egui::{self, Color32, FontFamily, FontId, Style, TextStyle, Visuals};

pub fn apply(ctx: &egui::Context) {
    let safe_green = Color32::from_rgb(179, 246, 103);
    let signal_blue = Color32::from_rgb(132, 168, 255);
    let bg = Color32::from_rgb(8, 10, 14);
    let panel = Color32::from_rgb(16, 20, 27);
    let panel_alt = Color32::from_rgb(21, 26, 35);
    let stroke = Color32::from_rgb(42, 50, 63);
    let text = Color32::from_rgb(245, 247, 250);
    let muted = Color32::from_rgb(156, 165, 177);

    let mut style = Style {
        visuals: Visuals::dark(),
        ..Style::default()
    };

    style.spacing.item_spacing = egui::vec2(10.0, 12.0);
    style.spacing.window_margin = egui::Margin::same(16.0);
    style.spacing.button_padding = egui::vec2(12.0, 8.0);
    style.spacing.menu_margin = egui::Margin::same(10.0);
    style.spacing.text_edit_width = 320.0;
    style.spacing.interact_size = egui::vec2(40.0, 28.0);
    style.visuals.window_fill = bg;
    style.visuals.panel_fill = bg;
    style.visuals.override_text_color = Some(text);
    style.visuals.widgets.noninteractive.bg_fill = panel;
    style.visuals.widgets.noninteractive.weak_bg_fill = panel;
    style.visuals.widgets.noninteractive.bg_stroke.color = stroke;
    style.visuals.widgets.noninteractive.fg_stroke.color = muted;
    style.visuals.widgets.inactive.bg_fill = panel;
    style.visuals.widgets.inactive.weak_bg_fill = panel;
    style.visuals.widgets.inactive.bg_stroke.color = stroke;
    style.visuals.widgets.inactive.fg_stroke.color = text;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(21, 26, 34);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(21, 26, 34);
    style.visuals.widgets.hovered.bg_stroke.color = safe_green;
    style.visuals.widgets.hovered.fg_stroke.color = text;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(24, 30, 38);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(24, 30, 38);
    style.visuals.widgets.active.bg_stroke.color = safe_green;
    style.visuals.widgets.active.fg_stroke.color = text;
    style.visuals.widgets.open.bg_fill = panel_alt;
    style.visuals.widgets.open.weak_bg_fill = panel_alt;
    style.visuals.widgets.open.bg_stroke.color = signal_blue;
    style.visuals.widgets.open.fg_stroke.color = text;
    style.visuals.selection.bg_fill = safe_green;
    style.visuals.selection.stroke.color = bg;
    style.visuals.hyperlink_color = safe_green;
    style.visuals.faint_bg_color = panel;
    style.visuals.extreme_bg_color = panel_alt;
    style.visuals.code_bg_color = panel_alt;
    style.visuals.window_stroke.color = stroke;
    style.visuals.window_shadow.color = Color32::from_black_alpha(140);
    style.visuals.popup_shadow.color = Color32::from_black_alpha(120);
    style.visuals.warn_fg_color = Color32::from_rgb(255, 196, 92);
    style.visuals.error_fg_color = Color32::from_rgb(255, 122, 122);

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Hero".into()),
            FontId::new(27.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Section".into()),
            FontId::new(17.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.5, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_style(style);
}
