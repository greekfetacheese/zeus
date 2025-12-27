use egui::{Color32, CornerRadius, Response, Shadow, Stroke};

pub type LabelVisuals = ButtonVisuals;

/// Visuals for a button
#[derive(Copy, Clone, Debug)]
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
#[derive(Copy, Clone, Debug)]
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
#[derive(Copy, Clone, Debug)]
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
#[derive(Clone, Copy, Debug)]
pub struct FrameVisuals {
   pub bg_on_hover: Color32,
   pub bg_on_click: Color32,
   pub border_on_hover: (f32, Color32),
   pub border_on_click: (f32, Color32),
}