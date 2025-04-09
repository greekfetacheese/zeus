use egui::{
   Align, FontSelection, Image, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextWrapMode, Ui, Vec2, Widget,
   WidgetText,
   epaint::{RectShape, TextShape},
   style::WidgetVisuals,
   text::LayoutJob,
};
use std::sync::Arc;

#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
#[derive(Clone)]
pub struct LabelWithImage {
   text: WidgetText,
   image: Option<Image<'static>>,
   spacing: f32,
   sense: Option<Sense>,
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
         sense: None,
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
   /// This will also turn the `selectable` to false
   pub fn sense(mut self, sense: Sense) -> Self {
      self.sense = Some(sense);
      self.selectable = Some(false);
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

   /// Calculate the size needed by the widget.
   /// `available_width` is the width available *for the text part* after accounting for image/spacing.
   pub fn galley_and_size(&self, ui: &Ui, available_width_for_text: f32) -> (Arc<egui::Galley>, Vec2) {
      let layout_job = self.prepare_layout_job(ui, available_width_for_text);
      let galley = ui.fonts(|fonts| fonts.layout_job(layout_job.clone())); // Use the prepared job

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

      (galley, Vec2::new(total_width, total_height))
   }

   /// Prepares the layout job for the text part.
   fn prepare_layout_job(&self, ui: &Ui, wrap_width: f32) -> LayoutJob {
      let wrap_mode = self.wrap_mode.unwrap_or_else(|| ui.wrap_mode());
      let mut layout_job = self
         .text
         .clone()
         .into_layout_job(ui.style(), FontSelection::Default, ui.text_valign()); // Use Default selection for size calc

      match wrap_mode {
         TextWrapMode::Extend => {
            layout_job.wrap.max_width = f32::INFINITY;
         }
         TextWrapMode::Wrap => {
            layout_job.wrap.max_width = wrap_width;
            // layout_job.wrap.max_rows = usize::MAX; // Default is MAX
            // layout_job.wrap.break_anywhere = false; // Default is false
         }
         TextWrapMode::Truncate => {
            layout_job.wrap.max_width = wrap_width;
            layout_job.wrap.max_rows = 1;
            layout_job.wrap.break_anywhere = true;
         }
      }

      layout_job.halign = Align::LEFT; // Usually align left within its box
      layout_job
   }

   // This function is now only used internally by combo_box_with_image_button
   // to paint the *content* inside the button frame. It does NOT paint background.
   pub(crate) fn paint_content_within_rect(
      &self,
      ui: &mut Ui,
      rect: Rect,
      button_visuals: &WidgetVisuals, // Use the visuals passed by the button
   ) {
      // Estimate available width for text layout within the provided rect
      let available_width_for_text = if self.image.is_some() {
         (rect.width()
            - self
               .image
               .as_ref()
               .map_or(0.0, |img| img.size().map_or(0.0, |s| s.x))
            - self.spacing)
            .max(0.0)
      } else {
         rect.width()
      };

      // Calculate galley based on available width
      let (galley, _) = self.galley_and_size(ui, available_width_for_text);

      if ui.is_rect_visible(rect) {
         // Use the helper to get positions
         let (text_pos, image_rect_opt) = layout_content_within_rect(
            ui,
            rect,
            &galley,
            &self.image,
            self.spacing,
            self.text_first,
         );

         // Paint text using the button's visuals' text color
         let text_color = button_visuals.text_color();
         ui.painter()
            .add(TextShape::new(text_pos, galley.clone(), text_color));

         // Paint the image
         if let Some(image_rect) = image_rect_opt {
            if let Some(image) = &self.image {
               image.paint_at(ui, image_rect);
            }
         }
      }
   }
}

impl Widget for LabelWithImage {
   fn ui(self, ui: &mut Ui) -> Response {
      // --- 1. Calculate Size ---
      // Use available width for the initial layout calculation
      let available_width_for_text = if self.image.is_some() {
         (ui.available_width()
            - self
               .image
               .as_ref()
               .map_or(0.0, |img| img.size().map_or(0.0, |s| s.x))
            - self.spacing)
            .max(10.0)
      } else {
         ui.available_width()
      };
      let (galley, desired_size) = self.galley_and_size(ui, available_width_for_text);

      // --- 2. Allocate Space ---
      // Allocate the space needed for the whole widget (including potential background)
      // Add padding if you want space around the hover/click highlight
      let padding = ui.style().spacing.button_padding; // Use button padding as a reasonable default
      let padded_size = desired_size + padding * 2.0;
      let sense = self.sense.unwrap_or(Sense::hover());
      let (rect, response) = ui.allocate_exact_size(padded_size, sense);

      // --- 3. Paint ---
      if ui.is_rect_visible(rect) {
         // Get visuals based on interaction state
         let visuals = ui.style().interact(&response);

         // --- 3a. Paint Background ---
         // Paint background highlight if hovered or active
         if self.sense.is_some() {
            if response.hovered() || response.is_pointer_button_down_on() || response.has_focus() {
               let background_rect = rect.expand(visuals.expansion);
               ui.painter().add(RectShape::new(
                  background_rect,
                  visuals.corner_radius,
                  visuals.weak_bg_fill,
                  visuals.bg_stroke,
                  StrokeKind::Inside,
               ));
            }
         }

         // --- 3b. Layout and Paint Content ---
         // Calculate content rect inside the padding
         let content_rect = rect.shrink2(padding);

         // Use the helper to get content positions within the inner content_rect
         let (text_pos, image_rect_opt) = layout_content_within_rect(
            ui,
            content_rect,
            &galley,
            &self.image,
            self.spacing,
            self.text_first,
         );

         // Paint text using the interaction visuals' text color
         let text_color = visuals.text_color();
         let underline = if response.has_focus() {
            Stroke::new(1.0, text_color)
         } else {
            Stroke::NONE
         };
         ui.painter()
            .add(TextShape::new(text_pos, galley.clone(), text_color).with_underline(underline));

         // Paint the image
         if let Some(image_rect) = image_rect_opt {
            if let Some(image) = self.image {
               image.paint_at(ui, image_rect);
            }
         }
      }

      response
   }
}

// Helper function to layout text and image within a given rectangle
// Returns (text_position, optional_image_rect)
fn layout_content_within_rect(
   ui: &Ui, // Needed for image size calculation context
   rect: Rect,
   galley: &egui::Galley,
   image: &Option<Image<'static>>, // Pass image ref
   spacing: f32,
   text_first: bool,
) -> (Pos2, Option<Rect>) {
   let text_size = galley.size();
   let image_size = if let Some(image) = image {
      image.calc_size(ui.available_size(), image.size())
   } else {
      Vec2::ZERO
   };

   // Center the combined content vertically within the allocated `rect`
   let total_content_height = text_size.y.max(image_size.y);
   let top_y = rect.top() + (rect.height() - total_content_height) * 0.5;

   let (text_start_x, image_final_rect) = if text_first {
      // Text first, image second
      let text_start_x = rect.left();
      let image_start_x = text_start_x + text_size.x + spacing;
      let image_final_rect = image.as_ref().map(|_| {
         let image_pos = Pos2::new(
            image_start_x,
            top_y + (total_content_height - image_size.y) * 0.5,
         );
         Rect::from_min_size(image_pos, image_size)
      });
      (text_start_x, image_final_rect)
   } else {
      // Image first, text second
      let image_start_x = rect.left();
      let text_start_x = image_start_x + image_size.x + spacing;
      let image_final_rect = image.as_ref().map(|_| {
         let image_pos = Pos2::new(
            image_start_x,
            top_y + (total_content_height - image_size.y) * 0.5,
         );
         Rect::from_min_size(image_pos, image_size)
      });
      (text_start_x, image_final_rect)
   };

   // Calculate final text baseline position
   let text_pos = Pos2::new(
      text_start_x,
      top_y + (total_content_height - text_size.y) * 0.5,
   );

   (text_pos, image_final_rect)
}
