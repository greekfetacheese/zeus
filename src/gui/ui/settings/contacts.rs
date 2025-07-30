use crate::assets::icons::Icons;
use crate::core::{ZeusCtx, user::Contact, utils::RT};
use crate::gui::SHARED_GUI;
use egui::{
   Align2, Button, FontId, Frame, Grid, Margin, Order, RichText, ScrollArea, TextEdit, Ui, Window,
   vec2,
};
use egui_theme::Theme;
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::alloy_primitives::Address;

pub struct AddContact {
   pub open: bool,
   pub contact: Contact,
   pub contact_added: bool,
   pub size: (f32, f32),
}

impl AddContact {
   pub fn new() -> Self {
      Self {
         open: false,
         contact: Contact::default(),
         contact_added: false,
         size: (450.0, 350.0),
      }
   }

   pub fn contact_added(&self) -> bool {
      self.contact_added
   }

   pub fn reset(&mut self) {
      self.open = false;
      self.contact_added = false;
      self.contact = Contact::default();
   }

   pub fn get_contact(&self) -> &Contact {
      &self.contact
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, reset_on_success: bool, ui: &mut Ui) {
      let mut open = self.open;

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

               ui.label(RichText::new("Name:").size(theme.text_sizes.normal));
               let name = &mut self.contact.name;
               ui.add(
                  TextEdit::singleline(name)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
               let address = &mut self.contact.address;
               ui.add(
                  TextEdit::singleline(address)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               if ui
                  .add(Button::new(
                     RichText::new("Add").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
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
                              gui.settings.contacts_ui.add_contact.open = false;
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
                     match ctx.encrypt_and_save_account(None, None) {
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
      self.open = open;
   }
}

pub struct DeleteContact {
   pub open: bool,
   pub contact_to_delete: Contact,
   pub size: (f32, f32),
}

impl DeleteContact {
   pub fn new() -> Self {
      Self {
         open: false,
         contact_to_delete: Contact::default(),
         size: (450.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

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
               ui.label(RichText::new(&contact_to_delete.name).size(theme.text_sizes.normal));
               ui.label(
                  RichText::new(contact_to_delete.address.to_string())
                     .size(theme.text_sizes.normal),
               );

               let res_delete = ui.add(Button::new(
                  RichText::new("Delete").size(theme.text_sizes.normal),
               ));

               if res_delete.clicked() {
                  ctx.remove_contact(&contact_to_delete.address);

                  RT.spawn_blocking(move || {
                     // On failure the contact is added again
                     match ctx.encrypt_and_save_account(None, None) {
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
         self.contact_to_delete = Contact::default();
         self.open = false;
      } else {
         self.open = open;
      }
   }
}

pub struct EditContact {
   pub open: bool,
   pub contact_to_edit: Contact,
   pub old_contact: Contact,
   pub size: (f32, f32),
}

impl EditContact {
   pub fn new() -> Self {
      Self {
         open: false,
         contact_to_edit: Contact::default(),
         old_contact: Contact::default(),
         size: (450.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let mut open = self.open;

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

               let mut contact = self.contact_to_edit.clone();
               ui.label(RichText::new("Name:").size(theme.text_sizes.normal));
               let name = &mut contact.name;
               ui.add(
                  TextEdit::singleline(name)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
               let address = &mut contact.address;
               ui.add(
                  TextEdit::singleline(address)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               self.contact_to_edit = contact.clone();

               if ui
                  .add(Button::new(
                     RichText::new("Save").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
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
                        gui.settings.contacts_ui.edit_contact.open = false;
                     });

                     ctx.write_account(|account| {
                        let new_contact = account
                           .contacts
                           .iter_mut()
                           .find(|c| c.address == old_contact.address);
                        if let Some(new_contact) = new_contact {
                           new_contact.name = edited_contact.name.clone();
                           new_contact.address = edited_contact.address.clone();
                        }
                     });

                     // On failure the contact changes are reverted
                     match ctx.encrypt_and_save_account(None, None) {
                        Ok(_) => {}
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              let error = format!(
                                 "Changes didn't take effect, encountered error: {}",
                                 e
                              );
                              gui.open_msg_window("Error while saving account data", error);
                           });

                           ctx.write_account(|account| {
                              let new_contact = account
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

      self.open = open;
   }
}

pub struct ContactsUi {
   pub open: bool,
   pub main_ui: bool,
   pub add_contact: AddContact,
   pub delete_contact: DeleteContact,
   pub edit_contact: EditContact,
   pub size: (f32, f32),
}

impl ContactsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         main_ui: true,
         add_contact: AddContact::new(),
         delete_contact: DeleteContact::new(),
         edit_contact: EditContact::new(),
         size: (500.0, 350.0),
      }
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

   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
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
         .frame(Frame::window(ui.style()).inner_margin(Margin::same(10)))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let contacts = ctx.contacts();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 10.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               // Add contact button
               if ui
                  .add(Button::new(
                     RichText::new("Add Contact").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  self.add_contact.open = true;
               }

               ui.add_space(30.0);

               if contacts.is_empty() {
                  ui.label(RichText::new("No contacts found").size(theme.text_sizes.large));
               } else {
                  ScrollArea::vertical()
                     .max_height(self.size.1)
                     .show(ui, |ui| {
                        ui.set_width(self.size.0);
                        let ui_width = ui.available_width();

                        Grid::new("contact_ui_grid")
                           .spacing(vec2(20.0, 20.0))
                           .show(ui, |ui| {
                              for contact in &contacts {
                                 // Name
                                 let name =
                                    RichText::new(&contact.name).size(theme.text_sizes.normal);
                                 let button = Button::new(name).truncate();
                                 ui.scope(|ui| {
                                    ui.set_width(ui_width * 0.3);
                                    ui.add(button);
                                 });

                                 // Address
                                 let chain = ctx.chain();
                                 let explorer = chain.block_explorer();
                                 let link = format!("{}/address/{}", explorer, &contact.address);
                                 ui.scope(|ui| {
                                    ui.set_width(ui_width * 0.4);
                                    ui.hyperlink_to(
                                       RichText::new(contact.address_short(10, 10))
                                          .size(theme.text_sizes.normal)
                                          .color(theme.colors.hyperlink_color),
                                       link,
                                    );
                                 });

                                 ui.scope(|ui| {
                                    ui.set_width(ui_width * 0.3);
                                    if ui
                                       .add(Button::new(
                                          RichText::new("Delete").size(theme.text_sizes.small),
                                       ))
                                       .clicked()
                                    {
                                       self.delete_contact.open = true;
                                       self.delete_contact.contact_to_delete = contact.clone();
                                    }

                                    if ui
                                       .add(Button::new(
                                          RichText::new("Edit").size(theme.text_sizes.small),
                                       ))
                                       .clicked()
                                    {
                                       self.edit_contact.open = true;
                                       self.edit_contact.contact_to_edit = contact.clone();
                                       self.edit_contact.old_contact = contact.clone();
                                    }
                                 });
                                 ui.end_row();
                              }
                           });
                     });
               }
            });
         });
      self.open = open;
   }
}
