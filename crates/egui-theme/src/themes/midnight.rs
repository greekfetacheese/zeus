use egui::{
    style::{ Selection, WidgetVisuals, Widgets },
    Color32,
    Frame,
    Margin,
    Rounding,
    Shadow,
    Stroke,
    Style,
    Visuals,
};
use super::super::{ Theme, ThemeKind, ThemeColors, FrameVisuals };

/// Hex: #252323
pub const BLACK: Color32 = Color32::from_rgba_premultiplied(37, 35, 35, 255);

/// Hex: #1F1D1D
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(31, 29, 29, 255);

/// Hex: #141415
pub const RAISIN_BLACK: Color32 = Color32::from_rgba_premultiplied(40, 40, 42, 255);

/// Hex: #3B1C32
pub const DARK_PURPLE: Color32 = Color32::from_rgba_premultiplied(59, 28, 50, 255);

/// Hex: #8F2872
pub const BYZANTIUM: Color32 = Color32::from_rgba_premultiplied(143, 40, 114, 255);

/// Hex: #A64D79
pub const MAGENTA_HAZE: Color32 = Color32::from_rgba_premultiplied(166, 77, 121, 255);

/// Hex: #F6F4F0
pub const WHITE: Color32 = Color32::from_rgba_premultiplied(246, 244, 240, 255);

/// Return this theme
pub fn theme() -> Theme {
    Theme {
        kind: ThemeKind::Midnight,
        style: style(),
        colors: colors(),
        frame1: frame1(),
        frame2: frame2(),
        frame1_visuals: frame1_visuals(),
        frame2_visuals: frame2_visuals(),
    }
}

/// Return the style for this theme
fn style() -> Style {
    let widgets = widgets(colors());
    let visuals = visuals(widgets);
    Style {
        visuals,
        ..Default::default()
    }
}

/// Return the theme colors for this theme
fn colors() -> ThemeColors {
    ThemeColors {
        bg_color: BLACK,
        highlight: Color32::from_gray(25),
        text_secondary: Color32::from_gray(150),
        overlay_color: OVERLAY,
        widget_bg_color_idle: MAGENTA_HAZE,
        widget_bg_color_click: DARK_PURPLE,
        widget_bg_color_hover: BYZANTIUM,
        widget_bg_color_open: DARK_PURPLE,
        text_color: WHITE,
        border_color_idle: BYZANTIUM,
        border_color_click: MAGENTA_HAZE,
        border_color_hover: MAGENTA_HAZE,
        border_color_open: DARK_PURPLE,
    }
}

fn frame1() -> Frame {
    Frame {
        inner_margin: Margin::same(10.0),
        outer_margin: Margin::same(10.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: (0.0, 0.0).into(),
            blur: 5.0,
            spread: 0.0,
            color: Color32::from_rgba_premultiplied(0, 0, 0, 128),
        },
        fill: MAGENTA_HAZE,
        stroke: Stroke::NONE,
    }
}

fn frame2() -> Frame {
    Frame {
        inner_margin: Margin::same(10.0),
        outer_margin: Margin::same(10.0),
        rounding: Rounding::same(10.0),
        shadow: Shadow {
            offset: (0.0, 0.0).into(),
            blur: 0.0,
            spread: 0.0,
            color: Color32::TRANSPARENT,
        },
        fill: Color32::from_rgba_premultiplied(24, 22, 22, 173),
        stroke: Stroke::NONE,
    }
}

fn frame1_visuals() -> FrameVisuals {
    FrameVisuals {
        bg_on_hover: BYZANTIUM,
        bg_on_click: DARK_PURPLE,
        border_on_hover: (0.0, MAGENTA_HAZE),
        border_on_click: (1.0, MAGENTA_HAZE),
    }
}

fn frame2_visuals() -> FrameVisuals {
    FrameVisuals {
        bg_on_hover: BYZANTIUM,
        bg_on_click: DARK_PURPLE,
        border_on_hover: (0.0, BYZANTIUM),
        border_on_click: (0.0, MAGENTA_HAZE),
    }
}

/// Return the visuals for this theme
fn visuals(widgets: Widgets) -> Visuals {
    Visuals {
        dark_mode: true,
        override_text_color: Some(WHITE),
        widgets,
        selection: Selection {
            bg_fill: MAGENTA_HAZE,
            stroke: Stroke {
                width: 1.0,
                color: MAGENTA_HAZE,
            },
        },
        hyperlink_color: MAGENTA_HAZE,
        faint_bg_color: MAGENTA_HAZE,
        extreme_bg_color: BLACK,
        code_bg_color: MAGENTA_HAZE,
        warn_fg_color: Color32::RED,
        error_fg_color: Color32::RED,
        window_rounding: Rounding::same(10.0),
        window_shadow: Shadow {
            offset: (0.0, 0.0).into(),
            blur: 0.0,
            spread: 0.0,
            color: Color32::TRANSPARENT,
        },
        window_fill: RAISIN_BLACK,
        window_stroke: Stroke::NONE,
        window_highlight_topmost: true,
        menu_rounding: Rounding::same(10.0),
        panel_fill: DARK_PURPLE,
        popup_shadow: Shadow {
            offset: (0.0, 0.0).into(),
            blur: 0.0,
            spread: 0.0,
            color: Color32::TRANSPARENT,
        },
        resize_corner_size: 0.0,
        button_frame: true,
        ..Default::default()
    }
}

/// Return the widget visuals for this theme
fn widgets(colors: ThemeColors) -> Widgets {
    let noninteractive = WidgetVisuals {
        bg_fill: DARK_PURPLE,
        weak_bg_fill: DARK_PURPLE,
        bg_stroke: Stroke {
            width: 0.0,
            color: WHITE,
        },
        rounding: Rounding::same(10.0),
        fg_stroke: Stroke {
            width: 1.0,
            color: WHITE,
        },
        expansion: 0.0,
    };

    let inactive = WidgetVisuals {
        bg_fill: MAGENTA_HAZE,
        weak_bg_fill: colors.widget_bg_color_idle,
        bg_stroke: Stroke {
            width: 0.0,
            color: WHITE,
        },
        rounding: Rounding::same(10.0),
        fg_stroke: Stroke {
            width: 1.0,
            color: WHITE,
        },
        expansion: 0.0,
    };

    let hovered = WidgetVisuals {
        bg_fill: BYZANTIUM,
        weak_bg_fill: colors.widget_bg_color_hover,
        bg_stroke: Stroke {
            width: 0.0,
            color: MAGENTA_HAZE,
        },
        rounding: Rounding::same(10.0),
        fg_stroke: Stroke {
            width: 1.0,
            color: WHITE,
        },
        expansion: 0.0,
    };

    let active = WidgetVisuals {
        bg_fill: MAGENTA_HAZE,
        weak_bg_fill: colors.widget_bg_color_click,
        bg_stroke: Stroke {
            width: 1.0,
            color: WHITE,
        },
        rounding: Rounding::same(10.0),
        fg_stroke: Stroke {
            width: 1.0,
            color: WHITE,
        },
        expansion: 0.0,
    };

    let open = WidgetVisuals {
        bg_fill: DARK_PURPLE,
        weak_bg_fill: colors.widget_bg_color_open,
        bg_stroke: Stroke {
            width: 0.0,
            color: WHITE,
        },
        rounding: Rounding::same(10.0),
        fg_stroke: Stroke {
            width: 0.0,
            color: WHITE,
        },
        expansion: 0.0,
    };

    Widgets {
        noninteractive,
        inactive,
        hovered,
        active,
        open,
    }
}
