use super::super::{
   FrameVisuals, OverlayManager, TextSizes, Theme, ThemeColors, ThemeKind, visuals::*,
};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Spacing, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Color Pallete

pub const BLACK: Color32 = Color32::from_rgba_premultiplied(28, 28, 32, 255);
pub const DARK_GREY: Color32 = Color32::from_rgba_premultiplied(34, 34, 38, 255);
pub const DARK_GREY2: Color32 = Color32::from_rgba_premultiplied(48, 48, 54, 255);
pub const CHARCOAL_GREY: Color32 = Color32::from_rgba_premultiplied(68, 68, 76, 255);
pub const CHARCOAL_GREY2: Color32 = Color32::from_rgba_premultiplied(76, 76, 76, 255);
pub const MEDIUM_GREY: Color32 = Color32::from_rgba_premultiplied(76, 76, 76, 255);
pub const GREY: Color32 = Color32::from_rgba_premultiplied(155, 155, 155, 255);
pub const SILVER: Color32 = Color32::from_rgba_premultiplied(204, 204, 204, 255);
pub const LIGHT_RED: Color32 = Color32::from_rgba_premultiplied(237, 57, 57, 255);
pub const ORANGE: Color32 = Color32::from_rgba_premultiplied(228, 128, 25, 255);
pub const GREEN: Color32 = Color32::from_rgba_premultiplied(72, 182, 120, 255);
pub const PASTEL_PURPLE: Color32 = Color32::from_rgba_premultiplied(195, 153, 255, 255);
pub const PURPLE: Color32 = Color32::from_rgba_premultiplied(137, 91, 245, 255);

const TITLE_BAR: Color32 = BLACK;
const MAIN_BG: Color32 = DARK_GREY;
const WIDGET_BG: Color32 = BLACK;
const HOVER: Color32 = CHARCOAL_GREY;
const TEXT: Color32 = SILVER;
const TEXT_MUTED: Color32 = GREY;
const HIGHLIGHT: Color32 = CHARCOAL_GREY;
const BORDER: Color32 = CHARCOAL_GREY2;
const ACCENT: Color32 = PURPLE;
const ERROR: Color32 = LIGHT_RED;
const WARNING: Color32 = ORANGE;
const SUCCESS: Color32 = GREEN;
const INFO: Color32 = PASTEL_PURPLE;

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      dark_mode: true,
      overlay_manager: OverlayManager::new(),
      image_tint_recommended: true,
      kind: ThemeKind::Dark,
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
      border_hover: Stroke::new(1.0, Color32::TRANSPARENT),
      border_click: Stroke::new(1.0, Color32::TRANSPARENT),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 2,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 174),
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
      border_hover: Stroke::new(1.0, Color32::TRANSPARENT),
      border_open: Stroke::new(1.0, Color32::TRANSPARENT),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 2,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 174),
      },
   }
}

pub fn label_visuals() -> ButtonVisuals {
   ButtonVisuals {
      bg: Color32::TRANSPARENT,
      ..button_visuals()
   }
}

pub fn text_edit_visuals() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: WIDGET_BG,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, SILVER),
      border_open: Stroke::new(1.0, SILVER),
      corner_radius: CornerRadius::same(6),
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
      window_shadow: Shadow::NONE,
      window_fill: colors.bg,
      window_stroke: Stroke::new(1.0, colors.border),
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
