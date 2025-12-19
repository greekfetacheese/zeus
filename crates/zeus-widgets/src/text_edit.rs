use egui::{
   Align, Align2, Color32, CursorIcon, Event, EventFilter, FontId, FontSelection, Galley, Id,
   ImeEvent, Key, KeyboardShortcut, Margin, Modifiers, NumExt, Response, Sense, Shape,
   TextWrapMode, Ui, Vec2, Widget, WidgetInfo, WidgetText, epaint, output,
   text::{self, LayoutJob},
   text_selection::{self, CCursorRange},
   vec2,
};
use secure_types::{SecureString, Zeroize};
use std::sync::Arc;

#[derive(Clone, Debug, Default)]
pub struct SecureTextEditState {
   pub cursor: text_selection::TextCursorState,
   pub singleline_offset: f32,
   pub last_interaction_time: f64,
   pub ime_enabled: bool,
   pub ime_cursor_range: CCursorRange,
}

impl SecureTextEditState {
   pub fn load(ctx: &egui::Context, id: egui::Id) -> Option<Self> {
      ctx.data_mut(|d| d.get_persisted(id))
   }

   pub fn store(self, ctx: &egui::Context, id: egui::Id) {
      ctx.data_mut(|d| d.insert_persisted(id, self));
   }
}

pub struct SecureTextEditOutput {
   pub response: Response,
   pub state: SecureTextEditState,
   pub cursor_range: Option<CCursorRange>,
}

/// A widget for editing text that is secured by a [`SecureString`].
///
/// This widget is identical to [`egui::TextEdit`], but it uses a [`SecureString`] instead of a [`std::string::String`].
///
/// ## Notes
///
/// - Accessability like screen readers is disabled to avoid multiple unsecure allocations of the entered text.
/// - If you want to make sure the text you enter doesn't stay in memory in any way you have to set [`Self::password`] to `true`.
///
/// Otherwise egui will make copies of that text and some of the copied allocations will stay in memory.
#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct SecureTextEdit<'a> {
   text: &'a mut SecureString,
   hint_text: WidgetText,
   id: Option<Id>,
   id_salt: Option<Id>,
   font_selection: FontSelection,
   text_color: Option<Color32>,
   password: bool,
   frame: bool,
   margin: Margin,
   multiline: bool,
   interactive: bool,
   desired_width: Option<f32>,
   desired_height_rows: usize,
   event_filter: EventFilter,
   cursor_at_end: bool,
   min_size: Vec2,
   align: Align2,
   clip_text: bool,
   char_limit: usize,
   return_key: Option<KeyboardShortcut>,
   background_color: Option<Color32>,
}

impl<'a> SecureTextEdit<'a> {
   pub fn singleline(text: &'a mut SecureString) -> Self {
      Self {
         text,
         hint_text: Default::default(),
         id: None,
         id_salt: None,
         font_selection: FontSelection::default(),
         text_color: None,
         password: false,
         frame: true,
         margin: Margin::symmetric(4, 2),
         multiline: false,
         interactive: true,
         desired_width: None,
         desired_height_rows: 1,
         event_filter: EventFilter {
            horizontal_arrows: true,
            vertical_arrows: true,
            tab: false,
            ..Default::default()
         },
         cursor_at_end: true,
         min_size: Vec2::ZERO,
         align: Align2::LEFT_CENTER,
         clip_text: true,
         char_limit: usize::MAX,
         return_key: Some(KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
         background_color: None,
      }
   }

   pub fn multiline(text: &'a mut SecureString) -> Self {
      Self {
         text,
         hint_text: Default::default(),
         id: None,
         id_salt: None,
         font_selection: FontSelection::default(),
         text_color: None,
         password: false,
         frame: true,
         margin: Margin::symmetric(4, 2),
         multiline: true,
         interactive: true,
         desired_width: None,
         desired_height_rows: 4,
         event_filter: EventFilter {
            horizontal_arrows: true,
            vertical_arrows: true,
            tab: false,
            ..Default::default()
         },
         cursor_at_end: true,
         min_size: Vec2::ZERO,
         align: Align2::LEFT_TOP,
         clip_text: false,
         char_limit: usize::MAX,
         return_key: Some(KeyboardShortcut::new(Modifiers::NONE, Key::Enter)),
         background_color: None,
      }
   }

   pub fn id(mut self, id: Id) -> Self {
      self.id = Some(id);
      self
   }

   pub fn id_source(self, id_source: impl std::hash::Hash) -> Self {
      self.id_salt(id_source)
   }

   pub fn id_salt(mut self, id_salt: impl std::hash::Hash) -> Self {
      self.id_salt = Some(Id::new(id_salt));
      self
   }

   pub fn hint_text(mut self, hint_text: impl Into<WidgetText>) -> Self {
      self.hint_text = hint_text.into();
      self
   }

   pub fn font(mut self, font_selection: impl Into<FontSelection>) -> Self {
      self.font_selection = font_selection.into();
      self
   }

   pub fn text_color(mut self, text_color: Color32) -> Self {
      self.text_color = Some(text_color);
      self
   }

   pub fn text_color_opt(mut self, text_color: Option<Color32>) -> Self {
      self.text_color = text_color;
      self
   }

   pub fn password(mut self, password: bool) -> Self {
      self.password = password;
      self
   }

   pub fn frame(mut self, frame: bool) -> Self {
      self.frame = frame;
      self
   }

   pub fn margin(mut self, margin: impl Into<Margin>) -> Self {
      self.margin = margin.into();
      self
   }

   pub fn interactive(mut self, interactive: bool) -> Self {
      self.interactive = interactive;
      self
   }

   pub fn desired_width(mut self, desired_width: f32) -> Self {
      self.desired_width = Some(desired_width);
      self
   }

   pub fn desired_rows(mut self, desired_height_rows: usize) -> Self {
      self.desired_height_rows = desired_height_rows;
      self
   }

   pub fn lock_focus(mut self, tab_will_indent: bool) -> Self {
      self.event_filter.tab = tab_will_indent;
      self
   }

   pub fn cursor_at_end(mut self, b: bool) -> Self {
      self.cursor_at_end = b;
      self
   }

   pub fn min_size(mut self, min_size: Vec2) -> Self {
      self.min_size = min_size;
      self
   }

   pub fn horizontal_align(mut self, align: Align) -> Self {
      self.align.0[0] = align;
      self
   }

   pub fn vertical_align(mut self, align: Align) -> Self {
      self.align.0[1] = align;
      self
   }

   pub fn clip_text(mut self, b: bool) -> Self {
      if !self.multiline {
         self.clip_text = b;
      }
      self
   }

   pub fn char_limit(mut self, limit: usize) -> Self {
      self.char_limit = limit;
      self
   }

   pub fn return_key(mut self, return_key: impl Into<Option<KeyboardShortcut>>) -> Self {
      self.return_key = return_key.into();
      self
   }

   pub fn background_color(mut self, color: Color32) -> Self {
      self.background_color = Some(color);
      self
   }

   pub fn show(self, ui: &mut Ui) -> SecureTextEditOutput {
      let frame = self.frame;
      let where_to_put_background = ui.painter().add(Shape::Noop);
      let background_color = self.background_color.unwrap_or(ui.visuals().extreme_bg_color);
      let is_interactive = self.interactive;

      let output = self.show_content(ui);

      let outer_rect_for_frame = output.response.rect;
      if frame {
         let visuals = ui.style().interact(&output.response);
         let frame_rect = outer_rect_for_frame.expand(visuals.expansion);
         let shape = if is_interactive {
            if output.response.has_focus() {
               epaint::RectShape::new(
                  frame_rect,
                  visuals.corner_radius,
                  background_color,
                  ui.visuals().selection.stroke,
                  epaint::StrokeKind::Inside,
               )
            } else {
               epaint::RectShape::new(
                  frame_rect,
                  visuals.corner_radius,
                  background_color,
                  visuals.bg_stroke,
                  epaint::StrokeKind::Inside,
               )
            }
         } else {
            // Not interactive
            let visuals = &ui.style().visuals.widgets.inactive;
            epaint::RectShape::stroke(
               frame_rect,
               visuals.corner_radius,
               visuals.bg_stroke,
               epaint::StrokeKind::Inside,
            )
         };
         ui.painter().set(where_to_put_background, shape);
      }
      output
   }

   #[allow(clippy::too_many_lines)]
   fn show_content(self, ui: &mut Ui) -> SecureTextEditOutput {
      let font_id = self.font_selection.resolve(ui.style());
      let text_color = self
         .text_color
         .or(ui.visuals().override_text_color)
         .unwrap_or_else(|| ui.visuals().widgets.inactive.text_color());

      let row_height = ui.fonts_mut(|f| f.row_height(&font_id));
      let available_width = (ui.available_width() - self.margin.sum().x).at_least(24.0); // Min width
      let desired_width = self.desired_width.unwrap_or_else(|| ui.spacing().text_edit_width);
      let wrap_width = if ui.layout().horizontal_justify() {
         available_width
      } else {
         desired_width.min(available_width)
      };

      // --- Layout Galley ---
      let galley: Arc<Galley> = self.text.unlock_str(|text_slice| {
         let display_text_cow = if self.password {
            // Generate '●' string based on actual char count
            std::borrow::Cow::<'_, str>::Owned(
               std::iter::repeat(epaint::text::PASSWORD_REPLACEMENT_CHAR)
                  .take(text_slice.chars().count())
                  .collect::<String>(),
            )
         } else {
            std::borrow::Cow::Owned(text_slice.to_string()) // !
         };

         let mut job = if self.multiline {
            LayoutJob::simple(
               (*display_text_cow).to_owned(),
               font_id.clone(),
               text_color,
               wrap_width,
            )
         } else {
            LayoutJob::simple_singleline(
               (*display_text_cow).to_owned(),
               font_id.clone(),
               text_color,
            )
         };
         job.halign = self.align.0[0];
         ui.fonts_mut(|f| f.layout_job(job))
      });

      // --- Size & Allocation ---
      let desired_inner_width = if self.clip_text && !self.multiline {
         wrap_width
      } else {
         galley.size().x.max(wrap_width)
      };
      let desired_height = (self.desired_height_rows.at_least(1) as f32) * row_height;
      let desired_inner_size = vec2(
         desired_inner_width,
         galley.size().y.max(desired_height),
      );
      let desired_outer_size = (desired_inner_size + self.margin.sum()).at_least(self.min_size);

      let (auto_id, outer_rect) = ui.allocate_space(desired_outer_size);
      let text_draw_rect = outer_rect - self.margin;

      let id = self.id.unwrap_or_else(|| {
         if let Some(id_salt) = self.id_salt {
            ui.make_persistent_id(id_salt)
         } else {
            auto_id
         }
      });
      let mut state = SecureTextEditState::load(ui.ctx(), id).unwrap_or_default();

      // --- Interaction ---
      let allow_drag_to_select =
         ui.input(|i| !i.has_touch_screen()) || ui.memory(|mem| mem.has_focus(id));
      let sense_behavior = if self.interactive {
         if allow_drag_to_select {
            Sense::click_and_drag()
         } else {
            Sense::click()
         }
      } else {
         Sense::hover()
      };
      let mut response = ui.interact(outer_rect, id, sense_behavior);
      response.intrinsic_size = Some(vec2(desired_width, desired_outer_size.y));

      // Handle click to focus
      if self.interactive {
         if let Some(pointer_pos) = ui.ctx().pointer_interact_pos() {
            if response.hovered() {
               ui.output_mut(|o| o.mutable_text_under_cursor = true);
            }
            let singleline_offset_vec = vec2(state.singleline_offset, 0.0);
            let cursor_at_pointer =
               galley.cursor_from_pos(pointer_pos - text_draw_rect.min + singleline_offset_vec);

            let is_being_dragged = ui.ctx().is_being_dragged(response.id);
            let did_interact_with_cursor = state.cursor.pointer_interaction(
               ui,
               &response,
               cursor_at_pointer,
               &galley,
               is_being_dragged,
            );

            if did_interact_with_cursor || response.clicked() {
               ui.memory_mut(|mem| mem.request_focus(response.id));
               state.last_interaction_time = ui.input(|i| i.time);
            }
         }
      }
      if self.interactive && response.hovered() {
         ui.ctx().set_cursor_icon(CursorIcon::Text);
      }

      // --- Event Handling ---
      let mut cursor_range_after_events = None;
      // Initial galley before any events in this frame
      let current_frame_galley = galley.clone();

      if self.interactive && ui.memory(|mem| mem.has_focus(id)) {
         ui.memory_mut(|mem| mem.set_focus_lock_filter(id, self.event_filter));

         let default_cursor_range = if self.cursor_at_end {
            CCursorRange::one(current_frame_galley.end())
         } else {
            CCursorRange::default()
         };

         let (text_changed_by_event, new_cursor_range, _updated_galley_from_events) =
            secure_text_edit_events(
               ui,
               &mut state,
               self.text,
               &current_frame_galley,
               id,
               self.multiline,
               self.password,
               default_cursor_range,
               self.char_limit,
               self.event_filter,
               self.return_key,
               &font_id,
               text_color,
               wrap_width,
               self.align.0[0],
            );

         if text_changed_by_event {
            response.mark_changed();
         }
         cursor_range_after_events = Some(new_cursor_range);

         if !text_changed_by_event {
            state.cursor.set_char_range(Some(new_cursor_range));
         }
      }

      // --- Galley Positioning & Single-line Offset ---
      let mut galley_pos = self.align.align_size_within_rect(galley.size(), text_draw_rect).min;
      if self.clip_text && !self.multiline {
         let current_cursor_primary_x =
            match cursor_range_after_events.or_else(|| state.cursor.range(&galley)) {
               Some(cr) => galley.pos_from_cursor(cr.primary).min.x,
               None => 0.0,
            };
         let visible_width = text_draw_rect.width();
         let mut offset_x = state.singleline_offset;
         let visible_range_start = offset_x;
         let visible_range_end = offset_x + visible_width;

         if current_cursor_primary_x < visible_range_start {
            offset_x = current_cursor_primary_x;
         } else if current_cursor_primary_x > visible_range_end {
            offset_x = current_cursor_primary_x - visible_width;
         }
         offset_x = offset_x.at_most(galley.size().x - visible_width).at_least(0.0);
         state.singleline_offset = offset_x;
         galley_pos.x -= offset_x;
      } else {
         // For multiline or non-clip singleline, capture any alignment offset
         // state.singleline_offset = text_draw_rect.left() - galley_pos.x;
         state.singleline_offset = 0.0;
         // And ensure galley_pos respects it if it was aligned (e.g. center/right)
         state.singleline_offset = text_draw_rect.left() - galley_pos.x;
      }

      // --- Painting ---
      if ui.is_rect_visible(text_draw_rect) {
         let is_text_empty = self.text.char_len() == 0;
         if is_text_empty && !self.hint_text.is_empty() {
            let hint_text_color = ui.visuals().weak_text_color();
            let hint_font_id = FontSelection::default();
            let hint_galley = self.hint_text.clone().into_galley(
               ui,
               Some(TextWrapMode::Wrap),
               text_draw_rect.width(),
               hint_font_id,
            );
            let hint_galley_pos =
               self.align.align_size_within_rect(hint_galley.size(), text_draw_rect).min;
            ui.painter_at(text_draw_rect)
               .galley(hint_galley_pos, hint_galley, hint_text_color);
         }

         let mut galley_for_paint = galley.clone();
         if ui.memory(|mem| mem.has_focus(id)) {
            if let Some(cursor_range_for_sel) = state.cursor.range(&galley_for_paint) {
               text_selection::visuals::paint_text_selection(
                  &mut galley_for_paint,
                  ui.visuals(),
                  &cursor_range_for_sel,
                  None,
               );
            }
         }
         ui.painter_at(text_draw_rect).galley(galley_pos, galley_for_paint, text_color);

         // Paint cursor
         if self.interactive && ui.memory(|mem| mem.has_focus(id)) {
            if let Some(cursor_range_for_cursor_paint) = state.cursor.range(&galley) {
               // Use original galley for metrics
               let primary_cursor_rect_ui = text_selection::text_cursor_state::cursor_rect(
                  &galley,
                  &cursor_range_for_cursor_paint.primary,
                  row_height,
               )
               .translate(galley_pos.to_vec2());

               if response.changed() {
                  // Could also check selection_changed
                  ui.scroll_to_rect(
                     primary_cursor_rect_ui.expand(self.margin.sum().y / 2.0),
                     None,
                  );
               }

               if ui.ctx().input(|i| i.focused) {
                  // Viewport has focus
                  let time_since_last_interaction =
                     ui.input(|i| i.time) - state.last_interaction_time;
                  text_selection::visuals::paint_text_cursor(
                     ui,
                     &ui.painter_at(text_draw_rect.expand(1.0)), // Expand for cursor
                     primary_cursor_rect_ui,
                     time_since_last_interaction,
                  );
               }
               // IME output
               let to_global =
                  ui.ctx().layer_transform_to_global(ui.layer_id()).unwrap_or_default();
               ui.ctx().output_mut(|o| {
                  o.ime = Some(output::IMEOutput {
                     rect: to_global * text_draw_rect,
                     cursor_rect: to_global * primary_cursor_rect_ui,
                  });
               });
            }
         }
      }

      // IME focus state management
      if state.ime_enabled && (response.gained_focus() || response.lost_focus()) {
         state.ime_enabled = false;
         if let Some(mut ccursor_range) = state.cursor.char_range() {
            ccursor_range.secondary.index = ccursor_range.primary.index;
            state.cursor.set_char_range(Some(ccursor_range));
         }
         ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Ime(_))));
      }

      state.clone().store(ui.ctx(), id);

      // !
      // This is only for accessibility, so set them to empty is fine
      /*
      let _ = self.text.str_scope(|s| {
         if self.password {
            std::iter::repeat(epaint::text::PASSWORD_REPLACEMENT_CHAR)
               .take(s.chars().count())
               .collect()
         } else {
            s.to_string()
         }
      });
      */
      response.widget_info(|| {
         WidgetInfo::text_edit(
            ui.is_enabled(),
            String::new(),
            String::new(),
            String::new(),
         )
      });

      SecureTextEditOutput {
         response,
         state,
         cursor_range: cursor_range_after_events,
      }
   }
}

impl<'a> Widget for SecureTextEdit<'a> {
   fn ui(self, ui: &mut Ui) -> Response {
      self.show(ui).response
   }
}

#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn secure_text_edit_events(
   ui: &Ui,
   state: &mut SecureTextEditState,
   secure_text: &mut SecureString,
   initial_galley: &Arc<Galley>,
   id: Id,
   multiline: bool,
   password: bool,
   default_cursor_range: CCursorRange,
   char_limit: usize,
   event_filter: EventFilter,
   return_key: Option<KeyboardShortcut>,
   font_id: &FontId,
   text_color: Color32,
   wrap_width: f32,
   text_align_horizontal: Align,
) -> (bool, CCursorRange, Arc<Galley>) {
   let os = ui.ctx().os();
   let mut current_galley = initial_galley.clone();
   let mut cursor_range = state.cursor.range(&current_galley).unwrap_or(default_cursor_range);
   let mut text_changed_in_total = false;

   let mut events_filtered = ui.input(|i| i.filtered_events(&event_filter));
   if state.ime_enabled {
      events_filtered.sort_by_key(|e| !matches!(e, Event::Ime(_)));
   }

   for event in events_filtered {
      let current_char_len_before_event = secure_text.char_len();
      let mut text_mutated_this_event = false;

      // Pass current_galley to on_event. If it modifies cursor_range, it uses current_galley.
      if cursor_range.on_event(os, &event, &current_galley, id) {
         state.last_interaction_time = ui.input(|i| i.time);
         continue;
      }

      let new_ccursor_range_opt: Option<CCursorRange> = match event {
         // For now don't allow copy/cut on any text
         Event::Copy => None,
         Event::Cut => None,
         Event::Paste(mut text_to_paste) => {
            if !text_to_paste.is_empty() {
               let [min, max] = cursor_range.sorted_cursors();
               let selection_char_len = max.index - min.index;
               secure_text.delete_text_char_range(min.index..max.index);

               let space_available = char_limit
                  .saturating_sub(current_char_len_before_event.saturating_sub(selection_char_len));
               let mut final_text_to_paste = if text_to_paste.chars().count() > space_available {
                  text_to_paste.chars().take(space_available).collect::<String>()
               } else {
                  text_to_paste.clone()
               };

               let mut current_ccursor = min;
               let chars_inserted =
                  secure_text.insert_text_at_char_idx(current_ccursor.index, &final_text_to_paste);
               current_ccursor.index += chars_inserted;
               text_mutated_this_event = true; // Mark mutation

               text_to_paste.zeroize();
               final_text_to_paste.zeroize();
               Some(text::CCursorRange::one(current_ccursor))
            } else {
               None
            }
         }
         Event::Text(mut text_to_insert) => {
            if !text_to_insert.is_empty() && text_to_insert != "\n" && text_to_insert != "\r" {
               let [min, max] = cursor_range.sorted_cursors();
               let selection_char_len = max.index - min.index;
               secure_text.delete_text_char_range(min.index..max.index);

               let space_available = char_limit
                  .saturating_sub(current_char_len_before_event.saturating_sub(selection_char_len));
               let mut final_text_to_insert = if text_to_insert.chars().count() > space_available {
                  text_to_insert.chars().take(space_available).collect::<String>()
               } else {
                  text_to_insert.clone()
               };

               let mut current_ccursor = min;
               let chars_inserted =
                  secure_text.insert_text_at_char_idx(current_ccursor.index, &final_text_to_insert);
               current_ccursor.index += chars_inserted;
               text_mutated_this_event = true;

               text_to_insert.zeroize();
               final_text_to_insert.zeroize();
               Some(text::CCursorRange::one(current_ccursor))
            } else {
               None
            }
         }
         Event::Key {
            key: Key::Enter,
            pressed: true,
            modifiers,
            ..
         } if return_key.is_some_and(|rk| {
            Key::Enter == rk.logical_key && modifiers.matches_logically(rk.modifiers)
         }) =>
         {
            if multiline {
               let [min, max] = cursor_range.sorted_cursors();
               let selection_char_len = max.index - min.index;
               secure_text.delete_text_char_range(min.index..max.index);

               let current_len_after_delete =
                  current_char_len_before_event.saturating_sub(selection_char_len);
               let space_available = char_limit.saturating_sub(current_len_after_delete);

               if space_available > 0 {
                  let mut current_ccursor = min;
                  let chars_inserted =
                     secure_text.insert_text_at_char_idx(current_ccursor.index, "\n");
                  current_ccursor.index += chars_inserted;
                  text_mutated_this_event = true; // Mark mutation
                  Some(text::CCursorRange::one(current_ccursor))
               } else {
                  None
               }
            } else {
               ui.memory_mut(|mem| mem.surrender_focus(id));
               None
            }
         }
         Event::Key {
            key: Key::Backspace,
            pressed: true,
            ..
         } => {
            // Modifiers for word/para delete later
            let [min, max] = cursor_range.sorted_cursors();
            let mut new_cursor_idx = min.index;
            if min == max {
               // No selection
               if min.index > 0 {
                  secure_text.delete_text_char_range(min.index - 1..min.index);
                  new_cursor_idx = min.index - 1;
                  text_mutated_this_event = true;
               }
            } else {
               // Selection exists
               secure_text.delete_text_char_range(min.index..max.index);
               // new_cursor_idx is already min.ccursor.index
               text_mutated_this_event = true;
            }
            if text_mutated_this_event {
               Some(text::CCursorRange::one(text::CCursor::new(
                  new_cursor_idx,
               )))
            } else {
               None
            }
         }
         Event::Key {
            key: Key::Delete,
            pressed: true,
            ..
         } => {
            // Modifiers for word/para delete later
            let [min, max] = cursor_range.sorted_cursors();
            if min == max {
               if min.index < current_char_len_before_event {
                  // Before deleting
                  secure_text.delete_text_char_range(min.index..min.index + 1);
                  text_mutated_this_event = true;
               }
            } else {
               secure_text.delete_text_char_range(min.index..max.index);
               text_mutated_this_event = true;
            }
            if text_mutated_this_event {
               Some(text::CCursorRange::one(min))
            } else {
               None
            }
         }
         Event::Key {
            key: Key::Tab,
            pressed: true,
            modifiers,
            ..
         } if multiline && event_filter.tab => {
            let [min, _max] = cursor_range.sorted_cursors();
            let mut current_ccursor = min;
            if modifiers.shift {
            } else {
               let space_available = char_limit.saturating_sub(current_char_len_before_event);
               if space_available > 0 {
                  // Enough for at least '\t'
                  let chars_inserted =
                     secure_text.insert_text_at_char_idx(current_ccursor.index, "\t");
                  current_ccursor.index += chars_inserted;
                  text_mutated_this_event = true;
               }
            }
            if text_mutated_this_event {
               Some(text::CCursorRange::one(current_ccursor))
            } else {
               None
            }
         }
         Event::Ime(ime_event) => {
            match ime_event {
               ImeEvent::Enabled => {
                  state.ime_enabled = true;
                  state.ime_cursor_range = cursor_range;
                  None
               }
               ImeEvent::Preedit(mut preedit_text) => {
                  let [min_ime, max_ime] = state.ime_cursor_range.sorted_cursors(); // Use IME's original range for delete
                  secure_text.delete_text_char_range(min_ime.index..max_ime.index);
                  let mut c = min_ime; // Insert at start of IME original selection
                  let inserted = secure_text.insert_text_at_char_idx(c.index, &preedit_text);
                  c.index += inserted;
                  text_mutated_this_event = true;
                  preedit_text.zeroize();
                  Some(text::CCursorRange::two(min_ime, c))
               }
               ImeEvent::Commit(mut commit_text) => {
                  state.ime_enabled = false; // IME done
                  let [min_commit, max_commit] = cursor_range.sorted_cursors();
                  secure_text.delete_text_char_range(min_commit.index..max_commit.index);
                  let mut c = min_commit;
                  let inserted = secure_text.insert_text_at_char_idx(c.index, &commit_text);
                  c.index += inserted;
                  text_mutated_this_event = true;
                  commit_text.zeroize();
                  Some(text::CCursorRange::one(c))
               }
               ImeEvent::Disabled => {
                  state.ime_enabled = false;
                  None
               }
            }
         }
         _ => None,
      };

      if text_mutated_this_event {
         text_changed_in_total = true;

         // --- Re-layout galley ---
         current_galley = secure_text.unlock_str(|text_slice| {
            let display_text_for_layout = if password {
               std::iter::repeat(epaint::text::PASSWORD_REPLACEMENT_CHAR)
                  .take(text_slice.chars().count())
                  .collect::<String>()
            } else {
               text_slice.to_owned() // !
            };
            let mut job = if multiline {
               LayoutJob::simple(
                  display_text_for_layout,
                  font_id.clone(),
                  text_color,
                  wrap_width,
               )
            } else {
               LayoutJob::simple_singleline(
                  display_text_for_layout,
                  font_id.clone(),
                  text_color,
               )
            };
            job.halign = text_align_horizontal;
            ui.fonts_mut(|f| f.layout_job(job))
         });
      }

      // Set the final state.cursor using the most up-to-date cursor_range
      state.cursor.set_char_range(new_ccursor_range_opt);
      if let Some(new_range) = new_ccursor_range_opt {
         state.last_interaction_time = ui.input(|i| i.time);
         cursor_range = new_range;
      }
   }

   (
      text_changed_in_total,
      cursor_range,
      current_galley,
   )
}
