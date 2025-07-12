use super::Label;
use egui::{
   AboveOrBelow, Align2, Id, InnerResponse, NumExt, Painter, Popup, PopupCloseBehavior, Rect,
   Response, ScrollArea, Sense, Stroke, TextWrapMode, Ui, Vec2, WidgetText,
   epaint::{RectShape, Shape, StrokeKind},
   style::WidgetVisuals,
};

#[must_use = "You should call .show_ui()"]
pub struct ComboBoxWithImage {
   id_salt: Id,
   label: Option<WidgetText>,
   selected_item: Label,
   width: Option<f32>,
   popup_max_height: Option<f32>,
   icon: Option<Box<dyn FnOnce(&Ui, Rect, &WidgetVisuals, bool, AboveOrBelow)>>,
   wrap_mode: Option<TextWrapMode>,
   close_behavior: Option<PopupCloseBehavior>,
}

impl ComboBoxWithImage {
   pub fn new(id_salt: impl std::hash::Hash, selected_item: Label) -> Self {
      Self {
         id_salt: Id::new(id_salt),
         label: None,
         selected_item,
         width: None,
         popup_max_height: None,
         icon: None,
         wrap_mode: None,
         close_behavior: None,
      }
   }

   pub fn label(mut self, label: impl Into<WidgetText>) -> Self {
      self.label = Some(label.into());
      self
   }

   /// Set the exact width of the combo box button.
   /// If not set, the width adapts to the content, icon, and minimum width.
   pub fn width(mut self, width: f32) -> Self {
      self.width = Some(width);
      self
   }

   /// Set the maximum height of the popup menu.
   /// Default is `ui.spacing().combo_height`.
   pub fn popup_max_height(mut self, height: f32) -> Self {
      self.popup_max_height = Some(height);
      self
   }

   pub fn icon(
      mut self,
      icon_fn: impl FnOnce(&Ui, Rect, &WidgetVisuals, bool, AboveOrBelow) + 'static,
   ) -> Self {
      self.icon = Some(Box::new(icon_fn));
      self
   }

   /// Set the wrap mode for the selected text displayed *in the button*.
   pub fn wrap_mode(mut self, wrap_mode: TextWrapMode) -> Self {
      self.wrap_mode = Some(wrap_mode);
      self
   }

   pub fn close_behavior(mut self, close_behavior: PopupCloseBehavior) -> Self {
      self.close_behavior = Some(close_behavior);
      self
   }

   pub fn show_ui<R>(
      self,
      ui: &mut Ui,
      menu_contents: impl FnOnce(&mut Ui) -> R,
   ) -> Option<InnerResponse<R>> {
      let button_id = ui.make_persistent_id(self.id_salt);
      let popup_id = button_id.with("popup");

      let is_popup_open = Popup::is_id_open(ui.ctx(), popup_id);

      // Button Rendering
      let button_response = combo_box_with_image_button(
         ui,
         button_id,
         is_popup_open,
         &self.selected_item,
         self.icon,
         self.wrap_mode,
         (self.width, None),
      );

      // Interaction
      if button_response.clicked() {
         Popup::toggle_id(ui.ctx(), popup_id);
      }

      // Popup Handling
      let popup_default_height = 200.0;
      let popup_current_height = ui.memory(|m| {
         m.area_rect(popup_id)
            .map(|rect| rect.height())
            .unwrap_or(popup_default_height)
      });

      let button_bottom = button_response.rect.bottom();
      let screen_bottom = ui.ctx().screen_rect().bottom();

      let space_below = screen_bottom - button_bottom;

      let _above_or_below = if space_below >= popup_current_height
         || space_below >= ui.spacing().interact_size.y * 4.0
      {
         AboveOrBelow::Below
      } else {
         AboveOrBelow::Above
      };

      let popup_max_h = self
         .popup_max_height
         .unwrap_or_else(|| ui.spacing().combo_height);

      let popup_max_w = self.width.unwrap_or(ui.available_width());

      let close_behavior = self
         .close_behavior
         .unwrap_or(PopupCloseBehavior::CloseOnClick);

      let popup = Popup::menu(&button_response).close_behavior(close_behavior);
      let inner = popup.show(|ui| {
         ScrollArea::vertical()
            .max_height(popup_max_h)
            .max_width(popup_max_w)
            .show(ui, |ui| {
               ui.set_width(
                  ui.available_width()
                     .max(button_response.rect.width() - ui.spacing().button_padding.x * 2.0),
               );
               ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
               menu_contents(ui)
            })
            .inner
      });

      inner
   }
}

fn combo_box_with_image_button(
   ui: &mut Ui,
   _id: Id,
   is_popup_open: bool,
   selected_item: &Label,
   icon_painter: Option<Box<dyn FnOnce(&Ui, Rect, &WidgetVisuals, bool, AboveOrBelow)>>,
   wrap_mode_override: Option<TextWrapMode>,
   (width_override, _): (Option<f32>, Option<f32>),
) -> Response {
   let button_padding = ui.spacing().button_padding;
   let icon_width = ui.spacing().icon_width;
   let icon_spacing = ui.spacing().icon_spacing;
   let minimum_height = ui.spacing().interact_size.y;

   let wrap_mode = wrap_mode_override.unwrap_or_else(|| ui.wrap_mode());

   // Size Calculation
   let available_width = ui.available_width();
   let width_for_layout = if let Some(w) = width_override {
      (w - button_padding.x * 2.0 - icon_width - icon_spacing).max(0.0)
   } else {
      (available_width - button_padding.x * 2.0 - icon_width - icon_spacing).max(10.0)
   };

   let mut item_for_measurement = selected_item.clone();
   if wrap_mode_override.is_some() {
      item_for_measurement = item_for_measurement.wrap_mode(wrap_mode);
   }

   let (_, content_size) = item_for_measurement.galley_and_size(ui, width_for_layout);

   // Calculate the total inner size needed (content + icon)
   let inner_width = content_size.x + icon_spacing + icon_width;
   let inner_height = content_size.y.max(icon_width);

   let mut button_size = Vec2::new(
      inner_width + button_padding.x * 2.0,
      inner_height + button_padding.y * 2.0,
   );

   button_size.y = button_size.y.at_least(minimum_height);
   if let Some(w) = width_override {
      button_size.x = w;
   } else {
      button_size.x = button_size.x.at_least(ui.spacing().combo_width);
   }

   // Allocation & Interaction
   let (rect, response) = ui.allocate_exact_size(button_size, Sense::click());

   // Painting
   if ui.is_rect_visible(rect) {
      let visuals = if is_popup_open {
         ui.visuals().widgets.open
      } else {
         ui.style().interact(&response).clone()
      };

      // Paint background
      let background_rect = rect.expand(visuals.expansion);
      ui.painter().add(RectShape::new(
         background_rect,
         visuals.corner_radius,
         visuals.weak_bg_fill,
         visuals.bg_stroke,
         StrokeKind::Inside,
      ));

      // Area for content (label + image) inside padding
      let content_total_rect = rect.shrink2(button_padding);

      let icon_rect = Align2::RIGHT_CENTER.align_size_within_rect(
         Vec2::splat(icon_width), // Square icon
         content_total_rect,
      );

      // Calculate rect for the LabelWithImage (remaining space to the left of the icon)
      let label_rect_width = (icon_rect.left() - content_total_rect.left() - icon_spacing).max(0.0);
      let label_rect = Rect::from_min_size(
         content_total_rect.min,
         Vec2::new(label_rect_width, content_total_rect.height()),
      );

      selected_item.paint_content_within_rect(ui, label_rect, &visuals);

      let popup_peek_height = 50.0;
      let above_or_below =
         if response.rect.bottom() + popup_peek_height < ui.ctx().screen_rect().bottom() {
            AboveOrBelow::Below
         } else {
            AboveOrBelow::Above
         };

      // Paint the icon
      if let Some(icon_painter) = icon_painter {
         icon_painter(
            ui,
            icon_rect,
            &visuals,
            is_popup_open,
            above_or_below,
         );
      } else {
         paint_default_icon(ui.painter(), icon_rect, &visuals, above_or_below);
      }
   }

   response
}

fn paint_default_icon(
   painter: &Painter,
   rect: Rect,
   visuals: &WidgetVisuals,
   above_or_below: AboveOrBelow,
) {
   let rect = Rect::from_center_size(
      rect.center(),
      Vec2::new(rect.width() * 0.7, rect.height() * 0.45),
   );

   let points = match above_or_below {
      AboveOrBelow::Above => vec![rect.left_bottom(), rect.right_bottom(), rect.center_top()],
      AboveOrBelow::Below => vec![rect.left_top(), rect.right_top(), rect.center_bottom()],
   };

   painter.add(Shape::convex_polygon(
      points,
      visuals.fg_stroke.color,
      Stroke::NONE,
   ));
}
