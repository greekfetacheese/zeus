use super::super::{
   FrameVisuals, OverlayManager, TextSizes, Theme, ThemeColors, ThemeKind, visuals::*,
};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Spacing, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Color Pallete

pub const DARK: Color32 = Color32::from_rgba_premultiplied(22, 22, 30, 255);
pub const DARK2: Color32 = Color32::from_rgba_premultiplied(26, 27, 38, 255);
pub const DARK3: Color32 = Color32::from_rgba_premultiplied(40, 47, 65, 255);
pub const POWDER_BLUE: Color32 = Color32::from_rgba_premultiplied(190, 204, 244, 255);
pub const SOFT_BLUE: Color32 = Color32::from_rgba_premultiplied(65, 73, 97, 255);
pub const DUSK: Color32 = Color32::from_rgba_premultiplied(83, 97, 136, 255);
pub const LIGHT_BLUE: Color32 = Color32::from_rgba_premultiplied(118, 210, 253, 255);
pub const SALMON_PINK: Color32 = Color32::from_rgba_premultiplied(255, 91, 103, 255);
pub const CAMEL: Color32 = Color32::from_rgba_premultiplied(218, 147, 61, 255);
pub const GREEN: Color32 = Color32::from_rgba_premultiplied(72, 182, 120, 255);
pub const PASTEL_PURPLE: Color32 = Color32::from_rgba_premultiplied(194, 111, 255, 255);
pub const FADED_BLUE: Color32 = Color32::from_rgba_premultiplied(65, 73, 97, 118);

const TITLE_BAR: Color32 = DARK;
const MAIN_BG: Color32 = DARK;
const WIDGET_BG: Color32 = DARK2;
const HOVER: Color32 = DARK3;
const TEXT: Color32 = POWDER_BLUE;
const TEXT_MUTED: Color32 = DUSK;
const HIGHLIGHT: Color32 = HOVER;
const BORDER: Color32 = SOFT_BLUE;
const ACCENT: Color32 = LIGHT_BLUE;
const ERROR: Color32 = SALMON_PINK;
const WARNING: Color32 = CAMEL;
const SUCCESS: Color32 = GREEN;
const INFO: Color32 = PASTEL_PURPLE;

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      dark_mode: true,
      overlay_manager: OverlayManager::new(),
      image_tint_recommended: true,
      kind: ThemeKind::TokyoNight,
      style: style(),
      colors: colors(),
      text_sizes: text_sizes(),
      window_frame: window_frame(&colors()),
      frame1: frame1(&colors()),
      frame2: frame2(&colors()),
      frame1_visuals: frame1_visuals(&colors()),
      frame2_visuals: frame2_visuals(&colors()),
   }
}

/// Return the theme colors for this theme
fn colors() -> ThemeColors {
   ThemeColors {
      button_visuals: button_visuals(),
      label_visuals: label_visuals(),
      combo_box_visuals: combo_box_visuals(),
      text_edit_visuals: text_edit_visuals(),
      title_bar: TITLE_BAR,
      bg: MAIN_BG,
      widget_bg: WIDGET_BG,
      hover: HOVER,
      text: TEXT,
      text_muted: TEXT_MUTED,
      highlight: HIGHLIGHT,
      border: BORDER,
      accent: ACCENT,
      error: ERROR,
      warning: WARNING,
      success: SUCCESS,
      info: INFO,
   }
}

fn text_sizes() -> TextSizes {
   TextSizes::new(12.0, 14.0, 16.0, 18.0, 20.0, 26.0)
}

pub fn window_frame(colors: &ThemeColors) -> Frame {
   Frame {
      fill: colors.title_bar,
      inner_margin: Margin::ZERO,
      outer_margin: Margin::ZERO,
      ..Default::default()
   }
}

/// Base container frame for major UI sections.
pub fn frame1(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      fill: colors.widget_bg,
      stroke: Stroke::new(0.0, colors.border),
      shadow: Shadow::NONE,
      ..Default::default()
   }
}

pub fn frame1_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.hover,
      bg_on_click: colors.widget_bg,
      border_on_hover: (0.0, colors.highlight),
      border_on_click: (0.0, colors.highlight),
   }
}

/// Frame for nested elements, like individual list items.
pub fn frame2(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      outer_margin: Margin::same(10),
      fill: colors.bg,
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

pub fn frame2_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.hover,
      bg_on_click: colors.bg,
      border_on_hover: (0.0, colors.highlight),
      border_on_click: (0.0, colors.highlight),
   }
}

pub fn button_visuals() -> ButtonVisuals {
   ButtonVisuals {
      text: TEXT,
      bg: WIDGET_BG,
      bg_hover: HOVER,
      bg_click: WIDGET_BG,
      bg_selected: HIGHLIGHT,
      border: Stroke::new(1.0, Color32::TRANSPARENT),
      border_hover: Stroke::new(1.0, DUSK),
      border_click: Stroke::new(1.0, Color32::TRANSPARENT),
      corner_radius: CornerRadius::same(3),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 2,
         spread: 1,
         color: SOFT_BLUE,
      },
   }
}

pub fn combo_box_visuals() -> ComboBoxVisuals {
   ComboBoxVisuals {
      bg: WIDGET_BG,
      icon: TEXT,
      bg_hover: HOVER,
      bg_open: WIDGET_BG,
      border: Stroke::new(1.0, Color32::TRANSPARENT),
      border_hover: Stroke::new(1.0, DUSK),
      border_open: Stroke::new(1.0, Color32::TRANSPARENT),
      corner_radius: CornerRadius::same(3),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 2,
         spread: 1,
         color: SOFT_BLUE,
      },
   }
}

pub fn label_visuals() -> ButtonVisuals {
   ButtonVisuals {
      bg: Color32::TRANSPARENT,
      border: Stroke::new(1.0, Color32::TRANSPARENT),
      border_hover: Stroke::new(1.0, Color32::TRANSPARENT),
      border_click: Stroke::new(1.0, Color32::TRANSPARENT),
      ..button_visuals()
   }
}

pub fn text_edit_visuals() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: WIDGET_BG,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, LIGHT_BLUE),
      border_open: Stroke::new(1.0, LIGHT_BLUE),
      corner_radius: CornerRadius::same(3),
      shadow: Shadow::NONE,
   }
}

fn style() -> Style {
   let widgets = widgets(colors());
   let visuals = visuals(widgets, &colors());
   let spacing = Spacing {
      window_margin: Margin::same(10),
      ..Default::default()
   };

   Style {
      visuals,
      animation_time: 0.3,
      spacing,
      ..Default::default()
   }
}

fn visuals(widgets: Widgets, colors: &ThemeColors) -> Visuals {
   Visuals {
      dark_mode: true,
      override_text_color: Some(colors.text),
      widgets,
      selection: Selection {
         bg_fill: colors.highlight, // affects selected text color, combox selected item bg
         stroke: Stroke::new(1.0, colors.highlight), // also affects TextEdit border color when active
      },
      hyperlink_color: colors.info,
      faint_bg_color: colors.bg,
      extreme_bg_color: colors.widget_bg,
      code_bg_color: colors.bg,
      warn_fg_color: colors.warning,
      error_fg_color: colors.error,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 3,
         spread: 0,
         color: FADED_BLUE,
      },
      window_fill: colors.bg,
      window_stroke: Stroke::new(1.0, Color32::TRANSPARENT),
      panel_fill: colors.bg,
      ..Default::default()
   }
}

fn widgets(colors: ThemeColors) -> Widgets {
   let base_visuals = WidgetVisuals {
      bg_fill: colors.widget_bg,
      weak_bg_fill: colors.bg,
      bg_stroke: Stroke::new(1.0, colors.border),
      corner_radius: CornerRadius::same(4),
      fg_stroke: Stroke::new(1.0, colors.text),
      expansion: 0.0,
   };

   let mut non_interactive_base = base_visuals.clone();
   non_interactive_base.bg_stroke.width = 1.0;

   // Set inactive bg to highlight color
   // Because widgets like sliders dont get a border and it will not distinguish
   // from the bg color
   let mut inactive_visuals = base_visuals.clone();
   inactive_visuals.bg_fill = colors.highlight;

   Widgets {
      noninteractive: non_interactive_base,
      inactive: inactive_visuals,
      hovered: WidgetVisuals {
         bg_fill: colors.widget_bg,
         weak_bg_fill: colors.highlight,
         bg_stroke: Stroke::new(1.0, colors.highlight),
         ..base_visuals
      },
      active: WidgetVisuals {
         bg_fill: colors.widget_bg,
         weak_bg_fill: colors.widget_bg,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
      open: WidgetVisuals {
         bg_fill: colors.widget_bg,
         weak_bg_fill: colors.widget_bg,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
   }
}
