//! UI that allows the user to add,edit and remove contacts.

use crate::core::{ZeusContext, types::Contact};
use crate::gui::{SHARED_GUI, dots_button};
use crate::utils::RT;
use egui::{
   Align, Align2, FontId, Frame, Layout, Margin, OpenUrl, Order, RichText, ScrollArea, Spinner,
   Stroke, Ui, vec2,
};
use egui_elements::{
   Button, Label, OverlayManager, QrImage, SecureTextEdit, Theme, widgets::Window,
};
use elegance::{Menu, MenuItem};
use std::str::FromStr;
use zeus_eth::alloy_primitives::Address;
use zeus_railgun::RailgunAddress;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContactsPageView {
   List,
   Add,
   Edit,
   Delete,
   Qr,
}

pub struct AddContact {
   open: bool,
   overlay: OverlayManager,
   contact: Contact,
   contact_added: bool,
}

impl AddContact {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         contact: Contact::default(),
         contact_added: false,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
         self.open = true;
      }
   }

   pub fn close(&mut self) {
      if self.open {
         self.overlay.window_closed();
         self.open = false;
      }
   }

   pub fn contact_added(&self) -> bool {
      self.contact_added
   }

   pub fn reset(&mut self) {
      self.close();
      self.contact_added = false;
      self.contact = Contact::default();
   }

   pub fn get_contact(&self) -> &Contact {
      &self.contact
   }

   /// Window used by recipient selection on the main app.
   pub fn show(&mut self, theme: &Theme, reset_on_success: bool, ui: &mut Ui) {
      let mut open = self.open;
      if !open {
         return;
      }

      let window_frame = theme.window_frame.fill(theme.frame1.fill);
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(RichText::new("Add new contact").size(theme.typography.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);
            ui.set_height(250.0);
            self.body(theme, reset_on_success, ui);
         });

      if !open {
         self.close();
      }
   }

   fn body(&mut self, theme: &Theme, reset_on_success: bool, ui: &mut Ui) {
      let res = ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = theme.spacing.lg;
         ui.spacing_mut().button_padding = theme.button_padding;

         let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);
         let text_edit_visuals = theme.text_edit_visuals();
         let button_visuals = theme.button_visuals();

         ui.label(RichText::new("Name").size(theme.typography.large));
         let name = &mut self.contact.name;
         ui.add(
            SecureTextEdit::singleline(name)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         ui.label(RichText::new("Public Address").size(theme.typography.large));
         let address = &mut self.contact.evm_address;
         ui.add(
            SecureTextEdit::singleline(address)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         ui.label(RichText::new("Railgun Address").size(theme.typography.large));
         let address = &mut self.contact.zk_address;
         ui.add(
            SecureTextEdit::singleline(address)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         let text = RichText::new("Add").size(theme.typography.large);
         let size = vec2(100.0, 25.0);
         let button = Button::new(text).visuals(button_visuals).min_size(size);
         ui.add(button)
      });

      if res.inner.clicked() {
         let new_contact = self.contact.clone();

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let _ = match Address::from_str(&new_contact.evm_address) {
               Ok(address) => address,
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     let msg = format!("Address is not an Ethereum address: {}", e);
                     gui.open_msg_window(msg);
                     gui.request_repaint();
                  });
                  return;
               }
            };

            if !new_contact.zk_address.is_empty() {
               match RailgunAddress::from_zk_address(&new_contact.zk_address) {
                  Ok(_) => {}
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Address is not a valid Railgun address: {}", e);
                        gui.open_msg_window(msg);
                        gui.request_repaint();
                     });
                     return;
                  }
               }
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
                     gui.open_msg_window(format!(
                        "Failed to add contact: {}",
                        e.to_string()
                     ));
                     gui.request_repaint();
                  });
                  return;
               }
            }

            match ctx.save_wallet_state() {
               Ok(_) => {}
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
   }
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

   fn body(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing.y = theme.spacing.md;
      ui.spacing_mut().button_padding = theme.button_padding;

      let contact_to_delete = self.contact_to_delete.clone();
      ui.label(
         RichText::new("Are you sure you want to delete this contact?")
            .size(theme.typography.large),
      );
      ui.label(RichText::new(&contact_to_delete.name).size(theme.typography.large));
      ui.label(
         RichText::new(contact_to_delete.evm_address.to_string()).size(theme.typography.normal),
      );

      let button_visuals = theme.button_visuals();
      let text = RichText::new("Delete").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);

      if ui.add(button).clicked() {
         ctx.remove_contact(&contact_to_delete.evm_address);

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            match ctx.save_wallet_state() {
               Ok(_) => {}
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

         self.contact_to_delete = Contact::default();
         SHARED_GUI.write(|gui| {
            gui.settings.contacts_ui.view = ContactsPageView::List;
         });
      }
   }
}

struct EditContact {
   contact_to_edit: Contact,
   old_contact: Contact,
}

impl EditContact {
   fn new() -> Self {
      Self {
         contact_to_edit: Contact::default(),
         old_contact: Contact::default(),
      }
   }

   fn body(&mut self, theme: &Theme, ui: &mut Ui) {
      let res = ui.vertical_centered(|ui| {
         ui.spacing_mut().item_spacing.y = theme.spacing.lg;
         ui.spacing_mut().button_padding = theme.button_padding;
         let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);

         let text_edit_visuals = theme.text_edit_visuals();
         let button_visuals = theme.button_visuals();

         let mut contact = self.contact_to_edit.clone();
         ui.label(RichText::new("Name:").size(theme.typography.large));
         let name = &mut contact.name;

         ui.add(
            SecureTextEdit::singleline(name)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         ui.label(RichText::new("Address:").size(theme.typography.large));
         let address = &mut contact.evm_address;

         ui.add(
            SecureTextEdit::singleline(address)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         ui.label(RichText::new("Railgun Address:").size(theme.typography.large));
         let address = &mut contact.zk_address;
         ui.add(
            SecureTextEdit::singleline(address)
               .visuals(text_edit_visuals)
               .min_size(text_edit_size)
               .margin(Margin::same(10))
               .font(FontId::proportional(theme.typography.normal)),
         );

         self.contact_to_edit = contact.clone();

         let text = RichText::new("Save").size(theme.typography.large);
         let size = vec2(100.0, 25.0);
         let button = Button::new(text).visuals(button_visuals).min_size(size);

         ui.add(button)
      });

      if res.inner.clicked() {
         let old_contact = self.old_contact.clone();
         let edited_contact = self.contact_to_edit.clone();

         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            let _ = match Address::from_str(&edited_contact.evm_address) {
               Ok(address) => address,
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     let msg = format!("Address is not an Ethereum address: {}", e);
                     gui.open_msg_window(msg);
                     gui.request_repaint();
                  });
                  return;
               }
            };

            if !edited_contact.zk_address.is_empty() {
               match RailgunAddress::from_zk_address(&edited_contact.zk_address) {
                  Ok(_) => {}
                  Err(e) => {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Address is not a valid Railgun address: {}", e);
                        gui.open_msg_window(msg);
                        gui.request_repaint();
                     });
                     return;
                  }
               }
            }

            SHARED_GUI.write(|gui| {
               gui.settings.contacts_ui.edit_contact.contact_to_edit = Contact::default();
               gui.settings.contacts_ui.edit_contact.old_contact = Contact::default();
               gui.settings.contacts_ui.view = ContactsPageView::List;
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
                     gui.open_msg_window("Contact saved");
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
                     let new_contact = ws
                        .contacts
                        .iter_mut()
                        .find(|c| c.evm_address == edited_contact.evm_address);
                     if let Some(new_contact) = new_contact {
                        *new_contact = old_contact.clone();
                     }
                  });
               }
            }
         });
      }
   }
}

struct QrWindow {
   contact: Option<Contact>,
   evm_address_qr: QrImage,
   zk_address_qr: QrImage,
}

impl QrWindow {
   fn new() -> Self {
      Self {
         contact: None,
         evm_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
         zk_address_qr: QrImage::empty_with_error("No QR code found".to_string()),
      }
   }

   fn open(&mut self, contact: Contact) {
      let contact_clone = contact.clone();

      RT.spawn_blocking(move || {
         let data = contact.evm_address.clone();
         let uri = format!("bytes://contact-{}.png", &contact.evm_address);
         let evm_address_qr = QrImage::new(&data, uri);

         let zk_address_qr = if !contact.zk_address.is_empty() {
            let data = contact.zk_address.clone();
            let uri = format!("bytes://contact-{}.png", &contact.zk_address);
            QrImage::new(&data, uri)
         } else {
            QrImage::empty_with_error("No zkAddress available".to_string())
         };

         SHARED_GUI.write(|gui| {
            gui.settings.contacts_ui.qr_window.evm_address_qr = evm_address_qr;
            gui.settings.contacts_ui.qr_window.zk_address_qr = zk_address_qr;
            gui.request_repaint();
         });
      });

      self.contact = Some(contact_clone);
   }

   fn reset(&mut self) {
      *self = Self::new();
   }

   fn body(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      let privacy_mode = ctx.privacy_mode;

      ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.sm);
      ui.spacing_mut().button_padding = theme.button_padding;

      if self.contact.is_none() {
         ui.label(RichText::new("No contact found, this is a bug").size(theme.typography.normal));
         ui.add(Spinner::new().size(17.0).color(theme.colors.text));
         return;
      }

      if let Some(contact) = self.contact.as_ref() {
         ui.label(RichText::new(&contact.name).size(theme.typography.large));

         let text = match privacy_mode {
            false => "Public Address (EVM)",
            true => "Private Address (zk)",
         };

         let rich_text = RichText::new(text).size(theme.typography.large);
         ui.label(rich_text);

         let address = match privacy_mode {
            false => contact.evm_address.clone(),
            true => contact.zk_address.clone(),
         };

         if !address.is_empty() {
            let address_text = RichText::new(address.clone()).size(theme.typography.normal);
            let label =
               Button::selectable(false, address_text).visuals(theme.button_visuals()).wrap();

            if ui.add(label).clicked() {
               ui.ctx().copy_text(address);
            }
         }
      }

      ui.add_space(10.0);

      if !privacy_mode {
         if let Some(error) = self.evm_address_qr.error() {
            ui.label(RichText::new(error.to_string()).size(theme.typography.large));
         }
      } else if let Some(error) = self.zk_address_qr.error() {
         ui.label(RichText::new(error.to_string()).size(theme.typography.large));
      }

      if !privacy_mode {
         let image = self.evm_address_qr.image().fit_to_exact_size(vec2(250.0, 250.0));
         ui.add(image);
      } else {
         let image = self.zk_address_qr.image().fit_to_exact_size(vec2(250.0, 250.0));
         ui.add(image);
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
}

impl ContactsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         view: ContactsPageView::List,
         search_query: String::new(),
         add_contact: AddContact::new(overlay),
         delete_contact: DeleteContact::new(),
         edit_contact: EditContact::new(),
         qr_window: QrWindow::new(),
      }
   }

   pub fn reset_page(&mut self) {
      if self.view == ContactsPageView::Qr {
         self.qr_window.reset();
      }
      self.view = ContactsPageView::List;
      self.search_query.clear();
   }

   pub fn show_page(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      match self.view {
         ContactsPageView::List => {
            self.list_ui(ctx, theme, ui);
         }
         ContactsPageView::Add => {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Add contact").size(theme.typography.heading));
               ui.add_space(10.0);
               self.add_contact.body(theme, true, ui);
            });
         }
         ContactsPageView::Edit => {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Edit contact").size(theme.typography.heading));
               ui.add_space(10.0);
               self.edit_contact.body(theme, ui);
            });
         }
         ContactsPageView::Delete => {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Delete contact").size(theme.typography.heading));
               ui.add_space(10.0);
               self.delete_contact.body(ctx, theme, ui);
            });
         }
         ContactsPageView::Qr => {
            self.back_row(theme, ui);
            ui.vertical_centered(|ui| {
               ui.label(RichText::new("Contact QR Code").size(theme.typography.heading));
               ui.add_space(10.0);
               self.qr_window.body(ctx, theme, ui);
            });
         }
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
      if ui.add(button).clicked() {
         self.view = ContactsPageView::Add;
      }

      ui.add_space(12.0);

      if contacts.is_empty() {
         ui.label(RichText::new("No contacts found").size(theme.typography.large));
         return;
      }

      let hint = RichText::new("Search contacts or enter an address")
         .size(theme.typography.normal)
         .color(theme.colors.text_muted);

      ui.add(
         SecureTextEdit::singleline(&mut self.search_query)
            .visuals(text_edit_visuals)
            .hint_text(hint)
            .min_size(vec2(ui.available_width() * 0.40, 25.0))
            .margin(Margin::same(10))
            .font(FontId::proportional(theme.typography.normal)),
      );

      ui.add_space(15.0);

      ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
         Frame::new().inner_margin(10.0).show(ui, |ui| {
            ui.set_width(ui.available_width());

            for contact in &contacts {
               let valid = valid_contact_search(contact, &self.search_query);

               if !valid {
                  continue;
               }

               self.contact(ctx, theme, contact, ui);
            }
         });
      });
   }

   fn contact(&mut self, ctx: &ZeusContext, theme: &Theme, contact: &Contact, ui: &mut Ui) {
      let frame = theme.frame1;
      let privacy_mode = ctx.privacy_mode;
      let button_visuals = theme.button_visuals();

      frame.show(ui, |ui| {
         ui.set_width(ui.available_width());

         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               let text = RichText::new(&contact.name).size(theme.typography.large);
               let label = Label::new(text, None).wrap().interactive(false);
               ui.add(label);
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let more = dots_button(theme, ui);
               let id = format!("{}_more_options", contact.evm_address);

               Menu::new(id).show_below(&more, |ui| {
                  if ui.add(MenuItem::new("Edit").shortcut("⌘ E")).clicked() {
                     self.view = ContactsPageView::Edit;
                     self.edit_contact.contact_to_edit = contact.clone();
                     self.edit_contact.old_contact = contact.clone();
                  }

                  if ui.add(MenuItem::new("Show QR Code").shortcut("⌘ Q")).clicked() {
                     self.view = ContactsPageView::Qr;
                     self.qr_window.open(contact.clone());
                  }

                  if ui.add(MenuItem::new("Delete").shortcut("⌘ D")).clicked() {
                     self.view = ContactsPageView::Delete;
                     self.delete_contact.contact_to_delete = contact.clone();
                  }

                  if ui.add(MenuItem::new("See on Block Explorer").shortcut("⌘ S")).clicked() {
                     let chain = ctx.chain;
                     let explorer = chain.block_explorer();
                     let link = format!("{}/address/{}", explorer, &contact.evm_address);
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
               .color(theme.colors.text);

            let label = Button::selectable(false, address_text).visuals(button_visuals);

            if ui.add(label).clicked() {
               ui.ctx().copy_text(address_full);
            }
         });
      });
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
