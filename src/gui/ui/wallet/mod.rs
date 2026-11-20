use crate::assets::icons::Icons;
use crate::core::{WalletInfo, WalletValue, ZeusContext};
use crate::gui::ui::common::{address_for_speech, chain_label, clickable_row, spoken_wallet_name};
use crate::gui::{
   SHARED_GUI, dots_button,
   ui::{WalletListByValue, show_with_fade, switch_wallet},
};
use eframe::egui::{
   Align, Context, FontId, Id, Layout, Margin, Order, RichText, Role, ScrollArea, Ui, Vec2,
   WidgetInfo, vec2,
};
use egui_elements::Modal;
use egui_elements::{
   Button, ChipRole, Frame, Label, Menu, MenuItem, SecureTextEdit, Spinner, Theme, Toast,
};
use std::time::Duration;
use std::{collections::HashMap, sync::Arc};
use zeus_eth::alloy_primitives::Address;

pub mod add;
pub mod delete;
pub mod discover;
pub mod export;
pub mod import;

pub use add::AddWalletUi;
pub use delete::DeleteWalletUi;
pub use export::ExportKeyUi;

/// The `dots_button` in a wallet card's top row.
const MORE_BUTTON_SIZE: Vec2 = Vec2::new(28.0, 20.0);

/// Ui to manage the wallets
pub struct WalletUi {
   open: bool,
   loading: bool,
   rename_wallet: bool,
   new_wallet_name: String,
   wallet_to_rename: Option<WalletInfo>,
   /// Whether the rename field still has to take the focus the dialog's Close button is given when
   /// it opens.
   rename_focus_field: bool,
   pub add_wallet_ui: AddWalletUi,
   search_query: String,
   export_key_ui: ExportKeyUi,
   delete_wallet_ui: DeleteWalletUi,
   wallets: Vec<WalletInfo>,
   /// The rows the list paints: [`Self::wallets`] without the current wallet, matched against the
   /// search query. Built off the frame path — by [`Self::calc_wallet_value`] and on a query or
   /// wallet change — because walking every wallet on every frame is what a large vault cannot
   /// afford.
   visible_wallets: Vec<WalletInfo>,
   /// The `(search query, current wallet)` [`Self::visible_wallets`] was built for.
   visible_key: Option<(String, Address)>,
   /// Wallet value by address
   wallet_value: HashMap<Address, WalletValue>,
   /// Chains that the wallet has balance on
   wallet_chains: HashMap<Address, Vec<u64>>,
   /// The one sentence the page wants said this frame. A page cannot announce from inside its own
   /// draw, so the panel drains this after the page is drawn.
   notice: Option<String>,
   /// The last count the page said, so a count that did not change is not repeated.
   spoken_count: Option<String>,
   size: (f32, f32),
}

impl WalletUi {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         rename_wallet: false,
         new_wallet_name: String::new(),
         wallet_to_rename: None,
         rename_focus_field: false,
         add_wallet_ui: AddWalletUi::new(),
         search_query: String::new(),
         export_key_ui: ExportKeyUi::new(),
         delete_wallet_ui: DeleteWalletUi::new(),
         wallets: Vec::new(),
         visible_wallets: Vec::new(),
         visible_key: None,
         wallet_value: HashMap::new(),
         wallet_chains: HashMap::new(),
         notice: None,
         spoken_count: None,
         size: (550.0, 600.0),
      }
   }

   pub fn erase(&mut self, ctx: &Context) {
      self.export_key_ui.erase(ctx);
      self.delete_wallet_ui.erase();
      self.add_wallet_ui.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open_rename_wallet(&mut self, wallet: Option<WalletInfo>) {
      // The field opens on the name in use, so a reader can hear what is there and change it
      // instead of retyping it from memory.
      self.new_wallet_name =
         wallet.as_ref().map(|wallet| wallet.name().to_string()).unwrap_or_default();

      self.rename_wallet = true;
      self.wallet_to_rename = wallet;
      self.rename_focus_field = true;
   }

   pub fn close_rename_wallet(&mut self) {
      self.rename_wallet = false;
      self.wallet_to_rename = None;
      self.new_wallet_name.clear();
      self.rename_focus_field = false;
   }

   pub fn open(&mut self) {
      self.open = true;
      self.calc_wallet_value();
   }

   pub fn calc_wallet_value(&mut self) {
      self.loading = true;
      on_calc_wallet_value();
   }

   /// Rebuild the rows the list paints for the current wallet and search query.
   ///
   /// Called when the load finishes and whenever the query or the current wallet changes — never
   /// from the frame path itself, which only walks the rows it actually paints.
   fn refresh_visible_wallets(&mut self, current_wallet: &WalletInfo) {
      let query = self.search_query.to_lowercase();

      self.visible_wallets = self
         .wallets
         .iter()
         .filter(|wallet| *wallet != current_wallet)
         .filter(|wallet| query.is_empty() || wallet.name().to_lowercase().contains(&query))
         .cloned()
         .collect();

      self.visible_key = Some((self.search_query.clone(), current_wallet.address));
   }

   pub fn close(&mut self) {
      self.open = false;
      // How many wallets the page holds is said when it arrives, so the last count is forgotten
      // with the page rather than being compared against on the next visit.
      self.spoken_count = None;
   }

   /// The sentence the page wants said, for the frame's announcement channel.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Close the wallet UI and drop the secrets it is holding.
   ///
   /// Called when Zeus auto-locks, so neither a private key shown before the lock nor a seed half
   /// typed into the import form can resurface after unlock.
   pub fn lock(&mut self, ctx: &Context) {
      self.close();
      self.close_rename_wallet();
      // A sentence raised before the lock is not about the page the user comes back to.
      self.notice = None;
      self.export_key_ui.lock(ctx);
      // The delete form carries a verified-credentials flag that authorizes the
      // delete; it must not survive the lock.
      self.delete_wallet_ui.lock();
      // The import form holds the key or seed phrase as it is typed, and `erase` runs only at
      // shutdown — without this the seed is still in the form when the wallet comes back.
      self.add_wallet_ui.erase();
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      show_with_fade(ui, "wallet_ui_fade", self.open, |ui| {
         self.main_ui(ctx, theme, icons.clone(), ui);
      });

      self.rename_wallet(theme, ui);
      self.add_wallet_ui.show(ctx, theme, icons.clone(), ui);

      // The add-wallet dialog draws after the page, so its sentence is the last one raised here.
      if let Some(notice) = self.add_wallet_ui.take_notice() {
         self.notice = Some(notice);
      }

      self.export_key_ui.show(ctx, theme, ui);

      // The export dialog draws after the page, so its sentence is the last one raised this frame
      // and the one the announcement channel keeps: what the dialog just did replaces whatever the
      // page said above.
      if let Some(notice) = self.export_key_ui.take_notice() {
         self.notice = Some(notice);
      }

      self.delete_wallet_ui.show(ctx, theme, ui);

      // The delete dialog draws after the export dialog, so its sentence is the last one raised
      // this frame and the one the announcement channel keeps.
      if let Some(notice) = self.delete_wallet_ui.take_notice() {
         self.notice = Some(notice);
      }
   }

   /// This is the first Ui we show to the user when this [WalletUi] is open.
   ///
   /// We can see, manage and add new wallets.
   fn main_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let frame = theme.frame1;

      ui.vertical_centered(|ui| {
         frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            let content_width = ui.available_width() * 0.9;

            let normal = theme.typography.normal;
            let large = theme.typography.large;

            ui.vertical_centered(|ui| {
               let text = RichText::new("Add Wallet").size(large);
               let button = Button::new(text).min_size(vec2(content_width, 45.0));

               if ui.add(button).clicked() {
                  self.add_wallet_ui.open();
               }

               ui.add_space(10.0);

               let current_wallet = ctx.current_wallet_info();

               ui.label(RichText::new("Selected Wallet").size(large));
               self.wallet(
                  ctx,
                  theme,
                  icons.clone(),
                  &current_wallet,
                  content_width,
                  ui,
               );

               ui.add_space(10.0);

               let hint = RichText::new("Search...").color(theme.colors.text_muted).size(normal);

               let search = ui.allocate_ui(vec2(content_width, 45.0), |ui| {
                  SecureTextEdit::singleline(&mut self.search_query)
                     .hint_text(hint)
                     .margin(Margin::same(10))
                     .font(FontId::proportional(normal))
                     .desired_width(ui.available_width())
                     .show(ui)
               });

               // The field draws its hint and nothing else, so it is named for what it narrows.
               ui.ctx().accesskit_node_builder(search.inner.response.id, |node| {
                  node.set_label("Search wallets");
               });

               if self.loading {
                  let spinner = ui.add(Spinner::new().size(17.0));
                  spinner.widget_info(|| {
                     WidgetInfo::labeled(
                        Role::ProgressIndicator,
                        ui.is_enabled(),
                        "Loading wallet values",
                     )
                  });
                  return;
               }

               // The rows are built by the loader and on a query or wallet change — not here. The
               // wallet Zeus is on already has the card above the search field, so the list holds
               // the rest.
               let stale = !matches!(
                  &self.visible_key,
                  Some((cached, address))
                     if *address == current_wallet.address && cached == &self.search_query
               );

               if stale {
                  self.refresh_visible_wallets(&current_wallet);
               }

               // The answer to the search is said when it changes, and never while the values are
               // loading: the rows below this point are the ones the count is about.
               let count = count_sentence(self.visible_wallets.len());

               if self.spoken_count.as_deref() != Some(count.as_str()) {
                  self.spoken_count = Some(count.clone());
                  self.notice = Some(count);
               }

               let row_height = 80.0;

               ui.spacing_mut().item_spacing.y = theme.spacing.md;

               ScrollArea::vertical().content_margin(5).auto_shrink([false; 2]).show_rows(
                  ui,
                  row_height,
                  self.visible_wallets.len(),
                  |ui, row_range| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.md;

                     for index in row_range {
                        // Only the painted rows are cloned: a row takes `&mut self`, so the cache
                        // cannot be borrowed across the call.
                        let Some(wallet) = self.visible_wallets.get(index).cloned() else {
                           continue;
                        };

                        if self.wallet(
                           ctx,
                           theme,
                           icons.clone(),
                           &wallet,
                           content_width,
                           ui,
                        ) {
                           self.notice = Some(format!(
                              "Switched to {}.",
                              spoken_wallet_name(&wallet)
                           ));
                           switch_wallet(ctx, wallet);
                        }
                     }
                  },
               );
            });
         });
      });
   }

   /// Show a wallet card.
   ///
   /// The card takes clicks only while clicking it would change the wallet: the selected wallet's
   /// card, above the search field, is the one Zeus is already on.
   ///
   /// Returns true when the card was clicked.
   fn wallet(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: &WalletInfo,
      width: f32,
      ui: &mut Ui,
   ) -> bool {
      let inner = theme.inner_margin;
      let corner = theme.corner_radius;
      let current = ctx.is_current_wallet(wallet.address);

      let frame = Frame::new()
         .interactive(!current)
         .inner_margin(inner)
         .corner_radius(corner)
         .visuals(theme.frame2_visuals());

      // The card says its name and its value once, so both are built before the frame draws: the
      // two labels that put them on screen are hidden below.
      let empty = WalletValue::default();
      let value = self.wallet_value.get(&wallet.address).unwrap_or(&empty);

      let value_fmt = if ctx.privacy_mode {
         format!("${:.10}", value.private.abbreviated())
      } else {
         format!("${:.10}", value.public.abbreviated())
      };

      let spoken = spoken_wallet_name(wallet);
      let name = format!("{spoken}, {value_fmt}");

      // The chain icons are not a sentence on their own, so the card says what they mean.
      let chains: Vec<String> = self
         .wallet_chains
         .get(&wallet.address)
         .map(|chains| {
            chains
               .iter()
               .filter(|chain| !ctx.is_chain_disabled(**chain))
               .map(|chain| chain_label(*chain))
               .collect()
         })
         .unwrap_or_default();

      let description = (!chains.is_empty()).then(|| format!("On {}", chains.join(", ")));

      if current {
         let card = frame.show(ui, |ui| {
            self.wallet_body(
               ctx,
               theme,
               icons.clone(),
               wallet,
               width,
               &spoken,
               &value_fmt,
               ui,
            )
         });

         card
            .response
            .widget_info(|| WidgetInfo::labeled(Role::Group, ui.is_enabled(), &name));

         if let Some(description) = &description {
            ui.ctx().accesskit_node_builder(card.response.id, |node| {
               node.set_description(description.as_str());
            });
         }

         for covered in card.inner {
            ui.ctx().accesskit_node_builder(covered, |node| node.set_hidden());
         }

         return false;
      }

      // A row here is never the selected one: the list holds the wallets Zeus is not on — the one
      // it is on is the card above the search field — so there is no selection to report.
      clickable_row(ui, frame, name, description, false, |ui| {
         self.wallet_body(
            ctx,
            theme,
            icons.clone(),
            wallet,
            width,
            &spoken,
            &value_fmt,
            ui,
         )
      })
      .clicked()
   }

   /// The card's contents: the options button, the name, the address, the value and the chains the
   /// wallet holds something on.
   ///
   /// Returns the ids of the labels whose words the card's own name already says.
   fn wallet_body(
      &mut self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      wallet: &WalletInfo,
      width: f32,
      spoken: &str,
      value_fmt: &str,
      ui: &mut Ui,
   ) -> Vec<Id> {
      let tint = theme.image_tint_recommended;
      let small = theme.typography.small;
      let normal = theme.typography.normal;

      let mut covered = Vec::new();

      ui.set_width(width);
      ui.spacing_mut().button_padding = theme.button_padding;

      ui.horizontal(|ui| {
         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let id = format!("{}_more_options", wallet.address);
            let more = dots_button(theme, MORE_BUTTON_SIZE, ui);
            let enabled = !wallet.is_master();

            let options = format!("Options for {spoken}");
            more.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), &options));

            Menu::new(id).show_below(&more, |ui| {
               if ui.add(MenuItem::new("Export key")).clicked() {
                  self.export_key_ui.open(wallet.clone());
               }

               if ui.add(MenuItem::new("Rename")).clicked() {
                  self.open_rename_wallet(Some(wallet.clone()));
               }

               if ui.add(MenuItem::new("Show address QR code")).clicked() {
                  on_show_wallet_qr(wallet.clone());
               }

               let delete = ui.add_enabled(enabled, MenuItem::new("Delete"));

               // The one wallet Zeus will not delete says why where a reader can reach it.
               if !enabled {
                  ui.ctx().accesskit_node_builder(delete.id, |node| {
                     node.set_description("The master wallet cannot be deleted.");
                  });
               }

               if delete.clicked() {
                  self.delete_wallet_ui.open(wallet.clone());
               }
            });

            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
               let text = RichText::new(wallet.name_with_source()).size(normal);
               let label = Label::new(text, None).interactive(false);
               covered.push(ui.add(label).id);
            });
         });
      });

      ui.horizontal(|ui| {
         let text = RichText::new(wallet.evm_address_truncated()).size(small);
         let label = Button::selectable(false, text);
         let copy = ui.add(label);

         // The drawn address keeps both of its ends; what is copied, and what a reader hears, is
         // the whole of it.
         let copy_name = format!("Copy address of {spoken}");
         let address = address_for_speech(None, &wallet.address.to_string());

         copy.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), &copy_name));
         ui.ctx().accesskit_node_builder(copy.id, |node| {
            node.set_description(address.name.as_str());
         });

         if copy.clicked() {
            ui.ctx().copy_text(wallet.address.to_string());
            self.notice = Some("Address copied.".to_string());
         }

         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;

            let value_text = RichText::new(value_fmt).size(small);
            let label = Label::new(value_text, None).interactive(false);
            covered.push(ui.add(label).id);

            let chains = self.wallet_chains.get(&wallet.address).cloned().unwrap_or_default();

            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.xs;
               for chain in chains {
                  if ctx.is_chain_disabled(chain) {
                     continue;
                  }

                  let image = icons.chain_icon(chain, tint).fit_to_exact_size(vec2(16.0, 16.0));
                  let icon = ui.add(image);

                  let label = chain_label(chain);
                  icon.widget_info(|| WidgetInfo::labeled(Role::Image, ui.is_enabled(), &label));
               }
            });
         });
      });

      covered
   }

   /// Rename wallet UI
   fn rename_wallet(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.rename_wallet {
         return;
      }

      let mut open = self.rename_wallet;
      let mut name_field = None;

      let normal = theme.typography.normal;
      let large = theme.typography.large;
      let heading = theme.typography.heading;

      let title = RichText::new("Rename Wallet").size(heading);
      let frame = theme.window_frame.fill(theme.colors.bg);
      let id = Id::unique("rename_wallet_window");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .closable(true)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(450.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing.y = theme.spacing.md;
               ui.spacing_mut().button_padding = theme.button_padding;

               let Some(old_wallet) = self.wallet_to_rename.as_ref() else {
                  ui.label(RichText::new("No wallet selected").size(large));
                  return;
               };

               ui.label(RichText::new("Wallet Name").size(large));

               let field_size = vec2(ui.available_width() * 0.9, 45.0);
               name_field = Some(
                  ui.allocate_ui(field_size, |ui| {
                     SecureTextEdit::singleline(&mut self.new_wallet_name)
                        .font(FontId::proportional(normal))
                        .margin(Margin::same(10))
                        .desired_width(ui.available_width())
                        .show(ui)
                  })
                  .inner
                  .response,
               );

               let text = RichText::new("Rename").size(large);
               let rename_button = Button::new(text).min_size(field_size);

               if ui.add(rename_button).clicked() {
                  on_rename_wallet(self.new_wallet_name.clone(), old_wallet.address);
               }
            });
         });

      // The field draws no label of its own, so it is named for what it holds.
      if let Some(field) = &name_field {
         ui.ctx()
            .accesskit_node_builder(field.id, |node| node.set_label("New wallet name"));
      }

      // The dialog spends its first passes sizing, where its body is invisible and cannot hold the
      // focus, so the field asks for it on the first pass that is drawn. It asks after the dialog
      // has drawn, so the dialog's own opening grab — which moves focus to its Close button —
      // cannot take it back.
      if let Some(field) = name_field.filter(|field| field.visible() && self.rename_focus_field) {
         ui.ctx().memory_mut(|memory| memory.request_focus(field.id));
         self.rename_focus_field = false;
      }

      if !open {
         self.close_rename_wallet();
      }
   }
}

#[cfg(test)]
impl WalletUi {
   /// Open the page on `current`, with `others` under it, and build the rows the list paints —
   /// without the worker that reads the vault.
   pub(crate) fn for_test(&mut self, current: &WalletInfo, others: Vec<WalletInfo>) {
      self.open = true;
      self.loading = false;
      self.wallets = std::iter::once(current.clone()).chain(others).collect();
      self.refresh_visible_wallets(current);
   }

   /// The value and the chains the loader would have written back for one wallet.
   pub(crate) fn set_value_for_test(
      &mut self,
      address: Address,
      value: WalletValue,
      chains: Vec<u64>,
   ) {
      self.wallet_value.insert(address, value);
      self.wallet_chains.insert(address, chains);
   }

   pub(crate) fn set_query_for_test(&mut self, query: &str) {
      self.search_query = query.to_string();
   }

   pub(crate) fn rename_is_open_for_test(&self) -> bool {
      self.rename_wallet
   }

   /// The export dialog, for a tree test that opens it without the worker its own `open` runs.
   pub(crate) fn export_key_ui_for_test(&mut self) -> &mut ExportKeyUi {
      &mut self.export_key_ui
   }

   /// The delete dialog, for a tree test that opens its confirmation with no vault behind it.
   pub(crate) fn delete_wallet_ui_for_test(&mut self) -> &mut DeleteWalletUi {
      &mut self.delete_wallet_ui
   }
}

/// The sentence that says how many wallets the search left.
fn count_sentence(count: usize) -> String {
   match count {
      0 => "No wallets found".to_string(),
      1 => "1 wallet".to_string(),
      count => format!("{count} wallets"),
   }
}

fn on_calc_wallet_value() {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let list = WalletListByValue::collect(&ctx);

      SHARED_GUI.write(|gui| {
         gui.wallet_ui.loading = false;
         gui.wallet_ui.wallets = list.wallets;
         gui.wallet_ui.wallet_value = list.values;
         gui.wallet_ui.wallet_chains = list.chains;

         // Build the rows the list will paint here, off the frame path: a full search match over
         // every wallet is the cost a long vault cannot pay per frame.
         let current_wallet = gui.ctx.current_wallet_info();
         gui.wallet_ui.refresh_visible_wallets(&current_wallet);
      });
   });
}

fn on_show_wallet_qr(wallet: WalletInfo) {
   crate::runtime::rt().spawn_blocking(move || {
      SHARED_GUI.write(|gui| {
         gui.account_panel.qr_window.open(wallet);
         gui.request_repaint();
      });
   });
}

fn on_rename_wallet(new_wallet_name: String, old_wallet_addr: Address) {
   crate::runtime::rt().spawn_blocking(move || {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let old_vault = ctx.get_vault();

      let Some(mut new_wallet) = ctx.get_wallet(old_wallet_addr) else {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window("Wallet not found");
            gui.request_repaint();
         });
         return;
      };

      // The field opens on the name in use, so asking for that same name is neither an error nor a
      // reason to rewrite the vault: the page says the wallet already answers to it.
      if new_wallet_name == new_wallet.name {
         SHARED_GUI.write(|gui| {
            gui.wallet_ui.close_rename_wallet();
            let notice = format!("This wallet is already called {new_wallet_name}.");
            gui.wallet_ui.notice = Some(notice);
            gui.request_repaint();
         });
         return;
      }

      if old_vault.wallet_name_exists(&new_wallet_name) {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window(format!(
               "Wallet with name {} already exists",
               new_wallet_name
            ));
            gui.request_repaint();
         });
         return;
      }

      if new_wallet_name.is_empty() {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window("Wallet name cannot be empty");
            gui.request_repaint();
         });
         return;
      }

      let max_chars = old_vault.name_max_chars();

      if new_wallet_name.chars().count() > max_chars {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window(format!(
               "Wallet name cannot be longer than {} characters",
               max_chars
            ));
            gui.request_repaint();
         });
         return;
      }

      new_wallet.name = new_wallet_name;

      let mut new_vault = old_vault.clone();

      for wallet in new_vault.all_wallets_mut() {
         if wallet.address() == old_wallet_addr {
            *wallet = new_wallet.clone();
         }
      }

      let is_current = ctx.is_current_wallet(new_wallet.address());
      let current_info = WalletInfo::from_wallet(&new_wallet, true);

      if is_current {
         ctx.write(|ctx| {
            ctx.current_wallet = current_info.clone();
         });
      }

      SHARED_GUI.write(|gui| {
         gui.wallet_ui.rename_wallet = false;
      });

      // Don't open the loading window here so we don't block the
      // user from interacting with the UI. Toast when the save finishes.
      // Safety: The wallet is only updated if the op is successful
      match ctx.encrypt_and_save_vault(Some(new_vault.clone()), None) {
         Ok(_) => {
            SHARED_GUI.write(|gui| {
               // Update the account panel
               if is_current {
                  gui.account_panel.set_current_wallet(current_info);
               }

               // Reset state
               gui.wallet_ui.close_rename_wallet();

               Toast::new("Vault saved")
                  .tone(ChipRole::Success)
                  .description("Wallet renamed successfully")
                  .duration(Duration::from_secs(5))
                  .show(&gui.egui_ctx);
               gui.request_repaint();
            });
         }
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!(
                  "Failed to encrypt vault, changes reverted: {}",
                  e
               ));
               gui.request_repaint();
            });
            return;
         }
      };

      ctx.set_vault(new_vault);
      ctx.build_wallet_info_cache();

      // Calculate the wallets again
      SHARED_GUI.write(|gui| {
         gui.wallet_ui.calc_wallet_value();
      });
   });
}

#[cfg(test)]
mod tests {
   use super::*;

   #[test]
   fn the_count_says_how_many_wallets_the_search_left() {
      assert_eq!(count_sentence(0), "No wallets found");
      assert_eq!(count_sentence(1), "1 wallet");
      assert_eq!(count_sentence(2), "2 wallets");
      assert_eq!(count_sentence(11), "11 wallets");
   }
}
