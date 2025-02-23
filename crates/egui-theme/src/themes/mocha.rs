use egui::{
    style::{ Selection, WidgetVisuals, Widgets },
    Color32,
    Frame,
    CornerRadius,
    Shadow,
    Stroke,
    Style,
    Visuals,
};
use super::super::{ Theme, ThemeKind, ThemeColors, FrameVisuals };

/// Main background
const BASE: Color32 = Color32::from_rgb(30, 30, 46);      // #1E1E2E

/// Overlays
const MANTLE: Color32 = Color32::from_rgb(24, 24, 37);    // #181825

/// secondary backgrounds, e.g., sidebar, windows
const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68);  // #313244

/// Widget when idle (at rest)
const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90);  // #45475A

/// Widget when hovered
const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112); // #585B70

/// Idle borders
const OVERLAY0: Color32 = Color32::from_rgb(108, 112, 134); // #6C7086

/// Hover borders
const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156); // #7F849C

/// Primary text
const TEXT: Color32 = Color32::from_rgb(205, 214, 244);   // #CDD6F4

/// Secondary text
const SUBTEXT1: Color32 = Color32::from_rgb(186, 194, 222); // #BAC2DE

/// Widget on click, focus
const BLUE: Color32 = Color32::from_rgb(137, 180, 250);   // #89B4FA

/// Highlight
const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254); // #B4BEFE



/// Return this theme
pub fn theme() -> Theme {
    Theme {
        kind: ThemeKind::Mocha,
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
        bg_color: BASE,
        highlight: LAVENDER,
        text_color: TEXT,
        text_secondary: SUBTEXT1,
        overlay_color: MANTLE,
        widget_bg_color_idle: SURFACE1,
        widget_bg_color_click: BLUE,
        widget_bg_color_hover: SURFACE2,
        widget_bg_color_open: SURFACE2,
        border_color_idle: OVERLAY0,
        border_color_click: BLUE,
        border_color_hover: OVERLAY1,
        border_color_open: OVERLAY1,
    }
}

// Frames that can be used to highlight something or create a border around a widget

fn frame1() -> Frame {
    Frame {
        corner_radius: CornerRadius::same(10),
        fill: SURFACE1,
        stroke: Stroke::NONE,
        shadow: Shadow::default(),
        ..Default::default()
    }
}

fn frame2() -> Frame {
    Frame {
        corner_radius: CornerRadius::same(10),
        fill: SURFACE2,
        stroke: Stroke::NONE,
        shadow: Shadow::default(),
        ..Default::default()
    }
}

fn frame1_visuals() -> FrameVisuals {
    FrameVisuals {
        bg_on_hover: SURFACE2,
        bg_on_click: BLUE,
        border_on_hover: (1.0, OVERLAY1),
        border_on_click: (1.0, BLUE),
    }
}

fn frame2_visuals() -> FrameVisuals {
    FrameVisuals {
        bg_on_hover: SURFACE2,
        bg_on_click: BLUE,
        border_on_hover: (1.0, OVERLAY1),
        border_on_click: (1.0, BLUE),
    }
}

/// Return the visuals for this theme
fn visuals(widgets: Widgets) -> Visuals {
    Visuals {
        dark_mode: true,
        override_text_color: Some(TEXT),
        widgets,
        selection: Selection {
            bg_fill: LAVENDER,
            stroke: Stroke {
                width: 1.0,
                color: LAVENDER,
            },
        },
        hyperlink_color: LAVENDER,
        faint_bg_color: MANTLE,
        extreme_bg_color: BASE, // This also affects the background of the TextEdit
        code_bg_color: MANTLE,
        warn_fg_color: Color32::RED,
        error_fg_color: Color32::RED,
        window_corner_radius: CornerRadius::same(10),
        window_shadow: Shadow {
            offset: (0, 0).into(),
            blur: 0,
            spread: 0,
            color: Color32::TRANSPARENT,
        },
        window_fill: SURFACE0,
        window_stroke: Stroke::NONE,
        window_highlight_topmost: true,
        menu_corner_radius: CornerRadius::same(10),
        panel_fill: SURFACE1,
        popup_shadow: Shadow {
            offset: (0, 0).into(),
            blur: 0,
            spread: 0,
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
        bg_fill: SURFACE1,
        weak_bg_fill: colors.widget_bg_color_idle,
        bg_stroke: Stroke {
            width: 0.0,
            color: TEXT,
        },
        corner_radius: CornerRadius::same(10),
        fg_stroke: Stroke {
            width: 1.0,
            color: TEXT,
        },
        expansion: 0.0,
    };

    // idle
    let inactive = WidgetVisuals {
        bg_fill: SURFACE1,
        weak_bg_fill: colors.widget_bg_color_idle,
        bg_stroke: Stroke {
            width: 0.0,
            color: TEXT,
        },
        corner_radius: CornerRadius::same(10),
        fg_stroke: Stroke {
            width: 1.0,
            color: TEXT,
        },
        expansion: 0.0,
    };


    let hovered = WidgetVisuals {
        bg_fill: SURFACE2,
        weak_bg_fill: colors.widget_bg_color_hover,
        bg_stroke: Stroke {
            width: 0.0,
            color: TEXT,
        },
        corner_radius: CornerRadius::same(10),
        fg_stroke: Stroke {
            width: 1.0,
            color: TEXT,
        },
        expansion: 0.0,
    };

    // on click
    let active = WidgetVisuals {
        bg_fill: SURFACE1,
        weak_bg_fill: colors.widget_bg_color_click,
        bg_stroke: Stroke {
            width: 1.0,
            color: BLUE,
        },
        corner_radius: CornerRadius::same(10),
        fg_stroke: Stroke {
            width: 1.0,
            color: BLUE,
        },
        expansion: 0.0,
    };

    let open = WidgetVisuals {
        bg_fill: SURFACE2,
        weak_bg_fill: colors.widget_bg_color_open,
        bg_stroke: Stroke {
            width: 0.0,
            color: TEXT,
        },
        corner_radius: CornerRadius::same(10),
        fg_stroke: Stroke {
            width: 0.0,
            color: TEXT,
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
