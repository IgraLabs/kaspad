use eframe::egui::{self, Color32, FontFamily, FontId, Style, TextStyle, Visuals};

pub fn apply(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
        family.insert(0, "Ubuntu-Light".to_owned());
    }
    if let Some(family) = fonts.families.get_mut(&FontFamily::Monospace) {
        family.insert(0, "Hack".to_owned());
    }
    ctx.set_fonts(fonts);

    let mut style = Style {
        visuals: Visuals::light(),
        ..Style::default()
    };

    style.spacing.item_spacing = egui::vec2(12.0, 10.0);
    style.spacing.window_margin = egui::Margin::same(18.0);
    style.spacing.button_padding = egui::vec2(14.0, 10.0);
    style.spacing.menu_margin = egui::Margin::same(10.0);
    style.visuals.window_fill = Color32::from_rgb(245, 240, 232);
    style.visuals.panel_fill = Color32::from_rgb(245, 240, 232);
    style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(248, 244, 238);
    style.visuals.widgets.noninteractive.bg_stroke.color = Color32::from_rgb(210, 196, 183);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(255, 252, 248);
    style.visuals.widgets.inactive.bg_stroke.color = Color32::from_rgb(194, 169, 146);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(255, 247, 236);
    style.visuals.widgets.hovered.bg_stroke.color = Color32::from_rgb(184, 112, 52);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(235, 224, 212);
    style.visuals.widgets.active.bg_stroke.color = Color32::from_rgb(130, 83, 44);
    style.visuals.selection.bg_fill = Color32::from_rgb(210, 122, 58);
    style.visuals.selection.stroke.color = Color32::from_rgb(255, 251, 246);
    style.visuals.hyperlink_color = Color32::from_rgb(0, 110, 128);
    style.visuals.faint_bg_color = Color32::from_rgb(237, 230, 220);
    style.visuals.extreme_bg_color = Color32::from_rgb(228, 219, 207);
    style.visuals.code_bg_color = Color32::from_rgb(252, 248, 244);
    style.visuals.window_stroke.color = Color32::from_rgb(207, 191, 175);

    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(26.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Hero".into()),
            FontId::new(34.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Name("Section".into()),
            FontId::new(22.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(15.5, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(14.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Small,
            FontId::new(12.5, FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_style(style);
}
