use super::super::{FrameVisuals, TextSizes, Theme, ThemeColors, ThemeKind, WidgetVisuals as ThemeWidgetVisuals};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

const MANTLE: Color32 = Color32::from_rgb(24, 24, 37); // #181825

/// Main background color
const MAIN_BG: Color32 = Color32::from_rgb(49, 50, 68); // #313244

/// Extreme background color, makes great contrast with the main background
/// Used for text edits, comboboxes, or anything that needs a good contrast to stand out
const EXTREME_BG: Color32 = Color32::from_rgb(30, 30, 46); // #1E1E2E

const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90); // #45475A

const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112); // #585B70

const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156); // #7F849C

const TEXT: Color32 = Color32::from_rgb(205, 214, 244); // #CDD6F4

const SUBTEXT1: Color32 = Color32::from_rgb(186, 194, 222); // #BAC2DE

const BLUE: Color32 = Color32::from_rgb(137, 180, 250); // #89B4FA

// const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254); // #B4BEFE

const BLACK_SHADE: Color32 = Color32::from_rgb(24, 25, 37);

const LIGHT_BLACK_SHADE: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 40);

const LIGHT_BLACK_SHADE2: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 50);

const LIGHT_BLACK_SHADE3: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 128);

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
      frame1_visuals: frame1_visuals(),
      frame2_visuals: frame2_visuals(),
   }
}

/// Button visuals based on the bg color
pub fn button_visuals(bg_color: Color32, colors: &ThemeColors) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: EXTREME_BG,
      bg_color_on_hover: BLACK_SHADE,
      bg_color_on_click: EXTREME_BG,
      border_on_idle: (0.0, Color32::TRANSPARENT),
      border_on_hover: (0.0, Color32::TRANSPARENT),
      border_on_click: (0.0, Color32::TRANSPARENT),
      ..Default::default()
   };

   if bg_color == colors.bg_color {
      visuals
   } else if bg_color == colors.secondary_bg_color || bg_color == colors.extreme_bg_color2 {
      visuals.bg_color_on_idle = colors.button_bg2;
      visuals.bg_color_on_hover = LITE_BLUE_SHADE2;
      visuals.bg_color_on_click = LITE_BLUE_SHADE3;
      visuals
   } else {
      visuals
   }
}

/// TextEdit visuals based on the bg color
pub fn text_edit_visuals(bg_color: Color32, colors: &ThemeColors) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: EXTREME_BG,
      bg_color_on_hover: Color32::TRANSPARENT,
      bg_color_on_click: Color32::TRANSPARENT,
      bg_color_on_open: Color32::TRANSPARENT,
      border_on_idle: (1.0, Color32::TRANSPARENT),
      border_on_hover: (1.0, Color32::TRANSPARENT),
      border_on_click: (1.0, Color32::TRANSPARENT),
      border_on_open: (1.0, OVERLAY1),
      combobox_bg: EXTREME_BG,
   };
   if bg_color == colors.bg_color {
      visuals
   } else if bg_color == colors.secondary_bg_color || bg_color == colors.extreme_bg_color2 {
      visuals.bg_color_on_idle = colors.text_edit_bg2;
      visuals.border_on_idle = (0.0, Color32::TRANSPARENT);
      visuals.border_on_hover = (0.0, Color32::TRANSPARENT);
      visuals.border_on_click = (0.0, Color32::TRANSPARENT);
      visuals
   } else {
      visuals
   }
}

/// Widget visuals based on the bg color
pub fn widget_visuals(bg_color: Color32, colors: &ThemeColors) -> ThemeWidgetVisuals {
   let mut visuals = ThemeWidgetVisuals {
      bg_color_on_idle: EXTREME_BG,
      bg_color_on_hover: BLACK_SHADE,
      bg_color_on_click: EXTREME_BG,
      bg_color_on_open: EXTREME_BG,
      combobox_bg: EXTREME_BG,
      border_on_idle: (1.0, Color32::TRANSPARENT),
      border_on_hover: (1.0, Color32::TRANSPARENT),
      border_on_click: (1.0, Color32::TRANSPARENT),
      border_on_open: (1.0, Color32::TRANSPARENT),
   };

   if bg_color == colors.bg_color {
      visuals
   } else if bg_color == colors.secondary_bg_color || bg_color == colors.extreme_bg_color2 {
      visuals.bg_color_on_idle = colors.widget_bg_color2;
      visuals.combobox_bg = colors.extreme_bg_color2;
      // ! On hover and click colors need improvement
      visuals.bg_color_on_hover = EXTREME_BG;
      visuals.bg_color_on_click = EXTREME_BG;
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
      bg_color: MAIN_BG,
      secondary_bg_color: LIGHT_BLACK_SHADE2,
      extreme_bg_color: EXTREME_BG,
      extreme_bg_color2: BLACK_SHADE,
      window_fill: MAIN_BG,
      highlight1: LIGHT_BLACK_SHADE2,
      highlight2: LIGHT_BLACK_SHADE3,
      text_color: TEXT,
      text_secondary: SUBTEXT1,
      text_edit_bg: EXTREME_BG,
      text_edit_bg2: BLACK_SHADE,
      overlay_color: MANTLE,
      button_bg: EXTREME_BG,
      button_bg2: LITE_BLUE_SHADE,
      widget_bg_color: EXTREME_BG,
      widget_bg_color2: BLACK_SHADE,
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

// Make good contrast with frame1
fn frame2() -> Frame {
   Frame {
      corner_radius: CornerRadius::same(5),
      inner_margin: Margin::same(10),
      fill: BLACK_SHADE, // ! Need to change this because it has the same color as the secondary widget color
      ..Default::default()
   }
}

fn frame1_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: SURFACE2,
      bg_on_click: BLUE,
      border_on_hover: (0.0, OVERLAY1),
      border_on_click: (0.0, BLUE),
   }
}

fn frame2_visuals() -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: SURFACE2,
      bg_on_click: BLUE,
      border_on_hover: (0.0, OVERLAY1),
      border_on_click: (0.0, BLUE),
   }
}

/// Return the visuals for this theme
fn visuals(widgets: Widgets) -> Visuals {
   Visuals {
      dark_mode: true,
      override_text_color: Some(TEXT),
      widgets,
      selection: Selection {
         bg_fill: SURFACE1,
         stroke: Stroke {
            width: 1.0,
            color: SURFACE1,
         },
      },
      hyperlink_color: SURFACE1,
      faint_bg_color: MANTLE,
      extreme_bg_color: EXTREME_BG, // This also affects the background of the TextEdit
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
      window_fill: MAIN_BG,
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
      weak_bg_fill: colors.widget_bg_color,
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
      weak_bg_fill: colors.widget_bg_color,
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
