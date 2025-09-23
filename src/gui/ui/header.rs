use crate::assets::icons::Icons;
use crate::core::utils::truncate_address;
use crate::core::{Wallet, ZeusCtx, utils::RT};
use crate::gui::{
   SHARED_GUI,
   ui::{ChainSelect, WalletSelect},
};
use egui::{Button, RichText, Ui, vec2};
use egui_theme::Theme;
use egui_widgets::Label;
use std::sync::Arc;
use zeus_eth::{
   currency::{Currency, NativeCurrency},
   types::ChainId,
};

/// Show some of current state of Zeus like the current chain, wallet, etc.
pub struct Header {
   open: bool,
   chain_select: ChainSelect,
   wallet_select: WalletSelect,
}

impl Header {
   pub fn new() -> Self {
      let chain_select = ChainSelect::new("main_chain_select", 1).size(vec2(220.0, 20.0));
      let wallet_select = WalletSelect::new("main_wallet_select").size(vec2(220.0, 20.0));

      Self {
         open: false,
         chain_select,
         wallet_select,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      self.open = true;
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn set_current_wallet(&mut self, wallet: Wallet) {
      self.wallet_select.wallet = wallet;
   }

   pub fn set_current_chain(&mut self, chain: ChainId) {
      self.chain_select.chain = chain;
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
      ui.spacing_mut().button_padding = vec2(4.0, 4.0);

      let chain = ctx.chain();
      let frame = theme.frame1;
      let wallet = ctx.current_wallet_info();

      frame.show(ui, |ui| {
         ui.vertical(|ui| {
            ui.horizontal(|ui| {
               self.show_chain_select(ctx.clone(), theme, icons.clone(), ui);
            });

            ui.horizontal(|ui| {
               self.show_wallet_select(ctx.clone(), theme, icons.clone(), ui);
            });

            // Wallet address, on click copy it to the clipboard
            ui.horizontal(|ui| {
               let address = wallet.address_truncated();

               let address_text = RichText::new(address).size(theme.text_sizes.normal);
               let button = Button::selectable(false, address_text);
               if ui.add(button).clicked() {
                  ui.ctx().copy_text(wallet.address.to_string());
               }
            });

            // Smart Account status
            let deleg_addr = ctx.get_delegated_address(chain.id(), wallet.address);
            ui.horizontal(|ui| {
               let text = RichText::new("Smart Account").size(theme.text_sizes.normal);

               let icon = match deleg_addr.is_some() {
                  true => icons.green_circle(),
                  false => icons.red_circle(),
               };

               let label = Label::new(text, Some(icon));
               ui.add(label);
            });

            let show_deleg = if cfg!(feature = "dev") {
               true
            } else {
               deleg_addr.is_some()
            };

            if show_deleg {
               ui.horizontal(|ui| {
                  let text = RichText::new("Delegated at").size(theme.text_sizes.normal);
                  ui.label(text);

                  ui.add_space(5.0);

                  let address = deleg_addr.unwrap_or_default();
                  let address_short = truncate_address(address.to_string());
                  let explorer = chain.block_explorer();
                  let link = format!("{}/address/{}", explorer, address.to_string());
                  let text = RichText::new(address_short)
                     .size(theme.text_sizes.normal)
                     .color(theme.colors.hyperlink_color);

                  ui.hyperlink_to(text, link);
               });
            }
         });
      });
   }

   fn show_chain_select(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.vertical(|ui| {
         if ctx.tx_confirm_window_open() {
            ui.disable();
         }

         if ctx.sign_msg_window_open() {
            ui.disable();
         }

         let clicked = self.chain_select.show(0, theme, icons.clone(), ui);
         if clicked {
            let new_chain = self.chain_select.chain;

            ctx.write(|ctx| {
               ctx.chain = new_chain;
            });

            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  let currency: Currency = NativeCurrency::from(new_chain.id()).into();
                  gui.send_crypto.set_currency(currency.clone());
                  gui.uniswap.swap_ui.default_currency_in(new_chain.id());
                  gui.uniswap.swap_ui.default_currency_out(new_chain.id());
                  gui.uniswap.create_position_ui.default_currency0(new_chain.id());
                  gui.uniswap.create_position_ui.default_currency1(new_chain.id());
               });
            });
         }
      });
   }

   fn show_wallet_select(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.vertical(|ui| {
         if ctx.tx_confirm_window_open() {
            ui.disable();
         }

         if ctx.sign_msg_window_open() {
            ui.disable();
         }

         let clicked = self.wallet_select.show(theme, ctx.clone(), icons.clone(), ui);
         if clicked {
            ctx.write(|ctx| {
               ctx.current_wallet = self.wallet_select.wallet.clone();
            });
         }
      });
   }
}
