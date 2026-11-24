//! UI that allows the user to add,edit and remove contacts.

use crate::core::{ZeusContext, types::Contact};
use crate::gui::ui::common;
use crate::gui::ui::{
   address_for_speech, copied_words, name_address, place_in_list, show_with_fade,
};
use crate::gui::{SHARED_GUI, dots_button};
use egui::{
   Align, FontId, Layout, Margin, OpenUrl, Response, RichText, Role, ScrollArea, Ui, WidgetInfo,
   vec2,
};
use egui_elements::{
   Button, Frame, InputField, Label, Menu, MenuItem, QrImage, SecureTextEdit, Spinner, Theme,
   components::QrEncoding,
};
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;
use zeus_railgun::RailgunAddress;

const QR_IMAGE_SIZE: u32 = 250;

/// What a reader hears in place of a symbol: the address above, for another device to scan.
const QR_ALT: &str = "QR code of the address above, for another device to scan.";

/// Display size of an address field's QR icon.
const FIELD_ICON: f32 = 18.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContactsPageView {
   List,
   Add,
   Edit,
   Delete,
   Qr,
}

pub struct AddContact {
   contact: Contact,
   contact_added: bool,
   /// Public (EVM) address. It owns its text and its QR scanner, so it lives here
   /// instead of being rebuilt every frame.
   evm_field: InputField,
   /// Railgun (zk) address.
   zk_field: InputField,
}

impl AddContact {
   pub fn new() -> Self {
      Self {
         contact: Contact::default(),
         contact_added: false,
         evm_field: InputField::new("Public Address", true).icon_size(vec2(FIELD_ICON, FIELD_ICON)),
         zk_field: InputField::new("Railgun Address", true).icon_size(vec2(FIELD_ICON, FIELD_ICON)),
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      common::field_controls::input_field(&mut self.evm_field, shown);
      common::field_controls::input_field(&mut self.zk_field, shown);
   }

   pub fn contact_added(&self) -> bool {
      self.contact_added
   }

   pub fn reset(&mut self) {
      self.contact_added = false;
      self.contact = Contact::default();
      self.evm_field.erase();
      self.zk_field.erase();
      self.close_qr_scanners();
   }

   /// Stop a QR overlay opened from either address field.
   fn close_qr_scanners(&mut self) {
      self.evm_field.close_qr_scanner();
      self.zk_field.close_qr_scanner();
   }

   pub fn get_contact(&self) -> &Contact {
      &self.contact
   }

   /// Draw the form and hand back the contact-name field.
   ///
   /// The field is the page's first control: the page puts the focus there when the form opens,
   /// and the response is the only handle it can ask with.
   pub fn body(&mut self, theme: &Theme, reset_on_success: bool, ui: &mut Ui) -> Response {
      let mut add_clicked = false;

      let name_field = ui
         .vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.lg;
            ui.spacing_mut().button_padding = theme.button_padding;

            let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);
            let text_edit_visuals = theme.text_edit_visuals();
            let button_visuals = theme.button_visuals();

            let label = ui.label(RichText::new("Name").size(theme.typography.large));
            ui.ctx().accesskit_node_builder(label.id, |node| node.set_hidden());

            let name = &mut self.contact.name;
            let name_field = ui.add(
               SecureTextEdit::singleline(name)
                  .accessible_name("Contact name")
                  .visuals(text_edit_visuals)
                  .min_size(text_edit_size)
                  .margin(Margin::same(10))
                  .font(FontId::proportional(theme.typography.normal)),
            );

            // The address fields label themselves and own their text — and their QR
            // scanner — so the contact is filled in from them on submit.
            self.evm_field.set_min_size(text_edit_size);
            self.evm_field.show(ui);

            self.zk_field.set_min_size(text_edit_size);
            self.zk_field.show(ui);

            let text = RichText::new("Add").size(theme.typography.large);
            let size = vec2(100.0, 25.0);
            let button = Button::new(text).visuals(button_visuals).min_size(size);
            add_clicked = ui.add(button).clicked();

            name_field
         })
         .inner;

      if add_clicked {
         self.contact.evm_address = self.evm_field.text().to_owned();
         self.contact.zk_address = self.zk_field.text().to_owned();
         on_add_contact(self.contact.clone(), reset_on_success);
      }

      name_field
   }
}

enum DeleteContactAction {
   Idle,
   Deleted,
   Cancelled,
}

struct DeleteContact {
   contact_to_delete: Contact,
}

impl DeleteContact {
   fn new() -> Self {
      Self {
         contact_to_delete: Contact::default(),
      }
   }

   /// Do not touch `SHARED_GUI` here — Settings paint already holds that write lock.
   fn body(&mut self, theme: &Theme, ui: &mut Ui) -> DeleteContactAction {
      let contact_to_delete = self.contact_to_delete.clone();
      let mut action = DeleteContactAction::Idle;
      let card_width = ui.available_width().min(550.0);
      let frame = theme.frame2;

      ui.vertical_centered(|ui| {
         ui.set_width(card_width);
         ui.spacing_mut().item_spacing.y = theme.spacing.md;
         ui.spacing_mut().button_padding = theme.button_padding;

         common::section_heading(ui, theme.typography.heading, "Delete contact");

         ui.label(
            RichText::new("Are you sure you want to delete this contact?")
               .size(theme.typography.large),
         );

         frame.show(ui, |ui| {
            ui.set_max_width(card_width);

            ui.label(RichText::new(&contact_to_delete.name).size(theme.typography.large));

            ui.label(
               RichText::new(contact_to_delete.evm_address.to_string())
                  .size(theme.typography.small)
                  .color(theme.colors.text_muted)
                  .monospace(),
            );

            if !contact_to_delete.zk_address.is_empty() {
               ui.label(
                  RichText::new(&contact_to_delete.zk_address)
                     .size(theme.typography.small)
                     .color(theme.colors.text_muted)
                     .monospace(),
               );
            }
         });

         ui.add_space(10.0);

         let button_visuals = theme.button_visuals();
         let content_width = ui.available_width() * 0.9;
         let button_size = vec2((content_width - theme.spacing.sm) / 2.0, 45.0);

         ui.allocate_ui(vec2(content_width, 45.0), |ui| {
            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.md;

               let text = RichText::new("Changed my mind").size(theme.typography.normal);
               let button = Button::new(text).visuals(button_visuals).min_size(button_size);
               if ui.add(button).clicked() {
                  action = DeleteContactAction::Cancelled;
               }

               let text = RichText::new("Delete").size(theme.typography.normal);
               let button = Button::new(text).visuals(button_visuals).min_size(button_size);
               if ui.add(button).clicked() {
                  action = DeleteContactAction::Deleted;
               }
            });
         });
      });

      match action {
         DeleteContactAction::Deleted => {
            on_delete_contact(contact_to_delete);
            self.contact_to_delete = Contact::default();
            DeleteContactAction::Deleted
         }
         DeleteContactAction::Cancelled => {
            self.contact_to_delete = Contact::default();
            DeleteContactAction::Cancelled
         }
         DeleteContactAction::Idle => DeleteContactAction::Idle,
      }
   }
}

struct EditContact {
   contact_to_edit: Contact,
   old_contact: Contact,
   /// Public (EVM) address. It owns its text and its QR scanner, so it lives here
   /// instead of being rebuilt every frame.
   evm_field: InputField,
   /// Railgun (zk) address.
   zk_field: InputField,
}

impl EditContact {
   fn new() -> Self {
      Self {
         contact_to_edit: Contact::default(),
         old_contact: Contact::default(),
         evm_field: InputField::new("Address:", true).icon_size(vec2(FIELD_ICON, FIELD_ICON)),
         zk_field: InputField::new("Railgun Address:", true)
            .icon_size(vec2(FIELD_ICON, FIELD_ICON)),
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      common::field_controls::input_field(&mut self.evm_field, shown);
      common::field_controls::input_field(&mut self.zk_field, shown);
   }

   /// Start editing `contact`: remember it and load its addresses into the fields.
   fn begin(&mut self, contact: Contact) {
      self.evm_field.set_text(&contact.evm_address);
      self.zk_field.set_text(&contact.zk_address);
      self.old_contact = contact.clone();
      self.contact_to_edit = contact;
   }

   /// Stop a QR overlay opened from either address field.
   fn close_qr_scanners(&mut self) {
      self.evm_field.close_qr_scanner();
      self.zk_field.close_qr_scanner();
   }

   fn body(&mut self, theme: &Theme, ui: &mut Ui) {
      let res = ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = theme.spacing.lg;
         ui.spacing_mut().button_padding = theme.button_padding;
         let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);

         let text_edit_visuals = theme.text_edit_visuals();
         let button_visuals = theme.button_visuals();

         let mut contact = self.contact_to_edit.clone();
         let label = ui.label(RichText::new("Name:").size(theme.typography.large));
         ui.ctx().accesskit_node_builder(label.id, |node| node.set_hidden());

         let name = &mut contact.name;
         ui.add(
            SecureTextEdit::singleline(name)
               .accessible_name("Contact name")
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         self.contact_to_edit = contact;

         // The address fields label themselves and own their text — and their QR
         // scanner — so the contact is filled in from them on save.
         self.evm_field.set_min_size(text_edit_size);
         self.evm_field.show(ui);

         self.zk_field.set_min_size(text_edit_size);
         self.zk_field.show(ui);

         let text = RichText::new("Save").size(theme.typography.large);
         let size = vec2(100.0, 25.0);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         ui.add(button)
      });

      if res.inner.clicked() {
         self.contact_to_edit.evm_address = self.evm_field.text().to_owned();
         self.contact_to_edit.zk_address = self.zk_field.text().to_owned();
         on_edit_contact(
            self.old_contact.clone(),
            self.contact_to_edit.clone(),
         );
      }
   }
}

struct QrWindow {
   contact: Option<Contact>,
   evm_address_qr: QrImage,
   zk_address_qr: QrImage,
   /// The words this page wants said, drained by the page that draws it.
   notice: Option<String>,
}

impl QrWindow {
   fn new() -> Self {
      Self {
         contact: None,
         evm_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         zk_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         notice: None,
      }
   }

   fn open(&mut self, contact: Contact) {
      on_open_contact_qr(contact.clone());
      self.contact = Some(contact);
   }

   fn reset(&mut self) {
      *self = Self::new();
   }

   /// The words this page wants said, drained by the page that draws it.
   fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   fn body(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let privacy_mode = ctx.privacy_mode;

      ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);
      ui.spacing_mut().button_padding = theme.button_padding;

      if self.contact.is_none() {
         ui.label(
            RichText::new(
               "No contact selected. Close this window and open it from the contact's menu.",
            )
            .size(theme.typography.normal),
         );
         ui.add(Spinner::new().size(17.0));
         return;
      }

      // The page is drawn from the window above it, which holds the only announcement channel,
      // so what it wants said is left here for that window to drain.
      let mut notice = None;

      let frame = theme.frame1;

      frame.show(ui, |ui| {
         ui.set_max_width(ui.available_width() * 0.5);

         if let Some(contact) = self.contact.as_ref() {
            ui.label(RichText::new(&contact.name).size(theme.typography.large));

            let text = match privacy_mode {
               false => "Public Address (EVM)",
               true => "Private Address (zk)",
            };

            // The page's only statement of which address is on screen, so it is a heading a
            // reader can move to rather than one more line of prose.
            common::section_heading(ui, theme.typography.large, text);

            let address = match privacy_mode {
               false => contact.evm_address.clone(),
               true => contact.zk_address.clone(),
            };

            if !address.is_empty() {
               // The drawn text is the whole address, which a reader cannot check by ear as a
               // run: the node names it grouped, and keeps the shortening as its description.
               let spoken = address_for_speech(None, &address);
               let address_text = RichText::new(address.clone()).size(theme.typography.normal);
               let button =
                  Button::selectable(false, address_text).visuals(theme.button_visuals()).wrap();

               let response = ui.add(button);
               name_address(ui, &response, &spoken);

               // Nothing on screen changes when the address is copied, so the page has to say
               // that it happened — and which address, since the row itself is not spoken.
               if response.clicked() {
                  notice = Some(copied_words(&address));
                  ui.ctx().copy_text(address);
               }
            }
         }

         if !privacy_mode {
            if let Some(error) = self.evm_address_qr.error() {
               ui.add_space(10.0);
               ui.label(RichText::new(error.to_string()).size(theme.typography.large));
            }
         } else if let Some(error) = self.zk_address_qr.error() {
            ui.add_space(10.0);
            ui.label(RichText::new(error.to_string()).size(theme.typography.large));
         }
      });

      ui.add_space(10.0);

      // The symbol carries the sentence it is for (`QrImage::alt`) and draws itself with it: the
      // node is named by that sentence, and egui's own account of the load state — `Loading
      // bytes://…` — never reaches the tree.
      if !privacy_mode {
         self.evm_address_qr.show(ui);
      } else {
         self.zk_address_qr.show(ui);
      }

      if let Some(text) = notice {
         self.notice = Some(text);
      }
   }
}

pub struct ContactsUi {
   view: ContactsPageView,
   search_query: String,
   pub add_contact: AddContact,
   delete_contact: DeleteContact,
   edit_contact: EditContact,
   qr_window: QrWindow,
   /// The words the QR page raised, drained by the Settings window after the page is drawn.
   notice: Option<String>,
   /// Open the add-contact form with the focus on its name field, once.
   focus_name_pending: bool,
   /// Give the focus back to the Add Contact button when the list is drawn again, once.
   focus_add_pending: bool,
}

impl ContactsUi {
   pub fn new() -> Self {
      Self {
         view: ContactsPageView::List,
         search_query: String::new(),
         add_contact: AddContact::new(),
         delete_contact: DeleteContact::new(),
         edit_contact: EditContact::new(),
         qr_window: QrWindow::new(),
         notice: None,
         focus_name_pending: false,
         focus_add_pending: false,
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      self.add_contact.set_field_controls(shown);
      self.edit_contact.set_field_controls(shown);
   }

   /// The words the page wants said, drained by the Settings window right after it is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn reset_page(&mut self) {
      if self.view == ContactsPageView::Qr {
         self.qr_window.reset();
      }
      self.view = ContactsPageView::List;
      self.search_query.clear();
      self.close_qr_scanners();
   }

   /// Stop a QR overlay opened from an address field.
   ///
   /// The add/edit forms draw their fields only on their own view, and a scanner
   /// keeps its capture worker alive while an overlay is open, so a scanner left
   /// open when the fields stop being drawn has to be stopped here.
   pub fn close_qr_scanners(&mut self) {
      self.add_contact.close_qr_scanners();
      self.edit_contact.close_qr_scanners();
   }

   pub fn show_page(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let view = self.view;

      // The inactive view's fields are not drawn, so its scanner must not be left
      // capturing (e.g. the user hit Back with the QR overlay still open).
      if view != ContactsPageView::Add {
         self.add_contact.close_qr_scanners();
      }
      if view != ContactsPageView::Edit {
         self.edit_contact.close_qr_scanners();
      }

      show_with_fade(
         ui,
         "contacts_list_ui_fade",
         view == ContactsPageView::List,
         |ui| {
            self.list_ui(ctx, theme, ui);
         },
      );

      show_with_fade(
         ui,
         "contacts_add_ui_fade",
         view == ContactsPageView::Add,
         |ui| {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.label(RichText::new("Add contact").size(theme.typography.heading));
               let name_field = self.add_contact.body(theme, true, ui);

               // The page opens on the field the user came to fill, once, on the frame the form
               // has painted: a sizing pass draws nothing a reader can be handed the focus on.
               if self.focus_name_pending && !ui.is_sizing_pass() {
                  name_field.request_focus();
                  self.focus_name_pending = false;
               }
            });
         },
      );

      show_with_fade(
         ui,
         "contacts_edit_ui_fade",
         view == ContactsPageView::Edit,
         |ui| {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.label(RichText::new("Edit contact").size(theme.typography.heading));
               self.edit_contact.body(theme, ui);
            });
         },
      );

      let mut delete_action = DeleteContactAction::Idle;
      show_with_fade(
         ui,
         "contacts_delete_ui_fade",
         view == ContactsPageView::Delete,
         |ui| {
            self.back_row(theme, ui);
            delete_action = self.delete_contact.body(theme, ui);
         },
      );
      match delete_action {
         DeleteContactAction::Deleted | DeleteContactAction::Cancelled => {
            self.view = ContactsPageView::List;
         }
         DeleteContactAction::Idle => {}
      }

      show_with_fade(
         ui,
         "contacts_qr_ui_fade",
         view == ContactsPageView::Qr,
         |ui| {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               common::section_heading(ui, theme.typography.heading, "Contact QR Code");
               self.qr_window.body(ctx, theme, ui);
            });
         },
      );

      // The QR page cannot announce from inside its own draw — the window above it holds the only
      // channel — so what it raised is taken here for that window to say.
      if let Some(text) = self.qr_window.take_notice() {
         self.notice = Some(text);
      }
   }

   fn back_row(&mut self, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Back").size(theme.typography.normal);
      let button = Button::new(text).min_size(vec2(50.0, 20.0));

      let res = ui.scope(|ui| {
         ui.spacing_mut().button_padding = theme.button_padding;
         ui.add(button)
      });

      if res.inner.clicked() {
         if self.view == ContactsPageView::Qr {
            self.qr_window.evm_address_qr.clear(ui.ctx());
            self.qr_window.zk_address_qr.clear(ui.ctx());
            self.qr_window.reset();
         }

         // Back leaves the user on the list, at the button that opened the form: the focus
         // follows them there when the list is drawn again.
         if self.view == ContactsPageView::Add {
            self.focus_add_pending = true;
         }

         self.view = ContactsPageView::List;
      }
      ui.add_space(8.0);
   }

   fn list_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let contacts = ctx.read_wallet_state(|ws| ws.contacts.clone());

      let text_edit_visuals = theme.text_edit_visuals();
      let button_visuals = theme.button_visuals();

      ui.spacing_mut().item_spacing.y = theme.spacing.sm;
      ui.spacing_mut().button_padding = theme.button_padding;

      let text = RichText::new("Add Contact").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);
      let add_contact = ui.add(button);

      if add_contact.clicked() {
         self.view = ContactsPageView::Add;
         self.focus_name_pending = true;
         self.focus_add_pending = false;
      } else if self.focus_add_pending && !ui.is_sizing_pass() {
         // Back from the form: the button that opens it is where the user left off.
         add_contact.request_focus();
         self.focus_add_pending = false;
      }

      ui.add_space(12.0);

      if contacts.is_empty() {
         ui.label(RichText::new("No contacts found").size(theme.typography.large));
         return;
      }

      let hint_text = "Search contacts or enter an address";
      let hint = RichText::new(hint_text)
         .size(theme.typography.normal)
         .color(theme.colors.text_muted);

      let search = ui.add(
         SecureTextEdit::singleline(&mut self.search_query)
            .accessible_name("Search contacts")
            .visuals(text_edit_visuals)
            .hint_text(hint)
            .min_size(vec2(ui.available_width() * 0.40, 25.0))
            .margin(Margin::same(10))
            .font(FontId::proportional(theme.typography.normal)),
      );

      // The field drew only a hint; it is named after what it searches, and the hint becomes its
      // description.
      ui.ctx()
         .accesskit_node_builder(search.id, |node| node.set_description(hint_text));

      ui.add_space(15.0);

      ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
         Frame::new().inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());

            let visible = contacts
               .iter()
               .filter(|contact| valid_contact_search(contact, &self.search_query))
               .count();

            let mut position = 0;

            for contact in &contacts {
               let valid = valid_contact_search(contact, &self.search_query);

               if !valid {
                  continue;
               }

               self.contact(ctx, theme, contact, position, visible, ui);
               position += 1;
            }
         });
      });
   }

   fn contact(
      &mut self,
      ctx: &ZeusContext,
      theme: &Theme,
      contact: &Contact,
      position: usize,
      len: usize,
      ui: &mut Ui,
   ) {
      let privacy_mode = ctx.privacy_mode;

      let visuals = theme.frame2_visuals();
      let inner = theme.inner_margin;
      let corner = theme.corner_radius;
      let frame = Frame::new()
         .interactive(true)
         .inner_margin(inner)
         .corner_radius(corner)
         .visuals(visuals);

      let row = frame.show(ui, |ui| {
         ui.spacing_mut().button_padding = theme.button_padding;
         ui.set_width(450.0);

         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let text = RichText::new(&contact.name).size(theme.typography.large);
               let label = Label::new(text, None).wrap().interactive(false);
               let name = ui.add(label);
               ui.ctx().accesskit_node_builder(name.id, |node| node.set_hidden());
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let size = vec2(28.0, 20.0);
               let more = dots_button(theme, size, ui);
               let options_name = format!("Options for {}", contact.name);
               more.widget_info(|| {
                  WidgetInfo::labeled(Role::Button, ui.is_enabled(), &options_name)
               });
               let id = format!("{}_more_options", contact.evm_address);

               // A chosen item closes the menu: a reader activates it with Enter, which presses
               // no pointer, so the popup would otherwise stay open over what it opened.
               Menu::new(id).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Edit")).clicked() {
                     ui.close();
                     self.view = ContactsPageView::Edit;
                     self.edit_contact.begin(contact.clone());
                  }

                  if ui.add(MenuItem::new("Delete")).clicked() {
                     ui.close();
                     self.view = ContactsPageView::Delete;
                     self.delete_contact.contact_to_delete = contact.clone();
                  }

                  if ui.add(MenuItem::new("Show QR Code")).clicked() {
                     ui.close();
                     self.view = ContactsPageView::Qr;
                     self.qr_window.open(contact.clone());
                  }

                  if ui.add(MenuItem::new("See on Block Explorer")).clicked() {
                     ui.close();
                     let chain = ctx.chain;
                     let explorer = chain.block_explorer();
                     let link = format!("{}/address/{}", explorer, contact.evm_address);
                     let url = OpenUrl::new_tab(link);
                     ui.ctx().open_url(url);
                  }
               });
            });
         });

         ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.xs);

            let address_short = match privacy_mode {
               false => contact.evm_address.clone(),
               true => contact.zk_address_truncated(),
            };

            let address_full = match privacy_mode {
               false => contact.evm_address.clone(),
               true => contact.zk_address.clone(),
            };

            let address_text = RichText::new(&address_short)
               .size(theme.typography.normal)
               .color(theme.colors.text_muted);

            let copy_name = format!("Copy address {address_full}");
            let label = Button::selectable(false, address_text);
            let copy = ui.add(label);
            copy.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), &copy_name));

            if copy.clicked() {
               self.notice = Some(copied_words(&address_full));
               ui.ctx().copy_text(address_full);
            }
         });
      });

      // The row itself is one node named after the contact; the name label inside it is drawn
      // but hidden from the tree, so the row says whose it is once.
      row.response
         .widget_info(|| WidgetInfo::labeled(Role::Group, ui.is_enabled(), &contact.name));

      place_in_list(ui, row.response.id, position, len);
   }
}

#[cfg(test)]
impl ContactsUi {
   /// Open the QR page on `contact` with the symbols the real opening path encodes.
   ///
   /// That path hands them to a worker which writes the frame's global, and a tree test has no
   /// frame for it to land in, so the same construction runs here instead.
   pub(crate) fn show_contact_qr_for_test(&mut self, contact: Contact) {
      let (evm_address_qr, zk_address_qr) = contact_qrs(&contact);

      self.view = ContactsPageView::Qr;
      self.qr_window.evm_address_qr = evm_address_qr;
      self.qr_window.zk_address_qr = zk_address_qr;
      self.qr_window.contact = Some(contact);
   }
}

fn valid_contact_search(contact: &Contact, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   contact.name.to_lowercase().contains(&query)
      || contact.evm_address.to_lowercase().contains(&query)
}

/// Parse EVM address and optional zk address. On failure, opens the msg window
/// and returns false. Call only from a worker — not from the frame path.
fn validate_contact_addresses(contact: &Contact) -> bool {
   if let Err(e) = Address::from_str(&contact.evm_address) {
      SHARED_GUI.write(|gui| {
         let msg = format!("Address is not an Ethereum address: {}", e);
         gui.open_msg_window(msg);
         gui.request_repaint();
      });
      return false;
   }

   if !contact.zk_address.is_empty()
      && let Err(e) = RailgunAddress::from_zk_address(&contact.zk_address)
   {
      SHARED_GUI.write(|gui| {
         let msg = format!("Address is not a valid Railgun address: {}", e);
         gui.open_msg_window(msg);
         gui.request_repaint();
      });
      return false;
   }

   true
}

fn on_add_contact(new_contact: Contact, reset_on_success: bool) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      if !validate_contact_addresses(&new_contact) {
         return;
      }

      match ctx.add_contact(new_contact.clone()) {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.settings.contacts_ui.add_contact.contact_added = true;
               if reset_on_success {
                  gui.settings.contacts_ui.add_contact.reset();
                  gui.settings.contacts_ui.view = ContactsPageView::List;
               }
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!("Failed to add contact: {}", e));
               gui.request_repaint();
            });
            return;
         }
      }

      match ctx.save_wallet_state() {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.announce("Contact added.");
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               let error = format!(
                  "Changes didn't take effect, encountered error: {}",
                  e
               );
               gui.open_msg_window(error);
               gui.request_repaint();
            });
            ctx.remove_contact(&new_contact.evm_address);
         }
      }
   });
}

fn on_delete_contact(contact_to_delete: Contact) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      ctx.remove_contact(&contact_to_delete.evm_address);

      // Reset the recipient selection to avoid sending to the wrong address.
      SHARED_GUI.write(|gui| {
         gui.recipient_selection.reset();
      });

      match ctx.save_wallet_state() {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.announce("Contact deleted.");
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               let error = format!(
                  "Changes didn't take effect, encountered error: {}",
                  e
               );
               gui.open_msg_window(error);
               gui.request_repaint();
            });
            let _res = ctx.add_contact(contact_to_delete);
         }
      }
   });
}

fn on_edit_contact(old_contact: Contact, edited_contact: Contact) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      if !validate_contact_addresses(&edited_contact) {
         return;
      }

      SHARED_GUI.write(|gui| {
         gui.settings.contacts_ui.edit_contact.contact_to_edit = Contact::default();
         gui.settings.contacts_ui.edit_contact.old_contact = Contact::default();
         gui.settings.contacts_ui.view = ContactsPageView::List;

         // Reset the recipient selection to avoid sending to the wrong address.
         gui.recipient_selection.reset();
      });

      ctx.write_wallet_state(|ws| {
         let new_contact =
            ws.contacts.iter_mut().find(|c| c.evm_address == old_contact.evm_address);
         if let Some(new_contact) = new_contact {
            *new_contact = edited_contact.clone();
         }
      });

      match ctx.save_wallet_state() {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window("Contact saved.");
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               let error = format!(
                  "Changes didn't take effect, encountered error: {}",
                  e
               );
               gui.open_msg_window(error);
               gui.request_repaint();
            });

            ctx.write_wallet_state(|ws| {
               let new_contact =
                  ws.contacts.iter_mut().find(|c| c.evm_address == edited_contact.evm_address);
               if let Some(new_contact) = new_contact {
                  *new_contact = old_contact.clone();
               }
            });
         }
      }
   });
}

fn on_open_contact_qr(contact: Contact) {
   crate::runtime::rt().spawn_blocking(move || {
      let (evm_address_qr, zk_address_qr) = contact_qrs(&contact);

      SHARED_GUI.write(|gui| {
         gui.settings.contacts_ui.qr_window.evm_address_qr = evm_address_qr;
         gui.settings.contacts_ui.qr_window.zk_address_qr = zk_address_qr;
         gui.request_repaint();
      });
   });
}

/// Both symbols of `contact`, with the page's one sentence as their text alternative.
///
/// A symbol that is not drawn says nothing: with no address to encode the page draws its error
/// line instead, and naming an empty picture after a code that is not there would claim more than
/// the page shows.
fn contact_qrs(contact: &Contact) -> (QrImage, QrImage) {
   let encoding = QrEncoding {
      target_px: QR_IMAGE_SIZE,
      ..Default::default()
   };

   let data = contact.evm_address.clone();
   let uri = format!("bytes://contact-{}.png", contact.evm_address);
   let evm_address_qr = QrImage::with_encoding(&data, uri, encoding).alt(QR_ALT);

   let zk_address_qr = if !contact.zk_address.is_empty() {
      let data = contact.zk_address.clone();
      let uri = format!("bytes://contact-{}.png", contact.zk_address);
      QrImage::with_encoding(&data, uri, encoding).alt(QR_ALT)
   } else {
      QrImage::empty_with_error("No zkAddress available".to_string())
   };

   (evm_address_qr, zk_address_qr)
}
