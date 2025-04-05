use crate::assets::icons::Icons;
use crate::core::{Contact, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use egui::{
   Align, Align2, Button, Color32, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea, TextEdit, Ui, Window,
   vec2,
};
use egui_theme::{Theme, utils::*};
use std::str::FromStr;
use std::sync::Arc;
use zeus_eth::alloy_primitives::Address;

pub struct ContactsUi {
   pub open: bool,
   pub main_ui: bool,
   pub add_contact: bool,
   pub delete_contact: bool,
   pub edit_contact: bool,
   pub contact_to_add: Contact,
   pub contact_to_delete: Option<Contact>,
   pub contact_to_edit: Option<Contact>,
   pub old_contact: Option<Contact>,
   pub size: (f32, f32),
}

impl ContactsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         main_ui: true,
         add_contact: false,
         delete_contact: false,
         edit_contact: false,
         contact_to_add: Contact::default(),
         contact_to_delete: None,
         contact_to_edit: None,
         old_contact: None,
         size: (500.0, 400.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, parent_open: &mut bool, ui: &mut Ui) {
      if !self.open {
         // reopen the settings main ui
         *parent_open = true;
         return;
      }

      self.main_ui(ctx.clone(), theme, icons.clone(), ui);
      self.add_contact_ui(ctx.clone(), theme, icons.clone(), ui);
      self.delete_contact_ui(ctx.clone(), theme, ui);
      self.edit_contact_ui(ctx, theme, icons, ui);
   }

   pub fn main_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.main_ui {
         return;
      }

      let mut open = self.open;
      Window::new(RichText::new("Contacts").size(theme.text_sizes.heading))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
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
                  self.add_contact = true;
                  self.main_ui = false;
               }

               if contacts.is_empty() {
                  ui.label(RichText::new("No contacts found").size(theme.text_sizes.large));
               } else {
                  ScrollArea::vertical().show(ui, |ui| {
                     ui.set_width(self.size.0);
                     ui.vertical_centered(|ui| {
                        for contact in &contacts {
                           Frame::group(ui.style()).inner_margin(8.0).show(ui, |ui| {
                              ui.set_width(250.0);
                              self.contact(theme, icons.clone(), contact, ui);
                           });
                        }
                     });
                  });
               }
            });
         });
      self.open = open;
   }

   /// Show a contact
   fn contact(&mut self, theme: &Theme, icons: Arc<Icons>, contact: &Contact, ui: &mut Ui) {
      ui.horizontal(|ui| {
         // Contact info column
         ui.vertical(|ui| {
            ui.set_width(ui.available_width() - 40.0); // Leave space for buttons

            // Name
            let name_label = Label::new(RichText::new(&contact.name).size(theme.text_sizes.normal)).wrap();
            ui.add(name_label);

            // Address
            let address = contact.address_short();
            if ui
               .selectable_label(false, RichText::new(&address).size(theme.text_sizes.normal))
               .clicked()
            {
               ui.ctx().copy_text(contact.address.clone());
            }
         });

         // Buttons column
         ui.vertical(|ui| {
            ui.set_min_width(40.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let delete_res = ui.add(Button::image_and_text(icons.trash(), "").min_size((16.0, 16.0).into()));
               let edit_res = ui.add(Button::image_and_text(icons.edit(), "").min_size((16.0, 16.0).into()));

               if delete_res.clicked() {
                  self.main_ui = false;
                  self.delete_contact = true;
                  self.contact_to_delete = Some(contact.clone());
               }
               if edit_res.clicked() {
                  self.main_ui = false;
                  self.edit_contact = true;
                  self.contact_to_edit = Some(contact.clone());
                  self.old_contact = Some(contact.clone());
               }
            });
         });
      });
   }

   fn delete_contact_ui(&mut self, ctx: ZeusCtx, theme: &Theme, ui: &mut Ui) {
      if !self.delete_contact {
         return;
      }

      Window::new(RichText::new("Delete contact").size(theme.text_sizes.heading))
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               // should not happen
               if self.contact_to_delete.is_none() {
                  ui.label(RichText::new("No contact to delete"));
                  if ui.add(Button::new(RichText::new("Close"))).clicked() {
                     self.delete_contact = false;
                     return;
                  }
               }

               let contact = self.contact_to_delete.clone().unwrap();
               ui.label(RichText::new("Are you sure you want to delete this contact?").size(theme.text_sizes.large));
               ui.label(RichText::new(&contact.name).size(theme.text_sizes.normal));
               ui.label(RichText::new(&contact.address_short()).size(theme.text_sizes.normal));

               let res_delete = ui.add(Button::new(
                  RichText::new("Delete").size(theme.text_sizes.normal),
               ));
               let res_cancel = ui.add(Button::new(
                  RichText::new("Cancel").size(theme.text_sizes.normal),
               ));

               if res_cancel.clicked() {
                  self.delete_contact = false;
                  self.main_ui = true;
                  self.contact_to_delete = None;
               }

               if res_delete.clicked() {
                  ctx.write(|ctx| {
                     ctx.contact_db.remove_contact(contact.address);
                  });
                  RT.spawn_blocking(move || {
                     ctx.save_contact_db();
                  });
                  self.delete_contact = false;
                  self.main_ui = true;
                  self.contact_to_delete = None;
               }
            });
         });
   }

   fn add_contact_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.add_contact {
         return;
      }

      Window::new(RichText::new("Add new contact").size(theme.text_sizes.heading))
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);

               // Go back button
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let button = Button::image_and_text(icons.arrow_left(), "").min_size(vec2(30.0, 20.0));
                  if ui.add(button).clicked() {
                     self.add_contact = false;
                     self.main_ui = true;
                  }
               });

               ui.label(RichText::new("Name:").size(theme.text_sizes.normal));
               let name = &mut self.contact_to_add.name;
               ui.add(
                  TextEdit::singleline(name)
                     .min_size(text_edit_size)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(theme.text_sizes.normal)),
               );

               ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
               let address = &mut self.contact_to_add.address;
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
                  let contact = self.contact_to_add.clone();

                  RT.spawn_blocking(move || {
                     // make sure the address is valid
                     let _ = match Address::from_str(&contact.address) {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Address is not an Ethereum address", &format!("{}", e));
                           });
                           return;
                        }
                     };

                     ctx.write(|ctx| match ctx.contact_db.add_contact(contact) {
                        Ok(_) => {
                           SHARED_GUI.write(|gui| {
                              gui.settings.contacts_ui.contact_to_add = Contact::default();
                              gui.settings.contacts_ui.add_contact = false;
                              gui.settings.contacts_ui.main_ui = true;
                           });
                        }
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Failed to add contact", &format!("{}", e));
                           });
                           return;
                        }
                     });

                     ctx.save_contact_db();
                  });
               }
            });
         });
   }

   fn edit_contact_ui(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.edit_contact {
         return;
      }

      Window::new("Edit contact")
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = 20.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               let text_edit_size = vec2(ui.available_width() * 0.6, 25.0);

               // Go back button
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let button = Button::image_and_text(icons.arrow_left(), "").min_size(vec2(30.0, 20.0));
                  bg_color_on_idle(ui, Color32::TRANSPARENT);
                  if ui.add(button).clicked() {
                     self.edit_contact = false;
                     self.main_ui = true;
                  }
               });

               // should not happen
               if self.contact_to_edit.is_none() {
                  ui.label(RichText::new("No contact to edit"));
                  if ui.add(Button::new(RichText::new("Close"))).clicked() {
                     self.edit_contact = false;
                     return;
                  }
               }

               let mut contact = self.contact_to_edit.clone().unwrap();
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

               self.contact_to_edit = Some(contact.clone());

               if ui
                  .add(Button::new(
                     RichText::new("Save").size(theme.text_sizes.normal),
                  ))
                  .clicked()
               {
                  let old_contact = self.old_contact.clone().unwrap();
                  let new_contact = self.contact_to_edit.clone().unwrap();

                  RT.spawn_blocking(move || {
                     // make sure the address is valid
                     let _ = match Address::from_str(&contact.address) {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window("Address is not an Ethereum address", &format!("{}", e));
                           });
                           return;
                        }
                     };

                     ctx.write(|ctx| {
                        ctx.contact_db.remove_contact(old_contact.address.clone());
                        match ctx.contact_db.add_contact(new_contact) {
                           Ok(_) => SHARED_GUI.write(|gui| {
                              gui.settings.contacts_ui.contact_to_edit = None;
                              gui.settings.contacts_ui.edit_contact = false;
                              gui.settings.contacts_ui.main_ui = true;
                              gui.loading_window.open = false;
                           }),
                           Err(e) => {
                              SHARED_GUI.write(|gui| {
                                 gui.open_msg_window("Failed to add contact", &format!("{}", e));
                              });
                              return;
                           }
                        }
                     });
                     ctx.save_contact_db();
                  });
               }
            });
         });
   }
}
