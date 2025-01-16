use eframe::egui::{
    Color32,
    UiBuilder,
    Sense,
    Response,
    Vec2,
    Frame,
    Margin,
    Rounding,
    Shadow,
    Stroke,
    Style,
    Ui,
};
use egui::vec2;


/// Hex: #252323
pub const HOVER_COLOR: Color32 = Color32::from_rgba_premultiplied(37, 35, 35, 140);

/// Hex: #373535
pub const HOVER_COLOR_2: Color32 = Color32::from_rgba_premultiplied(55, 53, 53, 255);

/// Hex: #252323
pub const RAISIN_BLACK: Color32 = Color32::from_rgba_premultiplied(37, 35, 35, 255);

/// Hex: 353131
pub const JET_BLACK: Color32 = Color32::from_rgba_premultiplied(53, 49, 49, 255);

/// Hex: #1E1E1E
pub const BACKGROUND_COLOR: Color32 = Color32::from_rgba_premultiplied(30, 30, 30, 255);

/// Hex: #009688
pub const HIGHLIGHT_COLOR_2: Color32 = Color32::from_rgba_premultiplied(0, 150, 136, 255);

/// Hex: #252F6A
pub const HIGHLIGHT_COLOR_3: Color32 = Color32::from_rgba_premultiplied(37, 47, 106, 255);

/// Hex: #E0E0E0
pub const PRIMARY_TEXT_COLOR: Color32 = Color32::from_rgba_premultiplied(224, 224, 224, 255);

/// Hex: #A0A0A0
pub const SECONDARY_TEXT_COLOR: Color32 = Color32::from_rgba_premultiplied(160, 160, 160, 255);

/// Hex: #424242
pub const DISABLED_COLOR: Color32 = Color32::from_rgba_premultiplied(66, 66, 66, 255);

// Colors used for Frames

/// Hex: #3F51B5
/// 
/// Bg color for [Frame]
pub const FRAME_BG_COLOR: Color32 = Color32::from_rgba_premultiplied(63, 81, 181, 255);

/// Hex: #5969c5
/// 
/// Use it if the bg color is [FRAME_BG_COLOR]
pub const FRAME_BG_ON_HOVER: Color32 = Color32::from_rgba_premultiplied(89, 105, 197, 180);

/// Hex: #E0E0E0
pub const FRAME_BORDER_COLOR: Color32 = Color32::from_rgba_premultiplied(224, 224, 224, 255);

/// Reapply the style if needed
pub fn apply_style(ui: &mut Ui, style: &Style) {
    ui.style_mut().visuals = style.visuals.clone();
    ui.style_mut().visuals.widgets = style.visuals.widgets.clone();
}

/// What we use as an outter frame to put a Ui inside it, it gets its color from the Shadow color
pub fn outter_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(6.0),
        outer_margin: Margin::same(6.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: vec2(0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 33),
        },
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(0.0, Color32::WHITE),
    }
}

/// What we use as a highlight frame
/// It is lighter compared to [outter_frame()]
pub fn highlight_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(6.0),
        outer_margin: Margin::same(0.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: vec2(0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
        },
        fill: JET_BLACK,
        stroke: Stroke::new(0.0, Color32::WHITE),
    }
}

/// A Frame used for icons
pub fn icon_frame(rounding: f32) -> Frame {
    Frame::none().rounding(Rounding::same(rounding))
}

/// A frame to put widgets inside
pub fn widget_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(6.0),
        outer_margin: Margin::same(0.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: vec2(0.0, 0.0),
            blur: 0.0,
            spread: 0.0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
        },
        fill: FRAME_BG_COLOR,
        stroke: Stroke::new(0.0, Color32::WHITE),
    }
}

/// A frame that is used for a popup window
pub fn window_frame() -> Frame {
    Frame {
        inner_margin: Margin::same(6.0),
        outer_margin: Margin::same(0.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: vec2(5.0, 8.0),
            blur: 20.0,
            spread: 5.0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 46),
        },
        fill: RAISIN_BLACK,
        stroke: Stroke::new(0.0, Color32::WHITE),
    }
}

/// Default theme for a button
pub fn button_theme(ui: &mut Ui) {
    ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.visuals_mut().widgets.hovered.weak_bg_fill = HOVER_COLOR_2;
    ui.visuals_mut().button_frame = true;
}

pub fn text_edit_theme(ui: &mut Ui) {
    ui.visuals_mut().extreme_bg_color = Color32::TRANSPARENT;
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1.0, PRIMARY_TEXT_COLOR);
}

/// Button color
pub fn button_color(ui: &mut Ui, color: Color32) {
    ui.visuals_mut().widgets.inactive.weak_bg_fill = color;
}

/// Background color for TextEdit
pub fn text_edit_bg_color(ui: &mut Ui, color: Color32) {
    ui.visuals_mut().extreme_bg_color = color;
}

/// Give inactive widgets a border color aka bg stroke
pub fn inactive_border_color(ui: &mut Ui, width: f32, color: Color32) {
    ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(width, color);
}

/// Give widgets when they hovered a border color aka bg stroke
pub fn on_hover_border_color(ui: &mut Ui, width: f32, color: Color32) {
    ui.visuals_mut().widgets.hovered.bg_stroke = Stroke::new(width, color);
}

/// Give widgets when they are hovered a bg fill color
pub fn on_hover_bg_color(ui: &mut Ui, color: Color32) {
    ui.visuals_mut().widgets.hovered.weak_bg_fill = color;
}

#[derive(Clone, Debug)]
pub struct FrameVisuals {
    pub on_hover_bg: Color32,
    pub on_click_bg: Color32,
    pub on_hover_border: (f32, Color32),
    pub on_click_border: (f32, Color32),
}

impl FrameVisuals {

    pub fn new() -> Self {
        Self {
            on_hover_bg: FRAME_BG_COLOR,
            on_click_bg: FRAME_BG_COLOR,
            on_hover_border: (1.0, PRIMARY_TEXT_COLOR),
            on_click_border: (1.0, PRIMARY_TEXT_COLOR)
        }
    }

    pub fn none() -> Self {
        Self {
            on_hover_bg: Color32::TRANSPARENT,
            on_click_bg: Color32::TRANSPARENT,
            on_hover_border: (0.0, Color32::TRANSPARENT),
            on_click_border: (0.0, Color32::TRANSPARENT)
        }
    }

    /// Used for icons
    pub fn icon() -> Self {
        Self {
            on_hover_bg: Color32::TRANSPARENT,
            on_click_bg: Color32::TRANSPARENT,
            on_hover_border: (1.0, PRIMARY_TEXT_COLOR),
            on_click_border: (1.0, PRIMARY_TEXT_COLOR)
        }
    }

    pub fn on_hover_bg(mut self, on_hover_bg: Color32) -> Self {
        self.on_hover_bg = on_hover_bg;
        self
    }

    pub fn on_click_bg(mut self, on_click_bg: Color32) -> Self {
        self.on_click_bg = on_click_bg;
        self
    }

    pub fn on_hover_border(mut self, on_hover_border: (f32, Color32)) -> Self {
        self.on_hover_border = on_hover_border;
        self
    }

    pub fn on_click_border(mut self, on_click_border: (f32, Color32)) -> Self {
        self.on_click_border = on_click_border;
        self
    }
}

/// Put this ui on top of this frame
pub fn frame_it(frame: &mut Frame, visuals: FrameVisuals, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) -> Response {
    let mut frame = frame.begin(ui);
    let res = frame.content_ui.scope(|ui| {
        add_contents(ui)
    });

    if res.response.interact(Sense::click()).clicked() {
        frame.frame = frame.frame.fill(visuals.on_click_bg);
        frame.frame = frame.frame.stroke(Stroke::new(visuals.on_click_border.0, visuals.on_click_border.1));
    } else if res.response.hovered() {
        frame.frame = frame.frame.fill(visuals.on_hover_bg);
        frame.frame = frame.frame.stroke(Stroke::new(visuals.on_hover_border.0, visuals.on_hover_border.1));
    }
    frame.end(ui);
    res.response
}
