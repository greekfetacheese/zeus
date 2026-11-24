//! A row whose words are drawn in one spelling and said in another.

use egui::{
   FontSelection, Image, Rect, Response, RichText, Role, Sense, TextWrapMode, Ui, Vec2, WidgetInfo,
   WidgetText, epaint::TextShape, pos2,
};
use egui_elements::Theme;

/// The gap between two parts of one row, the space a `MultiLabel` leaves by default.
const INTER_SPACING: f32 = 6.0;

/// Draw `parts` as the row's words, and name the row `spoken`.
///
/// The screen and a reader want different spellings of the same figure. A ticker is what a token is
/// known by at a glance and what a reader spells out one letter at a time; the name behind it reads
/// and does not fit the row. One node cannot carry both: a label's glyphs are what its node says,
/// and a name set over them is said as well — the shape that stalled the swap and bridge prompts,
/// where the reader stopped after the first row. So the words are painted here, where they make no
/// node at all, and the node over them says `spoken` alone.
///
/// The row is focusable, which a drawn label never was: `Tab` reaches it, so a keyboard user can
/// walk the rows of a prompt instead of only meeting them in the reader's own order.
///
/// `spacing` is the gap between a part's words and its icon, the one the label widget takes.
pub fn spoken_row(
   theme: &Theme,
   ui: &mut Ui,
   spoken: impl Into<String>,
   parts: &[(RichText, Option<Image<'static>>)],
   spacing: f32,
) -> Response {
   let mut drawn = Vec::with_capacity(parts.len());
   let mut size = Vec2::ZERO;

   // Measured the way a label measures itself, so a painted row holds the space a drawn one held.
   for (index, (text, icon)) in parts.iter().enumerate() {
      let galley = WidgetText::from(text.clone()).into_galley(
         ui,
         Some(TextWrapMode::Extend),
         f32::INFINITY,
         FontSelection::Default,
      );
      let image = icon
         .as_ref()
         .map(|icon| icon.calc_size(ui.available_size(), icon.size()))
         .unwrap_or(Vec2::ZERO);
      let width = galley.size().x
         + match icon {
            Some(_) => spacing + image.x,
            None => 0.0,
         };
      let part = Vec2::new(width, galley.size().y.max(image.y));

      if index > 0 {
         size.x += INTER_SPACING;
      }

      size.x += part.x;
      size.y = size.y.max(part.y);
      drawn.push((galley, icon.clone(), image, part));
   }

   let (rect, response) = ui.allocate_exact_size(size, Sense::focusable_noninteractive());

   if ui.is_rect_visible(rect) {
      let color = theme.label_visuals().text;
      let mut x = rect.left();

      for (galley, icon, image, part) in drawn {
         let top = rect.top() + (size.y - part.y) * 0.5;
         let text = galley.size();

         ui.painter().add(TextShape::new(
            pos2(x, top + (part.y - text.y) * 0.5),
            galley,
            color,
         ));

         if let Some(icon) = icon {
            let at = pos2(
               x + text.x + spacing,
               top + (part.y - image.y) * 0.5,
            );

            icon.paint_at(ui, Rect::from_min_size(at, image));
         }

         x += part.x + INTER_SPACING;
      }
   }

   // The row is one node: the sentence, with nothing drawn under it. It is named here rather than
   // through `name_text` because a widget allocated with a sense has a node of egui's own already,
   // and that helper only writes a name into a node it finds — the role and the value a reader
   // meets first are set with the rest of the widget's information.
   let spoken = spoken.into();

   response.widget_info(|| WidgetInfo::labeled(Role::Label, ui.is_enabled(), spoken.clone()));
   ui.ctx().accesskit_node_builder(response.id, |node| {
      node.set_value(spoken.clone());
      node.set_label(spoken.as_str());
   });

   response
}
