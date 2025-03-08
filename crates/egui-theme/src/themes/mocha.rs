use super::super::{
   FrameVisuals, TextSizes, Theme, ThemeColors, ThemeKind, WidgetVisuals as ThemeWidgetVisuals,
};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

/// Main background
const BASE: Color32 = Color32::from_rgb(30, 30, 46); // #1E1E2E

/// Overlays
const MANTLE: Color32 = Color32::from_rgb(24, 24, 37); // #181825

/// secondary backgrounds, e.g., sidebar, windows
const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68); // #313244

/// Widget when idle (at rest)
const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90); // #45475A

/// Widget when hovered
const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112); // #585B70

/// Idle borders
const OVERLAY0: Color32 = Color32::from_rgb(108, 112, 134); // #6C7086

/// Hover borders
const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156); // #7F849C

/// Primary text
const TEXT: Color32 = Color32::from_rgb(205, 214, 244); // #CDD6F4

/// Secondary text
const SUBTEXT1: Color32 = Color32::from_rgb(186, 194, 222); // #BAC2DE

/// Widget on click, focus
const BLUE: Color32 = Color32::from_rgb(137, 180, 250); // #89B4FA

/// Highlight
const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254); // #B4BEFE

const TEXT_EDIT_BG: Color32 = Color32::from_rgb(45, 45, 61); // #2D2D3D

const LIGHT_BLACK_SHADE: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 40);

const LIGHT_BLACK_SHADE2: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 50);

const LIGHT_BLACK_SHADE3: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 128);

const LIGHT_BLACK_SHADE4: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 69);

const LITE_BLUE_SHADE: Color32 = Color32::from_rgb(45, 52, 112);

const LITE_BLUE_SHADE2: Color32 = Color32::from_rgb(41, 60, 134);

const LITE_BLUE_SHADE3: Color32 = Color32::from_rgb(59, 62, 178);

// ! For some reason when the app actually runs this becomes 250, 250, 250, 98
const VERY_LITE_WHITE: Color32 = Color32::from_rgba_premultiplied(163, 163, 163, 98);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      kind: ThemeKind::Mocha,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      frame1: frame1(),
      frame2: frame2(),
      frame3: frame3(),
      frame1_visuals: frame1_visuals(),
      frame2_visuals: frame2_visuals(),
   }
}

pub fn button_visuals(bg_color: Color32) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: LIGHT_BLACK_SHADE,
      bg_color_on_hover: LITE_BLUE_SHADE2,
      bg_color_on_click: LITE_BLUE_SHADE3,
      border_on_idle: (1.0, Color32::TRANSPARENT),
      border_on_hover: (1.0, Color32::TRANSPARENT),
      border_on_click: (1.0, Color32::TRANSPARENT),
      ..Default::default()
   };

   if bg_color == SURFACE0 {
      visuals
   } else if bg_color == LIGHT_BLACK_SHADE2 {
      visuals.bg_color_on_idle = LITE_BLUE_SHADE;
      visuals
   } else {
      visuals
   }
}

pub fn text_edit_visuals(bg_color: Color32) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: BASE,
      bg_color_on_hover: Color32::TRANSPARENT,
      bg_color_on_click: Color32::TRANSPARENT,
      bg_color_on_open: Color32::TRANSPARENT,
      border_on_idle: (1.0, Color32::TRANSPARENT),
      border_on_hover: (1.0, Color32::TRANSPARENT),
      border_on_click: (1.0, Color32::TRANSPARENT),
      border_on_open: (1.0, OVERLAY1),
   };
   if bg_color == SURFACE0 {
      visuals
   } else if bg_color == LIGHT_BLACK_SHADE2 {
      visuals.bg_color_on_idle = Color32::TRANSPARENT;
      visuals.border_on_idle = (1.0, VERY_LITE_WHITE);
      visuals.border_on_hover = (1.0, Color32::WHITE);
      visuals.border_on_click = (1.0, Color32::WHITE);
      visuals
   } else {
      visuals
   }
}

pub fn widget_visuals(bg_color: Color32) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: BASE,
      bg_color_on_hover: LITE_BLUE_SHADE2,
      bg_color_on_click: LITE_BLUE_SHADE3,
      bg_color_on_open: BASE,
      border_on_idle: (1.0, Color32::TRANSPARENT),
      border_on_hover: (1.0, Color32::TRANSPARENT),
      border_on_click: (1.0, Color32::TRANSPARENT),
      border_on_open: (1.0, Color32::TRANSPARENT),
   };

   if bg_color == SURFACE0 {
      visuals
   } else if bg_color == LIGHT_BLACK_SHADE2 {
      visuals.bg_color_on_idle = Color32::TRANSPARENT;
      visuals.border_on_idle = (1.0, VERY_LITE_WHITE);
      visuals.border_on_hover = (1.0, Color32::WHITE);
      visuals.border_on_click = (1.0, Color32::WHITE);
      visuals.border_on_open = (1.0, OVERLAY1);
      visuals
   } else {
      visuals
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
      bg_color: SURFACE0,
      secondary_bg_color: LIGHT_BLACK_SHADE2,
      extreme_bg_color: BASE,
      window_fill: LITE_BLUE_SHADE,
      combo_box_fill: Color32::TRANSPARENT,
      highlight: LAVENDER,
      text_color: TEXT,
      text_secondary: SUBTEXT1,
      text_edit_bg_color: Color32::TRANSPARENT,
      overlay_color: MANTLE,
      button_bg_color: LITE_BLUE_SHADE,
      widget_bg_color_idle: LIGHT_BLACK_SHADE,
      widget_bg_color_hover: LITE_BLUE_SHADE2,
      widget_bg_color_click: LITE_BLUE_SHADE3,
      widget_bg_color_open: Color32::TRANSPARENT,
      border_color_idle: VERY_LITE_WHITE,
      border_color_click: Color32::WHITE,
      border_color_hover: Color32::WHITE,
      border_color_open: OVERLAY1,
   }
}

fn text_sizes() -> TextSizes {
   let very_small = 12.0;
   let small = 14.0;
   let normal = 16.0;
   let large = 18.0;
   let very_large = 20.0;
   let heading = 26.0;
   TextSizes::new(very_small, small, normal, large, very_large, heading)
}

// Frames that can be used to highlight something or create a border around a widget

// Its good to give a nice bg to a ui, makes good contrsts with the main bg color
fn frame1() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(5),
      inner_margin: Margin::same(10),
      fill: LIGHT_BLACK_SHADE2,
      stroke: Stroke::NONE,
      shadow: Shadow {
         blur: 22,
         spread: 5,
         color: LIGHT_BLACK_SHADE,
         ..Default::default()
      },
      ..Default::default()
   }
}

// Good combo for a window if the bg color is the frame1
fn frame2() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(5),
      inner_margin: Margin::same(10),
      fill: SURFACE0,
      ..Default::default()
   }
}

fn frame3() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(5),
      inner_margin: Margin::same(10),
      fill: SURFACE0,
      shadow: Shadow {
         blur: 30,
         spread: 5,
         color: LIGHT_BLACK_SHADE4,
         ..Default::default()
      },
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
         blur: 20,
         spread: 1,
         color: LIGHT_BLACK_SHADE3,
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
