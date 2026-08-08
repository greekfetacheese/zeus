use egui::{Color32, CornerRadius, Response, Shadow, Stroke};

pub type LabelVisuals = ButtonVisuals;

/// Visuals for a button
#[derive(Copy, Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ButtonVisuals {
   pub text: Color32,
   pub bg: Color32,
   pub bg_hover: Color32,
   pub bg_click: Color32,
   pub bg_selected: Color32,
   pub border: Stroke,
   pub border_hover: Stroke,
   pub border_click: Stroke,
   pub corner_radius: CornerRadius,
   pub shadow: Shadow,
}

impl PartialEq for ButtonVisuals {
   fn eq(&self, other: &Self) -> bool {
      self.text == other.text
         && self.bg == other.bg
         && self.bg_hover == other.bg_hover
         && self.bg_click == other.bg_click
         && self.bg_selected == other.bg_selected
         && self.border == other.border
         && self.border_hover == other.border_hover
         && self.border_click == other.border_click
         && self.corner_radius == other.corner_radius
         && self.shadow == other.shadow
   }
}

impl Eq for ButtonVisuals {}

impl ButtonVisuals {
   pub fn bg_from_res(&self, res: &Response) -> Color32 {
      if res.is_pointer_button_down_on() || res.has_focus() || res.clicked() {
         self.bg_click
      } else if res.hovered() || res.highlighted() {
         self.bg_hover
      } else {
         self.bg
      }
   }

   pub fn border_from_res(&self, res: &Response) -> Stroke {
      if res.is_pointer_button_down_on() || res.has_focus() || res.clicked() {
         self.border_click
      } else if res.hovered() || res.highlighted() {
         self.border_hover
      } else {
         self.border
      }
   }
}

/// Visuals for a TextEdit
#[derive(Copy, Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TextEditVisuals {
   pub text: Color32,
   pub bg: Color32,
   pub border: Stroke,
   pub border_hover: Stroke,
   pub border_open: Stroke,
   pub corner_radius: CornerRadius,
   pub shadow: Shadow,
}

impl PartialEq for TextEditVisuals {
   fn eq(&self, other: &Self) -> bool {
      self.text == other.text
         && self.bg == other.bg
         && self.border == other.border
         && self.border_hover == other.border_hover
         && self.border_open == other.border_open
         && self.corner_radius == other.corner_radius
         && self.shadow == other.shadow
   }
}

impl Eq for TextEditVisuals {}

impl TextEditVisuals {
   pub fn border_from_res(&self, res: &Response) -> Stroke {
      if res.is_pointer_button_down_on() || res.has_focus() || res.clicked() {
         self.border_open
      } else if res.hovered() || res.highlighted() {
         self.border_hover
      } else {
         self.border
      }
   }
}

/// Visuals for a ComboBox
#[derive(Copy, Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ComboBoxVisuals {
   pub bg: Color32,
   pub icon: Color32,
   pub bg_hover: Color32,
   pub bg_open: Color32,
   pub border: Stroke,
   pub border_hover: Stroke,
   pub border_open: Stroke,
   pub corner_radius: CornerRadius,
   pub shadow: Shadow,
}

impl PartialEq for ComboBoxVisuals {
   fn eq(&self, other: &Self) -> bool {
      self.bg == other.bg
         && self.icon == other.icon
         && self.bg_hover == other.bg_hover
         && self.bg_open == other.bg_open
         && self.border == other.border
         && self.border_hover == other.border_hover
         && self.border_open == other.border_open
         && self.corner_radius == other.corner_radius
         && self.shadow == other.shadow
   }
}

impl Eq for ComboBoxVisuals {}

impl ComboBoxVisuals {
   pub fn bg_from_res(&self, res: &Response) -> Color32 {
      if res.is_pointer_button_down_on() || res.has_focus() || res.clicked() {
         self.bg_open
      } else if res.hovered() || res.highlighted() {
         self.bg_hover
      } else {
         self.bg
      }
   }

   pub fn border_from_res(&self, res: &Response) -> Stroke {
      if res.is_pointer_button_down_on() || res.has_focus() || res.clicked() {
         self.border_open
      } else if res.hovered() || res.highlighted() {
         self.border_hover
      } else {
         self.border
      }
   }
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameVisuals {
   pub bg_on_hover: Color32,
   pub bg_on_click: Color32,
   pub border_on_hover: (f32, Color32),
   pub border_on_click: (f32, Color32),
}

impl PartialEq for FrameVisuals {
   fn eq(&self, other: &Self) -> bool {
      self.bg_on_hover == other.bg_on_hover
         && self.bg_on_click == other.bg_on_click
         && self.border_on_hover == other.border_on_hover
         && self.border_on_click == other.border_on_click
   }
}

impl Eq for FrameVisuals {}
