use super::super::{TextSizes, Theme, OverlayManager, FrameVisuals, visuals::*, ThemeColors, ThemeKind};
use egui::{
   Color32, CornerRadius, Frame, Margin, Shadow, Stroke, Style, Visuals,
   style::{Selection, WidgetVisuals, Widgets},
};

// Background

const BG: Color32 = Color32::from_rgba_premultiplied(226, 227, 232, 255);
const BG2: Color32 = Color32::from_rgba_premultiplied(232, 232, 232, 255);
const BG3: Color32 = Color32::from_rgba_premultiplied(246, 246, 248, 255);
const BG4: Color32 = Color32::from_rgba_premultiplied(249, 249, 249, 255);

const TEXT: Color32 = Color32::from_rgba_premultiplied(25, 25, 25, 255);
const TEXT_MUTED: Color32 = Color32::from_rgba_premultiplied(63, 63, 63, 255);

const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(44, 44, 44, 255);

const BORDER: Color32 = Color32::from_rgba_premultiplied(165, 165, 165, 255);

const PRIMARY: Color32 = Color32::from_rgba_premultiplied(221, 152, 198, 255);
const SECONDARY: Color32 = Color32::from_rgba_premultiplied(118, 198, 157, 255);

// Semantic

const ERROR: Color32 = Color32::from_rgba_premultiplied(153, 0, 32, 255);
const WARNING: Color32 = Color32::from_rgba_premultiplied(192, 71, 0, 255);
const SUCCESS: Color32 = Color32::from_rgba_premultiplied(40, 101, 26, 255);
const INFO: Color32 = Color32::from_rgba_premultiplied(111, 47, 206, 255);

/// Return this theme
pub fn theme() -> Theme {
   Theme {
      dark_mode: false,
      overlay_manager: OverlayManager::new(),
      image_tint_recommended: false,
      kind: ThemeKind::Light,
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
      button_visuals_2: button_visuals_1(),
      button_visuals_3: button_visuals_1(),
      label_visuals_1: label_visuals_1(),
      label_visuals_2: label_visuals_1(),
      label_visuals_3: label_visuals_1(),
      combo_box_visuals_1: combo_box_visuals_1(),
      combo_box_visuals_2: combo_box_visuals_1(),
      combo_box_visuals_3: combo_box_visuals_1(),
      text_edit_visuals_1: text_edit_visuals_1(),
      text_edit_visuals_2: text_edit_visuals_1(),
      text_edit_visuals_3: text_edit_visuals_1(),
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
      corner_radius: CornerRadius::same(6),
      fill: colors.bg,
      stroke: Stroke::new(1.0, colors.bg),
      ..Default::default()
   }
}

/// Base container frame for major UI sections.
pub fn frame1(colors: &ThemeColors) -> Frame {
   Frame {
      corner_radius: CornerRadius::same(6),
      inner_margin: Margin::same(12),
      fill: colors.bg2,
      stroke: Stroke::new(1.0, colors.border),
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

pub fn button_visuals_1() -> ButtonVisuals {
   ButtonVisuals {
      text: TEXT,
      bg: BG3,
      bg_hover: BG4,
      bg_click: BG3,
      bg_selected: BG4,
      border: Stroke::NONE,
      border_hover: Stroke::NONE,
      border_click: Stroke::NONE,
      corner_radius: CornerRadius::same(6),
      shadow: Shadow {
         offset: (0, 0).into(),
         blur: 6,
         spread: 1,
         color: Color32::from_rgba_premultiplied(0, 0, 0, 200),
      },
   }
}

pub fn label_visuals_1() -> ButtonVisuals {
   ButtonVisuals {
      bg: Color32::TRANSPARENT,
      ..button_visuals_1()
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

pub fn text_edit_visuals_1() -> TextEditVisuals {
   TextEditVisuals {
      text: TEXT,
      bg: BG3,
      border: Stroke::new(1.0, BORDER),
      border_hover: Stroke::new(1.0, BORDER),
      border_open: Stroke::new(1.0, BORDER),
      corner_radius: CornerRadius::same(6),
      shadow: Shadow::NONE,
   }
}

fn style() -> Style {
   let widgets = widgets(colors());
   let visuals = visuals(widgets, &colors());
   Style {
      visuals,
      animation_time: 0.3,
      ..Default::default()
   }
}

fn visuals(widgets: Widgets, colors: &ThemeColors) -> Visuals {
   Visuals {
      dark_mode: true,
      override_text_color: Some(colors.text),
      widgets,
      selection: Selection {
         bg_fill: colors.bg3, // affects selected text color, combox selected item bg
         stroke: Stroke::new(1.0, colors.highlight), // also affects TextEdit border color when active
      },
      hyperlink_color: colors.info,
      faint_bg_color: colors.bg,
      extreme_bg_color: colors.bg2,
      code_bg_color: colors.bg,
      warn_fg_color: colors.warning,
      error_fg_color: colors.error,
      window_corner_radius: CornerRadius::same(6),
      window_shadow: Shadow {
         offset: (0, 0).into(),
         blur: 12,
         spread: 1,
         color: Color32::from_rgba_premultiplied(140, 140, 140, 255),
      },
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
   non_interactive_base.bg_stroke.width = 0.0;

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
