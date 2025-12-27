use egui::{
   Atom, AtomExt as _, AtomKind, AtomLayout, AtomLayoutResponse, Frame, Image, IntoAtoms,
   NumExt as _, Response, Sense, Shadow, TextStyle, TextWrapMode, Ui, Vec2, Widget, WidgetInfo,
   WidgetText, WidgetType,
};

use zeus_theme::visuals::ButtonVisuals;

#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct Button<'a> {
   layout: AtomLayout<'a>,
   visuals: Option<ButtonVisuals>,
   small: bool,
   frame_when_inactive: bool,
   min_size: Vec2,
   selected: bool,
   image_tint_follows_text_color: bool,
   limit_image_size: bool,
}

impl<'a> Button<'a> {
   pub fn new(atoms: impl IntoAtoms<'a>) -> Self {
      Self {
         layout: AtomLayout::new(atoms.into_atoms())
            .sense(Sense::click())
            .fallback_font(TextStyle::Button),
         visuals: None,
         small: false,
         frame_when_inactive: true,
         min_size: Vec2::ZERO,
         selected: false,
         image_tint_follows_text_color: false,
         limit_image_size: false,
      }
   }

   /// Show a selectable button.
   ///
   /// Equivalent to:
   /// ```rust
   /// # use egui::{Button, IntoAtoms, __run_test_ui};
   /// # __run_test_ui(|ui| {
   /// let selected = true;
   /// ui.add(Button::new("toggle me").selected(selected).frame_when_inactive(!selected).frame(true));
   /// # });
   /// ```
   ///
   /// See also:
   ///   - [`Ui::selectable_value`]
   ///   - [`Ui::selectable_label`]
   pub fn selectable(selected: bool, atoms: impl IntoAtoms<'a>) -> Self {
      Self::new(atoms).selected(selected).frame_when_inactive(selected)
   }

   /// Creates a button with an image. The size of the image as displayed is defined by the provided size.
   ///
   /// Note: In contrast to [`Button::new`], this limits the image size to the default font height
   /// (using [`crate::AtomExt::atom_max_height_font_size`]).
   pub fn image(image: impl Into<Image<'a>>) -> Self {
      Self::opt_image_and_text(Some(image.into()), None)
   }

   /// Creates a button with an image to the left of the text.
   ///
   /// Note: In contrast to [`Button::new`], this limits the image size to the default font height
   /// (using [`crate::AtomExt::atom_max_height_font_size`]).
   pub fn image_and_text(image: impl Into<Image<'a>>, text: impl Into<WidgetText>) -> Self {
      Self::opt_image_and_text(Some(image.into()), Some(text.into()))
   }

   /// Create a button with an optional image and optional text.
   ///
   /// Note: In contrast to [`Button::new`], this limits the image size to the default font height
   /// (using [`crate::AtomExt::atom_max_height_font_size`]).
   pub fn opt_image_and_text(image: Option<Image<'a>>, text: Option<WidgetText>) -> Self {
      let mut button = Self::new(());
      if let Some(image) = image {
         button.layout.push_right(image);
      }
      if let Some(text) = text {
         button.layout.push_right(text);
      }
      button.limit_image_size = true;
      button
   }

   /// Set the wrap mode for the text.
   ///
   /// By default, [`crate::Ui::wrap_mode`] will be used, which can be overridden with [`crate::Style::wrap_mode`].
   ///
   /// Note that any `\n` in the text will always produce a new line.
   #[inline]
   pub fn wrap_mode(mut self, wrap_mode: TextWrapMode) -> Self {
      self.layout = self.layout.wrap_mode(wrap_mode);
      self
   }

   /// Set [`Self::wrap_mode`] to [`TextWrapMode::Wrap`].
   #[inline]
   pub fn wrap(self) -> Self {
      self.wrap_mode(TextWrapMode::Wrap)
   }

   /// Set [`Self::wrap_mode`] to [`TextWrapMode::Truncate`].
   #[inline]
   pub fn truncate(self) -> Self {
      self.wrap_mode(TextWrapMode::Truncate)
   }

   /// Make this a small button, suitable for embedding into text.
   #[inline]
   pub fn small(mut self) -> Self {
      self.small = true;
      self
   }

   /// If `false`, the button will not have a frame when inactive.
   ///
   /// Default: `true`.
   ///
   /// Note: When [`Self::frame`] (or `ui.visuals().button_frame`) is `false`, this setting
   /// has no effect.
   #[inline]
   pub fn frame_when_inactive(mut self, frame_when_inactive: bool) -> Self {
      self.frame_when_inactive = frame_when_inactive;
      self
   }

   /// By default, buttons senses clicks.
   /// Change this to a drag-button with `Sense::drag()`.
   #[inline]
   pub fn sense(mut self, sense: Sense) -> Self {
      self.layout = self.layout.sense(sense);
      self
   }

   /// Set the minimum size of the button.
   #[inline]
   pub fn min_size(mut self, min_size: Vec2) -> Self {
      self.min_size = min_size;
      self
   }

   /// If true, the tint of the image is multiplied by the widget text color.
   ///
   /// This makes sense for images that are white, that should have the same color as the text color.
   /// This will also make the icon color depend on hover state.
   ///
   /// Default: `false`.
   #[inline]
   pub fn image_tint_follows_text_color(mut self, image_tint_follows_text_color: bool) -> Self {
      self.image_tint_follows_text_color = image_tint_follows_text_color;
      self
   }

   /// Show some text on the right side of the button, in weak color.
   ///
   /// Designed for menu buttons, for setting a keyboard shortcut text (e.g. `Ctrl+S`).
   ///
   /// The text can be created with [`crate::Context::format_shortcut`].
   ///
   /// See also [`Self::right_text`].
   #[inline]
   pub fn shortcut_text(mut self, shortcut_text: impl Into<Atom<'a>>) -> Self {
      let mut atom = shortcut_text.into();
      atom.kind = match atom.kind {
         AtomKind::Text(text) => AtomKind::Text(text.weak()),
         other => other,
      };
      self.layout.push_right(Atom::grow());
      self.layout.push_right(atom);
      self
   }

   /// Show some text on the right side of the button.
   #[inline]
   pub fn right_text(mut self, right_text: impl Into<Atom<'a>>) -> Self {
      self.layout.push_right(Atom::grow());
      self.layout.push_right(right_text.into());
      self
   }

   /// If `true`, mark this button as "selected".
   #[inline]
   pub fn selected(mut self, selected: bool) -> Self {
      self.selected = selected;
      self
   }

   /// Set the visuals of the button
   #[inline]
   pub fn visuals(mut self, visuals: ButtonVisuals) -> Self {
      self.visuals = Some(visuals);
      self
   }

   /// Show the button and return a [`AtomLayoutResponse`] for painting custom contents.
   pub fn atom_ui(self, ui: &mut Ui) -> AtomLayoutResponse {
      let Button {
         mut layout,
         small,
         visuals,
         frame_when_inactive,
         mut min_size,
         selected,
         image_tint_follows_text_color,
         limit_image_size,
      } = self;

      if !small {
         min_size.y = min_size.y.at_least(ui.spacing().interact_size.y);
      }
      if limit_image_size {
         layout.map_atoms(|atom| {
            if matches!(&atom.kind, AtomKind::Image(_)) {
               atom.atom_max_height_font_size(ui)
            } else {
               atom
            }
         });
      }

      let text = layout.text().map(String::from);

      let has_frame_margin = visuals.is_some() || ui.visuals().button_frame;
      let mut button_padding = if has_frame_margin {
         ui.spacing().button_padding
      } else {
         Vec2::ZERO
      };

      if small {
         button_padding.y = 0.0;
      }

      let frame = Frame::new().inner_margin(button_padding);
      let mut prepared = layout.frame(frame).min_size(min_size).allocate(ui);

      let response = if ui.is_rect_visible(prepared.response.rect) {
         let interact_visuals = ui.style().interact_selectable(&prepared.response, selected);

         let is_active = prepared.response.is_pointer_button_down_on(); // Clicked
         let is_hovered = prepared.response.hovered();

         // Select custom visuals based on state, or fall back to defaults
         let (fill, stroke, corner_radius) = if let Some(v) = &visuals {
            if is_active {
               (v.bg_click, v.border_click, v.corner_radius)
            } else if is_hovered {
               (v.bg_hover, v.border_hover, v.corner_radius)
            } else {
               (v.bg, v.border, v.corner_radius)
            }
         } else {
            (
               interact_visuals.weak_bg_fill,
               interact_visuals.bg_stroke,
               interact_visuals.corner_radius,
            )
         };

         let fill = match selected {
            false => fill,
            true => {
               visuals.as_ref().map(|v| v.bg_selected).unwrap_or(interact_visuals.weak_bg_fill)
            }
         };

         let text_color = visuals.as_ref().map(|v| v.text).unwrap_or(interact_visuals.text_color());

         let visible_frame = if frame_when_inactive {
            has_frame_margin
         } else {
            has_frame_margin && (is_hovered || is_active || prepared.response.has_focus())
         };

         if image_tint_follows_text_color {
            prepared.map_images(|image| image.tint(text_color));
         }
         prepared.fallback_text_color = text_color;

         if visible_frame {
            let shadow = visuals.as_ref().map(|v| v.shadow).unwrap_or(Shadow::NONE);
            prepared.frame = prepared
               .frame
               .inner_margin(
                  button_padding + Vec2::splat(interact_visuals.expansion)
                     - Vec2::splat(stroke.width),
               )
               .outer_margin(-Vec2::splat(interact_visuals.expansion))
               .fill(fill)
               .stroke(stroke)
               .corner_radius(corner_radius)
               .shadow(shadow);
         }

         prepared.paint(ui)
      } else {
         AtomLayoutResponse::empty(prepared.response)
      };

      response.response.widget_info(|| {
         if let Some(text) = &text {
            WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), text)
         } else {
            WidgetInfo::new(WidgetType::Button)
         }
      });

      response
   }
}

impl Widget for Button<'_> {
   fn ui(self, ui: &mut Ui) -> Response {
      self.atom_ui(ui).response
   }
}
