use crate::assets::icons::Icons;
use crate::core::{Contact, ZeusCtx, utils::RT};
use crate::gui::SHARED_GUI;
use egui::{
   Align, Align2, Button, FontId, Frame, Label, Layout, Margin, Order, RichText, ScrollArea,
   TextEdit, Ui, Window, vec2,
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

   /// `size`: Optional pass a sliglthy bigger size if this is shown on top of another window
   ///
   /// So we dont lose it if click on window behind it
   pub fn show(
      &mut self,
      ctx: ZeusCtx,
      theme: &Theme,
      size: Option<(f32, f32)>,
      reset_on_success: bool,
      ui: &mut Ui,
   ) {
      let mut open = self.open;

      let (width, height) = size.unwrap_or((self.size.0, self.size.1));

      Window::new(RichText::new("Add new contact").size(theme.text_sizes.large))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);

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
                  let contact = self.contact.clone();

                  RT.spawn_blocking(move || {
                     // make sure the address is valid
                     let _ = match Address::from_str(&contact.address) {
                        Ok(address) => address,
                        Err(e) => {
                           SHARED_GUI.write(|gui| {
                              gui.open_msg_window(
                                 "Address is not an Ethereum address",
                                 &format!("{}", e),
                              );
                           });
                           return;
                        }
                     };

                     let mut ok: (bool, String) = (false, String::new());
                     ctx.write(|ctx| match ctx.contact_db.add_contact(contact) {
                        Ok(_) => {
                           ok = (true, String::new());
                        }
                        Err(e) => {
                           ok = (false, format!("{}", e));
                        }
                     });

                     if ok.0 {
                        SHARED_GUI.write(|gui| {
                           gui.settings.contacts_ui.add_contact.open = false;
                           gui.settings.contacts_ui.add_contact.contact_added = true;
                           if reset_on_success {
                              gui.settings.contacts_ui.add_contact.reset();
                           }
                        });
                     } else {
                        SHARED_GUI.write(|gui| {
                           gui.open_msg_window("Failed to add contact", ok.1);
                        });
                        return;
                     }

                     ctx.save_contact_db();
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

   /// `size`: Optional pass a sliglthy bigger size if this is shown on top of another window
   ///
   /// So we dont lose it if click on window behind it
   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, size: Option<(f32, f32)>, ui: &mut Ui) {
      let mut open = self.open;

      let mut should_close = false;
      let (width, height) = size.unwrap_or((self.size.0, self.size.1));

      Window::new(RichText::new("Delete contact").size(theme.text_sizes.large))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);

            ui.vertical_centered(|ui| {
               ui.set_width(self.size.0);
               ui.spacing_mut().item_spacing.y = 15.0;
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);
               ui.add_space(20.0);

               let contact = self.contact_to_delete.clone();
               ui.label(
                  RichText::new("Are you sure you want to delete this contact?")
                     .size(theme.text_sizes.large),
               );
               ui.label(RichText::new(&contact.name).size(theme.text_sizes.normal));
               ui.label(RichText::new(&contact.address).size(theme.text_sizes.normal));

               let res_delete = ui.add(Button::new(
                  RichText::new("Delete").size(theme.text_sizes.normal),
               ));

               if res_delete.clicked() {
                  ctx.write(|ctx| {
                     ctx.contact_db.remove_contact(contact.address);
                  });
                  RT.spawn_blocking(move || {
                     ctx.save_contact_db();
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

   /// `size`: Optional pass a sliglthy bigger size if this is shown on top of another window
   ///
   /// So we dont lose it if click on window behind it
   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, size: Option<(f32, f32)>, ui: &mut Ui) {
      let mut open = self.open;
      let (width, height) = size.unwrap_or((self.size.0, self.size.1));

      Window::new(RichText::new("Edit contact").size(theme.text_sizes.large))
         .open(&mut open)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, (0.0, 0.0))
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(width);
            ui.set_height(height);

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
                                 &format!("{}", e),
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

                     ctx.write(|ctx| {
                        let new_contact = ctx.contact_db.contact_mut(&old_contact);
                        if let Some(new_contact) = new_contact {
                           new_contact.name = edited_contact.name.clone();
                           new_contact.address = edited_contact.address.clone();
                        }
                     });
                     ctx.save_contact_db();
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
         size: (450.0, 350.0),
      }
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, _icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let window_size = Some((self.size.0 + 10.0, self.size.1 + 10.0));

      self.main_ui(ctx.clone(), theme, ui);
      self
         .add_contact
         .show(ctx.clone(), theme, window_size, true, ui);
      self
         .delete_contact
         .show(ctx.clone(), theme, window_size, ui);
      self.edit_contact.show(ctx, theme, window_size, ui);
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
                  self.add_contact.open = true;
               }

               if contacts.is_empty() {
                  ui.label(RichText::new("No contacts found").size(theme.text_sizes.large));
               } else {
                  ScrollArea::vertical().show(ui, |ui| {
                     ui.set_width(self.size.0);
                     ui.vertical_centered(|ui| {
                        for contact in &contacts {
                           Frame::group(ui.style()).inner_margin(8.0).show(ui, |ui| {
                              ui.set_width(300.0); // Slightly wider for better balance
                              self.contact(theme, contact, ui);
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
   fn contact(&mut self, theme: &Theme, contact: &Contact, ui: &mut Ui) {
      ui.horizontal(|ui| {
         // Contact info column (name + address)
         ui.vertical(|ui| {
            ui.set_max_width(200.0);

            // Name
            let name_label =
               Label::new(RichText::new(&contact.name).size(theme.text_sizes.normal)).wrap();
            ui.add(name_label);

            // Address
            let address = contact.address_short();
            if ui
               .selectable_label(
                  false,
                  RichText::new(&address).size(theme.text_sizes.normal),
               )
               .clicked()
            {
               ui.ctx().copy_text(contact.address.clone());
            }
         });

         ui.add_space(ui.available_width() - 80.0);

         // Buttons column
         ui.vertical(|ui| {
            ui.set_min_width(80.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
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
         });
      });
   }
}
