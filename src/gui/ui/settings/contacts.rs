use crate::assets::icons::Icons;
use crate::core::{Contact, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use egui::{Align2, FontId, Frame, Margin, Order, RichText, ScrollArea, Ui, Window, vec2};
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::alloy_primitives::Address;
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, Label, SecureTextEdit};

pub struct AddContact {
   open: bool,
   overlay: OverlayManager,
   contact: Contact,
   contact_added: bool,
   size: (f32, f32),
}

impl AddContact {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         contact: Contact::default(),
         contact_added: false,
         size: (450.0, 250.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
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

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, reset_on_success: bool, ui: &mut Ui) {
      let mut open = self.open;
      if !open {
         return;
      }

      Window::new(RichText::new("Add new contact").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);
               let text_edit_visuals = theme.text_edit_visuals();
               let button_visuals = theme.button_visuals();

               ui.label(RichText::new("Name:").size(theme.text_sizes.normal));
               let name = &mut self.contact.name;
               ui.add(
                  SecureTextEdit::singleline(name)
                     .visuals(text_edit_visuals)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
               let address = &mut self.contact.address;
               ui.add(
                  SecureTextEdit::singleline(address)
                     .visuals(text_edit_visuals)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               let text = RichText::new("Add").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  let new_contact = self.contact.clone();

                  RT.spawn_blocking(move || {
                     // make sure the address is valid
                     let _ = match Address::from_str(&new_contact.address) {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(
                                 "Address is not an Ethereum address",
                                 format!("{}", e),
                              );
                           });
                           return;
                        }
                     };

                     match ctx.add_contact(new_contact.clone()) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              // gui.settings.contacts_ui.add_contact.close();
                              gui.settings.contacts_ui.add_contact.contact_added = true;
                              if reset_on_success {
                                 gui.settings.contacts_ui.add_contact.reset();
                              }
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to add contact", e.to_string());
                           });
                           return;
                        }
                     }

                     // On failure the contact is removed
                     match ctx.encrypt_and_save_vault(None, None) {
                        Ok(_) => {}
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              let error = format!(
                                 "Changes didn't take effect, encountered error: {}",
                                 e
                              );
                              gui.open_msg_window("Error while saving account data", error);
                           });
                           ctx.remove_contact(&new_contact.address);
                        }
                     }
                  });
               }
            });
         });

      if !open {
         self.close();
      }
   }
}

struct DeleteContact {
   open: bool,
   overlay: OverlayManager,
   contact_to_delete: Contact,
   size: (f32, f32),
}

impl DeleteContact {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         contact_to_delete: Contact::default(),
         size: (450.0, 180.0),
      }
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

      if !open {
         return;
      }

      let mut should_close = false;

      Window::new(RichText::new("Delete contact").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               let contact_to_delete = self.contact_to_delete.clone();
               ui.label(
                  RichText::new("Are you sure you want to delete this contact?")
                     .size(theme.text_sizes.large),
               );
               ui.label(RichText::new(&contact_to_delete.name).size(theme.text_sizes.large));
               ui.label(
                  RichText::new(contact_to_delete.address.to_string())
                     .size(theme.text_sizes.normal),
               );

               let button_visuals = theme.button_visuals();

               let text = RichText::new("Delete").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               let res_delete = ui.add(button);

               if res_delete.clicked() {
                  ctx.remove_contact(&contact_to_delete.address);

                  RT.spawn_blocking(move || {
                     // On failure the contact is added again
                     match ctx.encrypt_and_save_vault(None, None) {
                        Ok(_) => {}
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              let error = format!(
                                 "Changes didn't take effect, encountered error: {}",
                                 e
                              );
                              gui.open_msg_window("Error while saving account data", error);
                           });
                           let _res = ctx.add_contact(contact_to_delete);
                        }
                     }
                  });

                  should_close = true;
                  self.contact_to_delete = Contact::default();
               }
            });
         });

      if should_close {
         self.close();
      }

      if !open {
         self.close();
      }
   }
}

struct EditContact {
   open: bool,
   overlay: OverlayManager,
   contact_to_edit: Contact,
   old_contact: Contact,
   size: (f32, f32),
}

impl EditContact {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         contact_to_edit: Contact::default(),
         old_contact: Contact::default(),
         size: (450.0, 350.0),
      }
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

      if !open {
         return;
      }

      Window::new(RichText::new("Edit contact").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Tooltip)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);

               let text_edit_visuals = theme.text_edit_visuals();
               let button_visuals = theme.button_visuals();

               let mut contact = self.contact_to_edit.clone();
               ui.label(RichText::new("Name:").size(theme.text_sizes.normal));
               let name = &mut contact.name;

               ui.add(
                  SecureTextEdit::singleline(name)
                     .visuals(text_edit_visuals)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
               let address = &mut contact.address;

               ui.add(
                  SecureTextEdit::singleline(address)
                     .visuals(text_edit_visuals)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               self.contact_to_edit = contact.clone();

               let text = RichText::new("Save").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);

               if ui.add(button).clicked() {
                  let old_contact = self.old_contact.clone();
                  let edited_contact = self.contact_to_edit.clone();

                  RT.spawn_blocking(move || {
                     // make sure the address is valid
                     let _ = match Address::from_str(&edited_contact.address) {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(
                                 "Address is not an Ethereum address",
                                 format!("{}", e),
                              );
                           });
                           return;
                        }
                     };

                     SHARED_GUI.write(|gui| {
                        gui.settings.contacts_ui.edit_contact.contact_to_edit = Contact::default();
                        gui.settings.contacts_ui.edit_contact.old_contact = Contact::default();
                        gui.settings.contacts_ui.edit_contact.close();
                     });

                     ctx.write_vault(|vault| {
                        let new_contact =
                           vault.contacts.iter_mut().find(|c| c.address == old_contact.address);
                        if let Some(new_contact) = new_contact {
                           new_contact.name = edited_contact.name.clone();
                           new_contact.address = edited_contact.address.clone();
                        }
                     });

                     // On failure the contact changes are reverted
                     match ctx.encrypt_and_save_vault(None, None) {
                        Ok(_) => {}
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              let error = format!(
                                 "Changes didn't take effect, encountered error: {}",
                                 e
                              );
                              gui.open_msg_window("Error while saving account data", error);
                           });

                           ctx.write_vault(|vault| {
                              let new_contact = vault
                                 .contacts
                                 .iter_mut()
                                 .find(|c| c.address == edited_contact.address);
                              if let Some(new_contact) = new_contact {
                                 new_contact.name = old_contact.name.clone();
                                 new_contact.address = old_contact.address.clone();
                              }
                           });
                        }
                     }
                  });
               }
            });
         });

      if !open {
         self.close();
      }
   }
}

pub struct ContactsUi {
   open: bool,
   overlay: OverlayManager,
   main_ui: bool,
   search_query: String,
   pub add_contact: AddContact,
   delete_contact: DeleteContact,
   edit_contact: EditContact,
   pub size: (f32, f32),
}

impl ContactsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         main_ui: true,
         search_query: String::new(),
         add_contact: AddContact::new(overlay.clone()),
         delete_contact: DeleteContact::new(overlay.clone()),
         edit_contact: EditContact::new(overlay),
         size: (500.0, 550.0),
      }
   }

   pub fn open(&mut self) {
      self.overlay.window_opened();
      self.open = true;
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.main_ui(ctx.clone(), theme, ui);
      self.add_contact.show(ctx.clone(), theme, true, ui);
      self.delete_contact.show(ctx.clone(), theme, ui);
      self.edit_contact.show(ctx, theme, ui);
   }

   fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let mut open = self.open;
      Window::new(RichText::new("Contacts").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let contacts = ctx.contacts();

            let text_edit_visuals = theme.text_edit_visuals();
            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 10.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               // Add contact button
               let text = RichText::new("Add Contact").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals);
               if ui.add(button).clicked() {
                  self.add_contact.open();
               }

               ui.add_space(20.0);

               if contacts.is_empty() {
                  ui.label(RichText::new("No contacts found").size(theme.text_sizes.large));
                  return;
               }

               // Search bar
               let hint = RichText::new("Search contacts or enter an address")
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.text_muted);

               ui.add(
                  SecureTextEdit::singleline(&mut self.search_query)
                     .visuals(text_edit_visuals)
                     .hint_text(hint)
                     .min_size(vec2(ui.available_width() * 0.80, 25.0))
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.add_space(15.0);

               ScrollArea::vertical().max_height(self.size.1).show(ui, |ui| {
                  ui.set_width(self.size.0);

                  let frame = theme.frame1;
                  for contact in &contacts {
                     let valid = valid_contact_search(contact, &self.search_query);

                     if !valid {
                        continue;
                     }

                     frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        // Name
                        ui.horizontal(|ui| {
                           let text = RichText::new(&contact.name).size(theme.text_sizes.large);
                           let label = Label::new(text, None).wrap().interactive(false);
                           ui.add(label);
                        });

                        // Address
                        ui.horizontal(|ui| {
                           let chain = ctx.chain();
                           let explorer = chain.block_explorer();
                           let link = format!("{}/address/{}", explorer, &contact.address);
                           ui.hyperlink_to(
                              RichText::new(&contact.address)
                                 .size(theme.text_sizes.small)
                                 .color(theme.colors.info),
                              link,
                           );
                        });

                        let size = vec2(ui.available_width() * 0.30, 40.0);
                        ui.allocate_ui(size, |ui| {
                           ui.horizontal(|ui| {
                              let text = RichText::new("Edit").size(theme.text_sizes.normal);
                              let edit_button = Button::new(text).visuals(button_visuals);
                              if ui.add(edit_button).clicked() {
                                 self.edit_contact.open();
                                 self.edit_contact.contact_to_edit = contact.clone();
                                 self.edit_contact.old_contact = contact.clone();
                              }

                              let text = RichText::new("Delete").size(theme.text_sizes.normal);
                              let delete_button = Button::new(text).visuals(button_visuals);
                              if ui.add(delete_button).clicked() {
                                 self.delete_contact.open();
                                 self.delete_contact.contact_to_delete = contact.clone();
                              }
                           });
                        });
                     });
                     ui.add_space(5.0);
                  }
               });
            });
         });

      if !open {
         self.close();
      }
   }
}

fn valid_contact_search(contact: &Contact, query: &str) -> bool {
   let query = query.to_lowercase();

   if query.is_empty() {
      return true;
   }

   contact.name.to_lowercase().contains(&query) || contact.address.to_lowercase().contains(&query)
}
