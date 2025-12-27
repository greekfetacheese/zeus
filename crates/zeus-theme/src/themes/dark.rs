use super::super::{
   FrameVisuals, TextSizes, Theme, OverlayManager, ThemeColors, ThemeKind, visuals::*,
};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Spacing, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Background

const BG: Color32 = Color32::from_rgba_premultiplied(17, 17, 18, 255);
const BG2: Color32 = Color32::from_rgba_premultiplied(24, 24, 26, 255);
const BG3: Color32 = Color32::from_rgba_premultiplied(31, 31, 34, 255);
const BG4: Color32 = Color32::from_rgba_premultiplied(43, 43, 48, 255);

const TEXT: Color32 = Color32::from_rgba_premultiplied(204, 204, 204, 255);
const TEXT_MUTED: Color32 = Color32::from_rgba_premultiplied(127, 127, 127, 255);

const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(160, 160, 160, 255);

const BORDER: Color32 = Color32::from_rgba_premultiplied(46, 46, 46, 255);

const PRIMARY: Color32 = Color32::from_rgba_premultiplied(221, 152, 198, 255);
const SECONDARY: Color32 = Color32::from_rgba_premultiplied(118, 198, 157, 255);

// Semantic

const ERROR: Color32 = Color32::from_rgba_premultiplied(237, 57, 57, 255);
const WARNING: Color32 = Color32::from_rgba_premultiplied(228, 128, 25, 255);
const SUCCESS: Color32 = Color32::from_rgba_premultiplied(72, 182, 120, 255);
const INFO: Color32 = Color32::from_rgba_premultiplied(195, 153, 255, 255);

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
      button_visuals_1: button_visuals_1(),
      button_visuals_2: button_visuals_2(),
      button_visuals_3: button_visuals_3(),
      label_visuals_1: label_visuals_1(),
      label_visuals_2: label_visuals_2(),
      label_visuals_3: label_visuals_3(),
      combo_box_visuals_1: combo_box_visuals_1(),
      combo_box_visuals_2: combo_box_visuals_1(),
      combo_box_visuals_3: combo_box_visuals_3(),
      text_edit_visuals_1: text_edit_visuals_1(),
      text_edit_visuals_2: text_edit_visuals_2(),
      text_edit_visuals_3: text_edit_visuals_3(),
      bg: BG,
      bg2: BG2,
      bg3: BG3,
      bg4: BG4,
      text: TEXT,
      text_muted: TEXT_MUTED,
      highlight: HIGHLIGHT,
      border: BORDER,
      primary: PRIMARY,
      secondary: SECONDARY,
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
      fill: colors.bg4,
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
      fill: colors.bg2,
      stroke: Stroke::new(0.0, colors.border),
      shadow: Shadow::NONE,
      ..Default::default()
   }
}

pub fn frame1_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.bg3,
      bg_on_click: colors.bg3,
      border_on_hover: (1.0, colors.border),
      border_on_click: (1.0, colors.border),
   }
}

/// Frame for nested elements, like individual list items.
pub fn frame2(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(10),
      outer_margin: Margin::same(10),
      fill: colors.bg3,
      stroke: Stroke::NONE,
      ..Default::default()
   }
}

pub fn frame2_visuals(colors: &ThemeColors) -> FrameVisuals {
   FrameVisuals {
      bg_on_hover: colors.bg4,
      bg_on_click: colors.bg4,
      border_on_hover: (0.0, colors.border),
      border_on_click: (0.0, colors.border),
   }
}

/// Button visuals on BG1
pub fn button_visuals_1() -> ButtonVisuals {
   ButtonVisuals {
      text: TEXT,
      bg: BG3,
      bg_hover: BG4,
      bg_click: BG3,
      bg_selected: BG4,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, Color32::TRANSPARENT),
      border_click: Stroke::new(1.0, Color32::TRANSPARENT),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 6,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 115),
      },
   }
}

/// Button visuals on BG2
pub fn button_visuals_2() -> ButtonVisuals {
   ButtonVisuals {
      border: Stroke::new(1.0, Color32::TRANSPARENT),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 17,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 79),
      },
      ..button_visuals_1()
   }
}

/// Button visuals on BG3
pub fn button_visuals_3() -> ButtonVisuals {
   ButtonVisuals {
      text: TEXT,
      bg: BG4,
      bg_hover: BG3,
      bg_click: BG4,
      bg_selected: BG3,
      border: Stroke::new(1.0, BORDER),
      ..button_visuals_2()
   }
}

pub fn combo_box_visuals_1() -> ComboBoxVisuals {
   ComboBoxVisuals {
      bg: BG3,
      icon: TEXT,
      bg_hover: BG4,
      bg_open: BG3,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, BORDER),
      border_open: Stroke::new(1.0, BORDER),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow::NONE,
   }
}

pub fn combo_box_visuals_2() -> ComboBoxVisuals {
   combo_box_visuals_1()
}

pub fn combo_box_visuals_3() -> ComboBoxVisuals {
   ComboBoxVisuals {
      bg: BG4,
      icon: TEXT,
      bg_hover: BG3,
      bg_open: BG2,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, BORDER),
      border_open: Stroke::new(1.0, BORDER),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow::NONE,
   }
}


pub fn label_visuals_1() -> ButtonVisuals {
   ButtonVisuals {
      bg: Color32::TRANSPARENT,
      border: Stroke::new(1.0, Color32::TRANSPARENT),
      ..button_visuals_1()
   }
}

pub fn label_visuals_2() -> ButtonVisuals {
   ButtonVisuals {
      ..label_visuals_1()
   }
}

pub fn label_visuals_3() -> ButtonVisuals {
   ButtonVisuals {
      bg_hover: BG4,
      bg_click: BG3,
      bg_selected: BG4,
      ..label_visuals_2()
   }
}

pub fn text_edit_visuals_1() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: BG2,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, HIGHLIGHT),
      border_open: Stroke::new(1.0, HIGHLIGHT),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow::NONE,
   }
}

pub fn text_edit_visuals_2() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: BG2,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, HIGHLIGHT),
      border_open: Stroke::new(1.0, HIGHLIGHT),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow::NONE,
   }
}

pub fn text_edit_visuals_3() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: BG2,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, HIGHLIGHT),
      border_open: Stroke::new(1.0, HIGHLIGHT),
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
         bg_fill: colors.bg4, // affects selected text color, combox selected item bg
         stroke: Stroke::new(1.0, colors.highlight), // also affects TextEdit border color when active
      },
      hyperlink_color: colors.info,
      faint_bg_color: colors.bg,
      extreme_bg_color: colors.bg2,
      code_bg_color: colors.bg,
      warn_fg_color: colors.warning,
      error_fg_color: colors.error,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow::NONE,
      window_fill: colors.bg2,
      window_stroke: Stroke::new(1.0, colors.border),
      panel_fill: colors.bg,
      ..Default::default()
   }
}

fn widgets(colors: ThemeColors) -> Widgets {
   let base_visuals = WidgetVisuals {
      bg_fill: colors.bg3,
      weak_bg_fill: colors.bg3,
      bg_stroke: Stroke::new(1.0, colors.border),
      corner_radius: CornerRadius::same(4),
      fg_stroke: Stroke::new(1.0, colors.text),
      expansion: 0.0,
   };

   let mut non_interactive_base = base_visuals.clone();
   non_interactive_base.bg_stroke.width = 1.0;

   // Set inactive bg to border color
   // Because widgets like sliders dont get a border and it will not distinguish
   // from the bg color
   let mut inactive_visuals = base_visuals.clone();
   inactive_visuals.bg_fill = colors.border;

   Widgets {
      noninteractive: non_interactive_base,
      inactive: inactive_visuals,
      hovered: WidgetVisuals {
         bg_fill: colors.bg3,
         weak_bg_fill: colors.bg4,
         bg_stroke: Stroke::new(1.0, colors.highlight),
         ..base_visuals
      },
      active: WidgetVisuals {
         bg_fill: colors.bg,
         weak_bg_fill: colors.bg,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
      open: WidgetVisuals {
         bg_fill: colors.bg,
         weak_bg_fill: colors.bg,
         bg_stroke: Stroke::new(1.0, colors.border),
         ..base_visuals
      },
   }
}
