use egui::{
    Align, FontSelection, Image, Pos2, Rect, Response, Sense, Stroke, TextWrapMode, Ui, Vec2, Widget, WidgetText,
    text_selection::LabelSelectionState,
 };
 use std::sync::Arc;



 #[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct LabelWithImage {
   text: WidgetText,
   image: Option<Image<'static>>,
   spacing: f32,
   sense: Sense,
   wrap_mode: Option<TextWrapMode>,
   selectable: Option<bool>,
   text_first: bool,
}

impl LabelWithImage {
   /// Create a new `LabelWithImage` with text and an optional image.
   /// By default, text appears before the image.
   pub fn new(text: impl Into<WidgetText>, image: Option<Image<'static>>) -> Self {
      Self {
         text: text.into(),
         image,
         spacing: 6.0,
         sense: Sense::hover(),
         wrap_mode: None,
         selectable: None,
         text_first: true,
      }
   }

   /// Set the spacing between the text and the image.
   pub fn spacing(mut self, spacing: f32) -> Self {
      self.spacing = spacing;
      self
   }

   /// Make the label respond to clicks and/or drags.
   pub fn sense(mut self, sense: Sense) -> Self {
      self.sense = sense;
      self
   }

   /// Set the wrap mode for the text (e.g., Wrap, Truncate, Extend).
   pub fn wrap_mode(mut self, wrap_mode: TextWrapMode) -> Self {
      self.wrap_mode = Some(wrap_mode);
      self
   }

   /// Set `wrap_mode` to `TextWrapMode::Wrap`.
   pub fn wrap(mut self) -> Self {
      self.wrap_mode = Some(TextWrapMode::Wrap);
      self
   }

   /// Set whether the text can be selected with the mouse.
   pub fn selectable(mut self, selectable: bool) -> Self {
      self.selectable = Some(selectable);
      self
   }

   /// Set whether the text appears before (true) or after (false) the image.
   pub fn text_first(mut self, text_first: bool) -> Self {
      self.text_first = text_first;
      self
   }

   /// Layout and position the text and optional image in the UI.
   fn layout_in_ui(&self, ui: &mut Ui) -> (Pos2, Arc<egui::Galley>, Option<Rect>, Response) {
      // Determine if text is selectable
      let selectable = self
         .selectable
         .unwrap_or_else(|| ui.style().interaction.selectable_labels);
      let mut sense = self.sense;
      if selectable {
         let allow_drag_to_select = ui.input(|i| !i.has_touch_screen());
         let mut select_sense = if allow_drag_to_select {
            Sense::click_and_drag()
         } else {
            Sense::click()
         };
         select_sense -= Sense::FOCUSABLE;
         sense = sense.union(select_sense);
      }

      // Create the layout job with wrap_mode
      let wrap_mode = self.wrap_mode.unwrap_or_else(|| ui.wrap_mode());
      let available_width = ui.available_width();
      let mut layout_job = self
         .text
         .clone()
         .into_layout_job(ui.style(), FontSelection::Default, ui.text_valign());

      match wrap_mode {
         TextWrapMode::Extend => {
            layout_job.wrap.max_width = f32::INFINITY;
         }
         TextWrapMode::Wrap => {
            layout_job.wrap.max_width = available_width;
         }
         TextWrapMode::Truncate => {
            layout_job.wrap.max_width = available_width;
            layout_job.wrap.max_rows = 1;
            layout_job.wrap.break_anywhere = true;
         }
      }

      layout_job.halign = Align::LEFT;

      let galley = ui.fonts(|fonts| fonts.layout_job(layout_job));
      let text_size = galley.size();
      let image_size = if let Some(image) = &self.image {
         image.calc_size(ui.available_size(), image.size())
      } else {
         Vec2::ZERO
      };

      let total_width = text_size.x
         + if self.image.is_some() {
            self.spacing + image_size.x
         } else {
            0.0
         };
      let total_height = text_size.y.max(image_size.y);

      // Allocate space for the combined widget
      let (rect, response) = ui.allocate_exact_size(Vec2::new(total_width, total_height), sense);

      // Determine positions based on text_first
      let (text_pos, image_rect) = if self.text_first {
         // Text first, image second
         let text_pos = rect.left_top() + Vec2::new(0.0, (total_height - text_size.y) / 2.0);
         let image_rect = self.image.as_ref().map(|_| {
            let image_pos = Pos2::new(
               rect.min.x + text_size.x + self.spacing,
               rect.min.y + (total_height - image_size.y) / 2.0,
            );
            Rect::from_min_size(image_pos, image_size)
         });
         (text_pos, image_rect)
      } else {
         // Image first, text second
         let image_rect = self.image.as_ref().map(|_| {
            let image_pos = Pos2::new(rect.min.x, rect.min.y + (total_height - image_size.y) / 2.0);
            Rect::from_min_size(image_pos, image_size)
         });
         let text_pos = rect.left_top()
            + Vec2::new(
               if self.image.is_some() {
                  image_size.x + self.spacing
               } else {
                  0.0
               },
               (total_height - text_size.y) / 2.0,
            );
         (text_pos, image_rect)
      };

      (text_pos, galley, image_rect, response)
   }
}

impl Widget for LabelWithImage {
   fn ui(self, ui: &mut Ui) -> Response {
      let image = self.image.clone();
      let selectable = self.selectable;
      let sense = self.sense;
      let (text_pos, galley, image_rect, response) = self.layout_in_ui(ui);

      if ui.is_rect_visible(response.rect) {
         let interactive = sense != Sense::hover();
         let selectable = selectable.unwrap_or_else(|| ui.style().interaction.selectable_labels);

         let response_color = if interactive {
            ui.style().interact(&response).text_color()
         } else {
            ui.style().visuals.text_color()
         };

         let underline = if response.has_focus() || response.highlighted() {
            Stroke::new(1.0, response_color)
         } else {
            Stroke::NONE
         };

         if selectable {
            LabelSelectionState::label_text_selection(
               ui,
               &response,
               text_pos,
               galley.clone(),
               response_color,
               underline,
            );
         } else {
            ui.painter()
               .add(egui::epaint::TextShape::new(text_pos, galley.clone(), response_color).with_underline(underline));
         }

         if let Some(image_rect) = image_rect {
            if let Some(image) = image {
               image.paint_at(ui, image_rect);
            }
         }
      }

      response
   }
}