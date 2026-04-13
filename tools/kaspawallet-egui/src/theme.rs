use eframe::egui::{self, Color32, FontFamily, FontId, Style, TextStyle, Visuals};

pub fn apply(ctx: &egui::Context) {
    let kaspa_aqua = Color32::from_rgb(73, 234, 203);
    let kaspa_mint = Color32::from_rgb(58, 221, 190);
    let kaspa_soft = Color32::from_rgb(112, 199, 186);
    let bg = Color32::from_rgb(6, 22, 24);
    let panel = Color32::from_rgb(10, 33, 36);
    let panel_alt = Color32::from_rgb(13, 44, 48);
    let stroke = Color32::from_rgb(31, 92, 90);
    let text = Color32::from_rgb(234, 255, 250);
    let muted = Color32::from_rgb(143, 191, 182);

    let mut style = Style {
        visuals: Visuals::dark(),
        ..Style::default()
    };

    style.spacing.item_spacing = egui::vec2(12.0, 12.0);
    style.spacing.window_margin = egui::Margin::same(18.0);
    style.spacing.button_padding = egui::vec2(12.0, 9.0);
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
    style.visuals.widgets.inactive.bg_fill = panel_alt;
    style.visuals.widgets.inactive.weak_bg_fill = panel_alt;
    style.visuals.widgets.inactive.bg_stroke.color = stroke;
    style.visuals.widgets.inactive.fg_stroke.color = text;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(16, 56, 60);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(16, 56, 60);
    style.visuals.widgets.hovered.bg_stroke.color = kaspa_aqua;
    style.visuals.widgets.hovered.fg_stroke.color = text;
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(20, 74, 79);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_rgb(20, 74, 79);
    style.visuals.widgets.active.bg_stroke.color = kaspa_mint;
    style.visuals.widgets.active.fg_stroke.color = text;
    style.visuals.widgets.open.bg_fill = panel_alt;
    style.visuals.widgets.open.weak_bg_fill = panel_alt;
    style.visuals.widgets.open.bg_stroke.color = kaspa_soft;
    style.visuals.widgets.open.fg_stroke.color = text;
    style.visuals.selection.bg_fill = kaspa_aqua;
    style.visuals.selection.stroke.color = bg;
    style.visuals.hyperlink_color = kaspa_aqua;
    style.visuals.faint_bg_color = panel;
    style.visuals.extreme_bg_color = panel_alt;
    style.visuals.code_bg_color = panel_alt;
    style.visuals.window_stroke.color = stroke;
    style.visuals.window_shadow.color = Color32::from_black_alpha(110);
    style.visuals.popup_shadow.color = Color32::from_black_alpha(96);
    style.visuals.warn_fg_color = Color32::from_rgb(255, 194, 107);
    style.visuals.error_fg_color = Color32::from_rgb(255, 122, 136);

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Hero".into()),
            FontId::new(28.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Section".into()),
            FontId::new(19.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(15.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(13.5, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(12.5, FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_style(style);
}
