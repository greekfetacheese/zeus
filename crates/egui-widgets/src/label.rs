use egui::{
   Align, Color32, FontSelection, Image, Pos2, Rect, Response, Sense, Stroke, StrokeKind,
   TextWrapMode, Ui, Vec2, Widget, WidgetText,
   epaint::{RectShape, TextShape},
   style::WidgetVisuals,
   text::LayoutJob,
};
use std::sync::Arc;

#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
#[derive(Clone)]
pub struct Label {
   text: WidgetText,
   pub(crate) image: Option<Image<'static>>,
   spacing: f32,
   expansion: Option<f32>,
   pub(crate) sense: Option<Sense>,
   wrap_mode: Option<TextWrapMode>,
   selectable: Option<bool>,
   text_first: bool,
   selected: bool,
   fill_width: bool,
}
impl Label {
   /// Create a new `LabelWithImage` with text and an optional image.
   /// By default the image is shown after the text
   pub fn new(text: impl Into<WidgetText>, image: Option<Image<'static>>) -> Self {
      Self {
         text: text.into(),
         image,
         spacing: 6.0,
         expansion: None,
         sense: None,
         wrap_mode: None,
         selectable: None,
         text_first: true,
         selected: false,
         fill_width: false,
      }
   }

   /// Set the space between the text and the image.
   pub fn spacing(mut self, spacing: f32) -> Self {
      self.spacing = spacing;
      self
   }

   pub fn expand(mut self, expansion: Option<f32>) -> Self {
      self.expansion = expansion;
      self
   }

   /// Make the label respond to clicks and/or drags.
   /// This will also turn the `selectable` to false
   pub fn sense(mut self, sense: Sense) -> Self {
      self.sense = Some(sense);
      self.selectable = Some(false);
      self
   }

   pub fn wrap_mode(mut self, wrap_mode: TextWrapMode) -> Self {
      self.wrap_mode = Some(wrap_mode);
      self
   }

   pub fn wrap(mut self) -> Self {
      self.wrap_mode = Some(TextWrapMode::Wrap);
      self
   }

   /// Set whether the text can be selected with the mouse.
   pub fn selectable(mut self, selectable: bool) -> Self {
      self.selectable = Some(selectable);
      self
   }

   /// Show the image first and then the text
   pub fn image_on_left(mut self) -> Self {
      self.text_first = false;
      self
   }

   pub fn selected(mut self, selected: bool) -> Self {
      self.selected = selected;
      self
   }

   pub fn fill_width(mut self, fill: bool) -> Self {
      self.fill_width = fill;
      self
   }

   /// Calculate the size needed by the widget.
   ///
   /// `available_width` is the width available *for the text part* after accounting for image/spacing.
   pub fn galley_and_size(
      &self,
      ui: &Ui,
      available_width_for_text: f32,
   ) -> (Arc<egui::Galley>, Vec2) {
      let layout_job = self.prepare_layout_job(ui, available_width_for_text);
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
      (galley, Vec2::new(total_width, total_height))
   }

   fn prepare_layout_job(&self, ui: &Ui, wrap_width: f32) -> LayoutJob {
      let wrap_mode = self.wrap_mode.unwrap_or_else(|| ui.wrap_mode());
      let layout_job = self.text.clone().into_layout_job(
         ui.style(),
         FontSelection::Default,
         ui.text_valign(),
      );

      // remove the Arc
      let mut layout_job: LayoutJob = (*layout_job).clone();
      match wrap_mode {
         TextWrapMode::Extend => {
            layout_job.wrap.max_width = f32::INFINITY;
         }
         TextWrapMode::Wrap => {
            layout_job.wrap.max_width = wrap_width;
         }
         TextWrapMode::Truncate => {
            layout_job.wrap.max_width = wrap_width;
            layout_job.wrap.max_rows = 1;
            layout_job.wrap.break_anywhere = true;
         }
      }

      layout_job.halign = Align::LEFT;
      layout_job
   }
   pub(crate) fn paint_content_within_rect(
      &self,
      ui: &mut Ui,
      rect: Rect,
      button_visuals: &WidgetVisuals,
   ) {
      // Estimate available width for text layout within the provided rect
      let available_width_for_text = if self.image.is_some() {
         (rect.width()
            - self.image.as_ref().map_or(0.0, |img| img.size().map_or(0.0, |s| s.x))
            - self.spacing)
            .max(0.0)
      } else {
         rect.width()
      };

      // Calculate galley based on available width
      let (galley, _) = self.galley_and_size(ui, available_width_for_text);
      if ui.is_rect_visible(rect) {
         let (text_pos, image_rect_opt) = layout_content_within_rect(
            ui,
            rect,
            &galley,
            &self.image,
            self.spacing,
            self.text_first,
         );

         let text_color = button_visuals.text_color();
         ui.painter().add(TextShape::new(
            text_pos,
            galley.clone(),
            text_color,
         ));

         if let Some(image_rect) = image_rect_opt {
            if let Some(image) = &self.image {
               image.paint_at(ui, image_rect);
            }
         }
      }
   }
}
impl Widget for Label {
   fn ui(self, ui: &mut Ui) -> Response {
      // Calculate Size (Content Only)
      let image_size = if let Some(image) = &self.image {
         image.calc_size(ui.available_size(), image.size())
      } else {
         Vec2::ZERO
      };

      let available_width_for_text = if self.fill_width {
         if self.image.is_some() {
            (ui.available_width() - self.spacing - image_size.x).max(10.0)
         } else {
            ui.available_width()
         }
      } else {
         f32::INFINITY
      };

      let (galley, content_size) = self.galley_and_size(ui, available_width_for_text);
      let desired_size = if self.fill_width {
         Vec2::new(ui.available_width(), content_size.y)
      } else {
         content_size
      };

      // Allocate Space (Content Size Only)
      let sense = self.sense.unwrap_or(Sense::hover());
      let (rect, response) = ui.allocate_exact_size(desired_size, sense);

      // Paint
      if ui.is_rect_visible(rect) {
         let mut visuals = ui.style().interact_selectable(&response, self.selected);

         if self.selected {
            visuals.weak_bg_fill = visuals.bg_fill;
         }

         let fill = if self.selected {
            visuals.bg_fill
         } else if response.hovered() || response.has_focus() {
            visuals.weak_bg_fill
         } else {
            Color32::TRANSPARENT
         };

         let stroke = if self.selected || response.hovered() || response.has_focus() {
            visuals.bg_stroke
         } else {
            Stroke::NONE
         };

         let expansion = self.expansion.unwrap_or(visuals.expansion);
         let background_rect = rect.expand(expansion);
         ui.painter().add(RectShape::new(
            background_rect,
            visuals.corner_radius,
            fill,
            stroke,
            StrokeKind::Inside,
         ));

         // Layout and Paint Content
         let (text_pos, image_rect_opt) = layout_content_within_rect(
            ui,
            rect,
            &galley,
            &self.image,
            self.spacing,
            self.text_first,
         );

         let text_color = visuals.text_color();
         ui.painter().add(TextShape::new(
            text_pos,
            galley.clone(),
            text_color,
         ));

         if let Some(image_rect) = image_rect_opt {
            if let Some(image) = self.image {
               image.paint_at(ui, image_rect);
            }
         }
      }
      response
   }
}
fn layout_content_within_rect(
   ui: &Ui,
   rect: Rect,
   galley: &egui::Galley,
   image: &Option<Image<'static>>,
   spacing: f32,
   text_first: bool,
) -> (Pos2, Option<Rect>) {
   let text_size = galley.size();
   let image_size = if let Some(image) = image {
      image.calc_size(ui.available_size(), image.size())
   } else {
      Vec2::ZERO
   };

   let total_content_height = text_size.y.max(image_size.y);
   let top_y = ui
      .layout()
      .align_size_within_rect(Vec2::new(0.0, total_content_height), rect)
      .min
      .y;

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

   let text_pos = Pos2::new(
      text_start_x,
      top_y + (total_content_height - text_size.y) * 0.5,
   );
   (text_pos, image_final_rect)
}
