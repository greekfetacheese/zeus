use super::Label;
use egui::{Pos2, Rect, Response, Sense, StrokeKind, Ui, Vec2, Widget, epaint::RectShape};

/// A widget that displays multiple [`Label`]s in a single horizontal line,
/// with consistent vertical alignment (centered) and optional spacing between them.
#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
#[derive(Clone)]
pub struct MultiLabel {
   labels: Vec<Label>,
   inter_spacing: f32,
   sense: Option<Sense>,
}

impl MultiLabel {
   /// Create a new `MultiLabel` from a vec of [`Label`].
   /// The default spacing between labels is 6.0.
   pub fn new(labels: Vec<Label>) -> Self {
      Self {
         labels,
         inter_spacing: 6.0,
         sense: None,
      }
   }

   /// Set the space between each pair of labels.
   pub fn inter_spacing(mut self, spacing: f32) -> Self {
      self.inter_spacing = spacing;
      self
   }

   /// Make the entire multi-label respond to clicks and/or drags.
   /// This will use the same visuals for all inner content.
   pub fn sense(mut self, sense: Sense) -> Self {
      self.sense = Some(sense);
      self
   }
}

impl Widget for MultiLabel {
   fn ui(self, ui: &mut Ui) -> Response {
      if self.labels.is_empty() {
         return ui.allocate_response(Vec2::ZERO, Sense::empty());
      }

      // Calculate Intrinsic Sizes (no wrapping)
      let mut total_width = 0.0;
      let mut max_height: f32 = 0.0;
      let mut label_sizes: Vec<Vec2> = Vec::with_capacity(self.labels.len());

      for label in &self.labels {
         let (_, size) = label.galley_and_size(ui, f32::INFINITY);
         label_sizes.push(size);
         total_width += size.x;
         max_height = max_height.max(size.y);
      }

      let num_gaps = (self.labels.len() as f32).max(0.0) - 1.0;
      total_width += self.inter_spacing * num_gaps;
      let desired_size = Vec2::new(total_width, max_height);

      // Allocate Space
      let sense = self.sense.unwrap_or(Sense::empty());
      let (rect, response) = ui.allocate_exact_size(desired_size, sense);

      // Paint
      if ui.is_rect_visible(rect) {
         let visuals = if self.sense.is_some() {
            ui.style().interact(&response).clone()
         } else {
            ui.style().noninteractive().clone()
         };

         // Paint Background
         let is_interactive = self.sense.is_some();
         if is_interactive
            && (response.hovered() || response.is_pointer_button_down_on() || response.has_focus())
         {
            let background_rect = rect.expand(visuals.expansion);
            ui.painter().add(RectShape::new(
               background_rect,
               visuals.corner_radius,
               visuals.weak_bg_fill,
               visuals.bg_stroke,
               StrokeKind::Inside,
            ));
         }

         // Layout and Paint Each Label
         let mut x = rect.left();
         let mut i = 0;

         for label in self.labels {
            if i > 0 {
               x += self.inter_spacing;
            }

            let label_size = label_sizes[i];
            let y_offset = (max_height - label_size.y) * 0.5;
            let label_rect = Rect::from_min_size(Pos2::new(x, rect.top() + y_offset), label_size);

            label.paint_content_within_rect(ui, label_rect, &visuals);

            x += label_size.x;
            i += 1;
         }
      }
      response
   }
}
