//! UI that allows the user to send ETH or ERC20 tokens (public) or private
//! Railgun (zk → zk) transfers when privacy mode is enabled.

use eframe::egui::{
   Align, CursorIcon, FontId, Frame, Layout, Margin, OpenUrl, RichText, Sense, Ui, vec2,
};

use std::{
   collections::HashMap,
   str::FromStr,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::core::{
   DecodedEvent, SendTxOptions, SendTxRequest, TransactionAnalysis, TransferParams, ZeusContext,
   ZeusCtx, send_transaction,
};
use crate::utils::{RT, estimate_tx_cost, simulate};

use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow,
      common::{AmountField, AmountFieldParams, show_with_fade},
      dapps::railgun::private_transfer,
      token_selection::{PickerMode, nft_collection_name},
   },
};
use crate::utils::simulate::{
   AccountPrefetch, fetch_accounts_info, native_balance_at, pinned_head,
};
use egui_elements::{Button, Label, SecureTextEdit, Theme};
use egui_lucide::Lucide;

use zeus_eth::{
   abi::{erc721, erc1155},
   alloy_primitives::{Address, Bytes, U256},
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token, NativeCurrency},
   nft::{NftCollection, NftStandard, NftToken, verify_ownership},
   revm_utils::{ForkFactory, Host, new_evm, simulate as revm_simulate},
   types::ChainId,
   utils::{NumericValue, batch},
};

use anyhow::{anyhow, bail};

const POOL_UPDATE_TIMEOUT: u64 = 60;

/// What the Send view is sending.
///
/// One of two things rather than two booleans: an NFT has no `Currency`, no decimals and no price, so
/// it cannot ride the amount field at all — the two paths share the recipient and the pipeline and
/// nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
   Fungible,
   Nft,
}

pub struct SendCryptoUi {
   open: bool,
   /// What this view is sending. The fungible path is the default and is unchanged.
   mode: SendMode,
   /// The NFT being sent, when `mode` is [`SendMode::Nft`].
   pub selected_nft: Option<NftToken>,
   /// How many units of it. ERC-721 always sends exactly one and ignores this.
   pub nft_amount: String,
   pub currency: Currency,
   pub amount_field: AmountField,
   pub recipient: String,
   pub recipient_name: Option<String>,
   pub search_query: String,
   /// Optional memo for private (zk → zk) transfers.
   pub memo: String,
   pub size: (f32, f32),
   pub price_syncing: bool,
   pub syncing_balance: bool,
   pub sending_tx: bool,
   last_price_update: HashMap<Address, Instant>,
}

impl SendCryptoUi {
   pub fn new() -> Self {
      Self {
         open: false,
         mode: SendMode::Fungible,
         selected_nft: None,
         nft_amount: String::new(),
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         amount_field: AmountField::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         memo: String::new(),
         size: (500.0, 690.0),
         price_syncing: false,
         syncing_balance: false,
         sending_tx: false,
         last_price_update: HashMap::new(),
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
      self.clear_recipient();
      self.amount_field.reset();
      self.clear_search_query();
      self.memo.clear();
      self.clear_nft();
   }

   pub fn set_currency(&mut self, currency: Currency) {
      self.clear_nft();
      self.currency = currency;
   }

   /// Default token for the active mode / chain (native for public, WETH for private).
   pub fn default_currency(&mut self, privacy_mode: bool, chain_id: u64) {
      self.clear_nft();

      self.currency = if privacy_mode {
         Currency::from(ERC20Token::wrapped_native_token(chain_id))
      } else {
         Currency::from(NativeCurrency::from(chain_id))
      };
   }

   pub fn get_mode(&self) -> SendMode {
      self.mode
   }

   /// Send this NFT.
   ///
   /// Sets the mode too: a selection that leaves the view on the fungible path is not a state any
   /// caller wants, so there is no way to get it.
   pub fn set_nft(&mut self, nft: NftToken) {
      self.mode = SendMode::Nft;
      self.selected_nft = Some(nft);
   }

   /// Back to the fungible path, forgetting the NFT.
   ///
   /// Called from every hook that means "the token for this chain changed" (`close`, `set_currency`,
   /// `default_currency`): an NFT picked on the previous chain cannot be sent on the new one, and a
   /// reopened view must never come back in a mode the user left behind.
   fn clear_nft(&mut self) {
      self.mode = SendMode::Fungible;
      self.selected_nft = None;
      self.nft_amount.clear();
   }

   pub fn clear_recipient(&mut self) {
      self.recipient_name = None;
      self.recipient = String::new();
   }

   pub fn clear_search_query(&mut self) {
      self.search_query = String::new();
   }

   fn show_railgun_not_supported(&self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         let text = RichText::new("Railgun is not supported for the selected chain")
            .size(theme.typography.very_large);
         ui.label(text);
      });
   }

   fn show_railgun_not_enabled(&self, theme: &Theme, ui: &mut Ui) {
      ui.vertical_centered(|ui| {
         ui.set_width(self.size.0);
         ui.set_max_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         let text = RichText::new("Railgun is disabled").size(theme.typography.very_large);
         ui.label(text);
         ui.label(
            RichText::new("Enable it in Settings/Railgun to send private transfers.")
               .size(theme.typography.large),
         );
      });
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      theme: &Theme,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      show_with_fade(ui, "send_crypto_ui_fade", self.open, |ui| {
         let privacy_mode = ctx.privacy_mode;
         // Private transfer: both tokens and recipients are private (notes + 0zk).
         let token_privacy_mode = privacy_mode;
         let recipient_privacy_mode = privacy_mode;

         let frame = theme.frame1;

         ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
               Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
                  ui.spacing_mut().button_padding = theme.button_padding;

                  if privacy_mode && !ctx.railgun_is_supported(ctx.chain) {
                     self.show_railgun_not_supported(theme, ui);
                     return;
                  }

                  if privacy_mode && !ctx.is_railgun_enabled(ctx.chain.id()) {
                     self.show_railgun_not_enabled(theme, ui);
                     return;
                  }

                  let text_edit_visuals = theme.text_edit_visuals();

                  let title = if privacy_mode {
                     "Private Transfer"
                  } else {
                     "Send Crypto"
                  };
                  ui.label(RichText::new(title).size(theme.typography.heading));

                  let owner = ctx.current_wallet_info().address;
                  let owner_zk = ctx.current_wallet_info().zk_address();
                  let chain = ctx.chain;
                  let inner_frame = theme.frame2;

                  // NFT mode replaces the whole amount block below: an NFT has no decimals, no price
                  // and no `Currency`, so `AmountField` cannot describe it.
                  if self.mode == SendMode::Nft && !privacy_mode {
                     inner_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.show_nft_selection(
                           ctx,
                           theme,
                           icons.clone(),
                           token_selection,
                           owner,
                           ui,
                        );
                     });
                  } else {
                     // Currency Selection
                     let balance = self.balance_for_mode(ctx, owner, privacy_mode);
                     let cost = self.cost(ctx, privacy_mode);
                     let max_amount = if privacy_mode || self.currency.is_erc20() {
                        balance.clone()
                     } else if balance.wei() < cost.wei() {
                        NumericValue::default()
                     } else {
                        NumericValue::format_wei(
                           balance.wei() - cost.wei(),
                           self.currency.decimals(),
                        )
                     };

                     let amount = self.amount_field.amount.clone();
                     let currency = self.currency.clone();
                     let data_syncing = self.price_syncing || self.syncing_balance;
                     let should_calculate_price = self.should_calculate_price(&currency);
                     let value = value(ctx, currency, amount, should_calculate_price);

                     inner_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        self.amount_field.show(
                           AmountFieldParams::new(
                              theme,
                              icons.clone(),
                              &self.currency,
                              owner,
                              chain.id(),
                           )
                           .privacy_mode(token_privacy_mode)
                           .balance(balance)
                           .max_amount(max_amount)
                           .value(value)
                           .label("Amount")
                           .token_selection(token_selection, None)
                           .loading(data_syncing)
                           .show_slider(true),
                           ui,
                        );
                     });
                  }

                  // The picker's own Tokens/NFTs switch decides the mode, so picking an NFT there is
                  // what makes this an NFT send — there is no second switch to keep in sync.
                  if let Some(currency) = token_selection.get_selected_currency() {
                     self.currency = currency.clone();
                     // A fungible pick means the fungible path, however the view got here.
                     self.clear_nft();
                     token_selection.reset();
                     self.sync_balance(owner, privacy_mode);
                  }

                  // Private transfers are fungible-only — an NFT note cannot pay a broadcaster or
                  // paymaster fee — so an NFT picked while privacy mode is on is ignored instead of
                  // being sent down the zk path.
                  if !privacy_mode {
                     if let Some(nft) = token_selection.get_selected_nft() {
                        self.set_nft(nft.clone());
                        token_selection.reset();
                     }
                  }

                  // Hoisted: `show` takes `ctx` mutably, so read the chain before the call.
                  let send_chain = ctx.chain.id();

                  recipient_selection.show(
                     ctx,
                     theme,
                     icons.clone(),
                     recipient_privacy_mode,
                     send_chain,
                     contacts_ui,
                     ui,
                  );
                  let recipient = recipient_selection.get_recipient();

                  // Recipient Selection
                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());
                     ui.horizontal(|ui| {
                        ui.label(RichText::new("Recipient").size(theme.typography.large));
                        ui.add_space(10.0);

                        if !recipient.is_empty(recipient_privacy_mode) {
                           if let Some(name) = &recipient.name {
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.typography.large)
                                    .color(theme.colors.info),
                              );
                           } else {
                              ui.label(
                                 RichText::new("Unknown Address")
                                    .size(theme.typography.large)
                                    .color(theme.colors.error),
                              );
                           }

                           ui.add_space(5.0);

                           if !recipient_privacy_mode && !recipient.evm_address.is_empty() {
                              let block_explorer = chain.block_explorer();
                              let link = format!(
                                 "{}/address/{}",
                                 block_explorer, recipient.evm_address
                              );
                              let icon = Lucide::ExternalLink
                                 .size(18.0)
                                 .color(theme.colors.text)
                                 .image()
                                 .sense(Sense::click());

                              let res = ui.add(icon).on_hover_cursor(CursorIcon::PointingHand);

                              if res.clicked() {
                                 let url = OpenUrl::new_tab(link);
                                 ui.ctx().open_url(url);
                              }
                           }
                        }
                     });

                     ui.horizontal(|ui| {
                        let hint = if recipient_privacy_mode {
                           RichText::new("Search contacts or enter a 0zk address")
                              .size(theme.typography.normal)
                              .color(theme.colors.text_muted)
                        } else {
                           RichText::new("Search contacts, ENS or enter an address")
                              .size(theme.typography.normal)
                              .color(theme.colors.text_muted)
                        };

                        let address_edit = if recipient_privacy_mode {
                           &mut recipient_selection.recipient.zk_address
                        } else {
                           &mut recipient_selection.recipient.evm_address
                        };

                        let res = ui.add(
                           SecureTextEdit::singleline(address_edit)
                              .visuals(text_edit_visuals)
                              .hint_text(hint)
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.typography.normal)),
                        );
                        if res.clicked() {
                           recipient_selection.open();
                        }
                     });
                  });

                  if privacy_mode {
                     inner_frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                           ui.label(RichText::new("Memo").size(theme.typography.large));
                           ui.add_space(8.0);
                           ui.label(
                              RichText::new("(optional)")
                                 .size(theme.typography.small)
                                 .color(theme.colors.text_muted),
                           );
                        });
                        ui.add(
                           SecureTextEdit::singleline(&mut self.memo)
                              .visuals(text_edit_visuals)
                              .hint_text(
                                 RichText::new("Shown in private history")
                                    .size(theme.typography.normal)
                                    .color(theme.colors.text_muted),
                              )
                              .min_size(vec2(ui.available_width(), 25.0))
                              .margin(Margin::same(10))
                              .font(FontId::proportional(theme.typography.normal)),
                        );
                     });
                  }

                  // A recipient resolved from an ERC-7828 name carries the chain it resolved
                  // for. Read it before the address fields are moved out of `recipient`.
                  let recipient_chain = recipient.chain;

                  let recipient_str = if recipient_privacy_mode {
                     recipient.zk_address
                  } else {
                     recipient.evm_address
                  };

                  self.send_button(
                     ctx,
                     theme,
                     owner,
                     owner_zk,
                     recipient_str,
                     recipient_chain,
                     privacy_mode,
                     ui,
                  );
               });
            });
         });
      });
   }

   /// The NFT-mode replacement for the amount block.
   ///
   /// `AmountField` needs a `Currency` and shows a balance, a fiat value and a slider — none of which
   /// an NFT has. This shows what is being sent and, for the standards that have more than one, how
   /// many; the button opens the picker, which owns the Tokens/NFTs switch.
   fn show_nft_selection(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      owner: Address,
      ui: &mut Ui,
   ) {
      let chain_id = ctx.chain.id();
      let text_edit_visuals = theme.text_edit_visuals();
      let selected = self.selected_nft.clone();

      ui.set_width(ui.available_width());

      ui.horizontal(|ui| {
         ui.label(RichText::new("NFT").size(theme.typography.large));
         ui.add_space(10.0);

         match &selected {
            None => {
               ui.label(
                  RichText::new("None selected")
                     .size(theme.typography.normal)
                     .color(theme.colors.text_muted),
               );
            }
            Some(nft) => {
               // The same label widget a picker row uses, so the art goes through one code path and
               // falls back to the placeholder the same way.
               let name = nft_collection_name(
                  ctx.nft_db.get_collection(chain_id, nft.collection).as_ref(),
                  nft.collection,
               );
               let text = format!("{}\n#{}", name, nft.token_id);
               let icon = icons.nft_icon_x64(
                  chain_id,
                  nft.collection,
                  nft.token_id,
                  theme.image_tint_recommended,
               );
               let label = Label::new(
                  RichText::new(text).size(theme.typography.normal),
                  Some(icon),
               )
               .interactive(false)
               .wrap()
               .image_on_left();

               ui.add(label);
            }
         }

         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let text = RichText::new(match selected.is_some() {
               true => "Change",
               false => "Select NFT",
            })
            .size(theme.typography.normal);

            let button =
               Button::new(text).min_size(vec2(90.0, 25.0)).visuals(theme.button_visuals());

            if ui.add(button).clicked() {
               // `open` restores the fungible default, so the mode is set *after* it.
               token_selection.open(false, chain_id, owner);
               token_selection.set_mode(PickerMode::Nft);
            }
         });
      });

      let Some(nft) = &selected else {
         return;
      };

      if nft.standard == NftStandard::Erc1155 {
         ui.add_space(theme.spacing.sm);
         ui.horizontal(|ui| {
            ui.label(RichText::new("Amount").size(theme.typography.large));
            ui.add_space(10.0);
            ui.add(
               SecureTextEdit::singleline(&mut self.nft_amount)
                  .visuals(text_edit_visuals)
                  .hint_text(
                     RichText::new("How many to send")
                        .size(theme.typography.normal)
                        .color(theme.colors.text_muted),
                  )
                  .desired_width(ui.available_width() * 0.5)
                  .margin(Margin::same(10))
                  .font(FontId::proportional(theme.typography.normal)),
            );
         });
      } else {
         // ERC-721: the id is the whole payload, so it is shown read-only. There is nothing to edit —
         // one token, and its id came from the selection.
         ui.add_space(theme.spacing.xs);
         ui.label(
            RichText::new(format!("Token ID {}", nft.token_id))
               .size(theme.typography.small)
               .color(theme.colors.text_muted),
         );
      }
   }

   /// The NFT-mode send button: the fungible button's states with NFT checks.
   fn nft_send_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      owner: Address,
      owner_zk: String,
      recipient: String,
      recipient_chain: Option<u64>,
      ui: &mut Ui,
   ) {
      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      let valid_recipient = self.valid_recipient(&recipient, false);
      let recipient_is_sender = self.recipient_is_sender(owner, &owner_zk, &recipient, false);
      let has_entered_recipient = !recipient.trim().is_empty();
      let nft = self.selected_nft.clone();
      let amount = self.nft_transfer_amount();
      let wrong_chain = recipient_chain.filter(|chain| *chain != ctx.chain.id());

      let mut button_text = match nft.is_some() {
         true => "Send".to_string(),
         false => "Select an NFT".to_string(),
      };

      if nft.is_some() && amount.is_none() {
         button_text = "Invalid Amount".to_string();
      }

      if has_entered_recipient && !valid_recipient {
         button_text = "Invalid Recipient".to_string();
      }

      if has_entered_recipient && recipient_is_sender {
         button_text = "Cannot send to yourself".to_string();
      }

      // Last, for the same reason as the fungible button: on the wrong chain every check above is
      // about the wrong chain, and sending there is the mistake worth blocking.
      if let Some(chain) = wrong_chain {
         button_text = match ChainId::new(chain) {
            Ok(chain) => format!("Switch to {} to send", chain.name()),
            Err(_) => "Unsupported chain".to_string(),
         };
      }

      let valid_inputs = nft.is_some()
         && amount.is_some()
         && valid_recipient
         && !recipient_is_sender
         && has_entered_recipient
         && wrong_chain.is_none()
         && !sending_tx;

      let text = RichText::new(button_text).size(theme.typography.large);
      let send = Button::new(text)
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(button_visuals);

      if ui.add_enabled(valid_inputs, send).clicked() {
         if let (Some(nft), Some(amount)) = (nft, amount) {
            self.sending_tx = true;

            match self.send_nft_transaction(ctx, nft, amount, recipient) {
               Ok(_) => {}
               Err(e) => {
                  self.sending_tx = false;

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Error while sending transaction: {}", e);
                        gui.open_msg_window(msg);
                     });
                  });
               }
            }
         }
      }
   }

   /// How many units to move: ERC-721 always exactly one, ERC-1155 whatever the user typed.
   ///
   /// `None` means the input is not a usable amount, which blocks the send. The chain would revert
   /// anyway, but a revert *after* the confirm window is a worse way to learn that zero is not an
   /// amount — and for an amount above the balance the simulation is what refuses.
   fn nft_transfer_amount(&self) -> Option<U256> {
      let nft = self.selected_nft.as_ref()?;

      if nft.standard == NftStandard::Erc721 {
         return Some(U256::from(1));
      }

      let amount = U256::from_str(self.nft_amount.trim()).ok()?;

      match amount.is_zero() {
         true => None,
         false => Some(amount),
      }
   }

   /// Spawn the NFT send, mirroring [`SendCryptoUi::send_public_transaction`].
   fn send_nft_transaction(
      &mut self,
      ctx: &mut ZeusContext,
      nft: NftToken,
      amount: U256,
      recipient: String,
   ) -> Result<(), anyhow::Error> {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let recipient_address = Address::from_str(&recipient)?;

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
            gui.ctx.clone()
         });

         match send_nft(
            ctx.clone(),
            chain,
            from,
            recipient_address,
            nft,
            amount,
         )
         .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  // The token left this wallet, so it cannot stay selected: sending it again would
                  // revert. The mode stays, so another NFT can be picked without a detour.
                  gui.send_crypto.selected_nft = None;
                  gui.send_crypto.nft_amount.clear();
               });
            }
            Err(e) => {
               tracing::error!("Error sending NFT: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  gui.notification.reset();
                  gui.loading_window.reset();
                  let msg = format!("Transaction Error: {}", e);
                  gui.msg_window.open(msg);
               });
            }
         }
      });

      Ok(())
   }

   fn should_calculate_price(&self, currency: &Currency) -> bool {
      let now = Instant::now();
      let last_updated = self.last_price_update.get(&currency.address()).cloned();
      if last_updated.is_none() {
         return true;
      }

      let last_updated = last_updated.unwrap();
      let timeout = Duration::from_secs(POOL_UPDATE_TIMEOUT);
      let time_passed = now.duration_since(last_updated);
      time_passed > timeout
   }

   fn send_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      owner: Address,
      owner_zk: String,
      recipient: String,
      recipient_chain: Option<u64>,
      privacy_mode: bool,
      ui: &mut Ui,
   ) {
      // NFT mode shares none of the checks below — no currency, no balance manager, no amount field —
      // so it leaves early, which is what keeps this path untouched.
      if self.mode == SendMode::Nft && !privacy_mode {
         self.nft_send_button(
            ctx,
            theme,
            owner,
            owner_zk,
            recipient,
            recipient_chain,
            ui,
         );
         return;
      }

      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      let recipient_is_sender =
         self.recipient_is_sender(owner, &owner_zk, &recipient, privacy_mode);
      let valid_recipient = self.valid_recipient(&recipient, privacy_mode);
      let valid_amount = self.valid_amount();
      let has_balance = self.sufficient_balance(ctx, owner, privacy_mode);
      let has_entered_amount = !self.amount_field.amount.is_empty();
      let has_entered_recipient = !recipient.trim().is_empty();
      let valid_token = if privacy_mode {
         self.currency.is_erc20()
      } else {
         true
      };

      // A chain-specific recipient may only be sent to on the chain it resolved for. The picker
      // asks before switching, so reaching here means the active chain moved (or the address was
      // edited) afterwards — refuse instead of sending on a chain the user did not pick.
      //
      // Public sends only: a private transfer goes to a `0zk` address, which has no chain.
      let wrong_chain = match privacy_mode {
         true => None,
         false => recipient_chain.filter(|chain| *chain != ctx.chain.id()),
      };

      let valid_inputs = valid_recipient
         && !recipient_is_sender
         && has_balance
         && has_entered_amount
         && valid_amount
         && valid_token
         && has_entered_recipient
         && wrong_chain.is_none()
         && !sending_tx;

      let mut button_text = "Send".to_string();

      if has_entered_amount && !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if has_entered_recipient && !valid_recipient {
         button_text = "Invalid Recipient".to_string();
      }

      if has_entered_recipient && recipient_is_sender {
         button_text = "Cannot send to yourself".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      if privacy_mode && !valid_token {
         button_text = "Invalid Token".to_string();
      }

      // Last, so it wins: on the wrong chain every balance figure above is for the wrong chain,
      // and sending there is the mistake worth blocking.
      if let Some(chain) = wrong_chain {
         button_text = match ChainId::new(chain) {
            Ok(chain) => format!("Switch to {} to send", chain.name()),
            Err(_) => "Unsupported chain".to_string(),
         };
      }

      let text = RichText::new(button_text).size(theme.typography.large);
      let send = Button::new(text)
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(button_visuals);

      if ui.add_enabled(valid_inputs, send).clicked() {
         self.sending_tx = true;

         if privacy_mode {
            self.send_private_transfer(ctx, recipient);
         } else {
            match self.send_public_transaction(ctx, recipient) {
               Ok(_) => {}
               Err(e) => {
                  self.sending_tx = false;

                  RT.spawn_blocking(move || {
                     SHARED_GUI.write(|gui| {
                        let msg = format!("Error while sending transaction: {}", e);
                        gui.open_msg_window(msg);
                     });
                  });
               }
            }
         }
      }
   }

   fn sync_balance(&mut self, owner: Address, privacy_mode: bool) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         if privacy_mode {
            ctx.update_private_data(chain, owner).await;
         } else {
            let balance_manager = ctx.balance_manager();
            if currency.is_native() {
               match balance_manager
                  .update_eth_balance(ctx.clone(), chain, vec![owner], false)
                  .await
               {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!("Failed to update ETH balance: {}", e);
                  }
               }
            } else {
               let token = currency.to_erc20().into_owned();
               match balance_manager
                  .update_tokens_balance(ctx.clone(), chain, owner, vec![token], false)
                  .await
               {
                  Ok(_) => {}
                  Err(e) => {
                     tracing::error!("Failed to update token balance: {}", e);
                  }
               }
            }
         }
         SHARED_GUI.write(|gui| {
            gui.send_crypto.syncing_balance = false;
         });
      });
   }

   fn cost(&self, ctx: &mut ZeusContext, privacy_mode: bool) -> NumericValue {
      let gas_used = if privacy_mode {
         500_000
      } else if self.currency.is_native() {
         ctx.chain.transfer_gas()
      } else {
         ctx.chain.erc20_transfer_gas()
      };

      let fee = ctx.priority_fee.get(ctx.chain.id()).cloned().unwrap_or_default();
      let (cost_in_wei, _) = estimate_tx_cost(ctx, ctx.chain.id(), gas_used, fee.wei());
      cost_in_wei
   }

   fn valid_recipient(&self, recipient: &str, privacy_mode: bool) -> bool {
      if privacy_mode {
         // Full 0zk parse is expensive (~15–20ms)
         // Actual check happens on execution path
         let r = recipient.trim();
         r.starts_with("0zk") && r.len() > 20
      } else {
         let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
         recipient != Address::ZERO
      }
   }

   fn recipient_is_sender(
      &self,
      owner: Address,
      owner_zk: &str,
      recipient: &str,
      privacy_mode: bool,
   ) -> bool {
      if privacy_mode {
         !owner_zk.is_empty() && owner_zk == recipient.trim()
      } else {
         let recipient = Address::from_str(recipient).unwrap_or(Address::ZERO);
         recipient == owner
      }
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount_field.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn balance_for_mode(
      &self,
      ctx: &mut ZeusContext,
      owner: Address,
      privacy_mode: bool,
   ) -> NumericValue {
      if !privacy_mode {
         return ctx.get_currency_balance(ctx.chain.id(), owner, &self.currency);
      }

      let portfolio = ctx.read_wallet_state(|ws| ws.portfolio_db.get(ctx.chain.id(), owner));
      if let Some(token) = self.currency.erc20_opt() {
         for (t, balance, _value, _price) in portfolio.private_tokens() {
            if t.address == token.address {
               return balance.clone();
            }
         }
      }
      NumericValue::default()
   }

   fn sufficient_balance(
      &self,
      ctx: &mut ZeusContext,
      sender: Address,
      privacy_mode: bool,
   ) -> bool {
      let balance = self.balance_for_mode(ctx, sender, privacy_mode);
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      balance.wei() >= amount.wei()
   }

   fn send_private_transfer(&mut self, ctx: &mut ZeusContext, recipient: String) {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let currency = self.currency.clone();
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      let memo = self.memo.clone();

      ctx.railgun_status.set_op_in_progress(chain.id(), true);

      RT.spawn_blocking(move || {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
            gui.ctx.clone()
         });

         let result = RT.block_on(private_transfer(
            ctx.clone(),
            chain,
            currency,
            amount,
            from,
            recipient,
            memo,
         ));

         match result {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  gui.send_crypto.amount_field.reset();
                  gui.send_crypto.memo.clear();
                  gui.loading_window.reset();
                  gui.request_repaint();
               });
            }
            Err(e) => {
               tracing::error!("Error sending private transfer: {:?}", e);
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.sending_tx = false;
                  gui.notification.reset();
                  gui.loading_window.reset();
                  let msg = format!("Private Transfer Error: {}", e);
                  gui.msg_window.open(msg);
                  gui.request_repaint();
               });
            }
         }

         ctx.write(|ctx| {
            ctx.railgun_status.set_op_in_progress(chain.id(), false);
         });
      });
   }

   fn send_public_transaction(
      &mut self,
      ctx: &mut ZeusContext,
      recipient: String,
   ) -> Result<(), anyhow::Error> {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;
      let currency = self.currency.clone();
      let recipient_address = Address::from_str(&recipient)?;
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.loading_window.open("Wait while magic happens");
            gui.request_repaint();
            gui.ctx.clone()
         });

         if currency.is_native() {
            match send_eth(
               ctx.clone(),
               chain,
               from,
               recipient_address,
               amount,
               currency,
            )
            .await
            {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.send_crypto.amount_field.reset();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     let msg = format!("Transaction Error: {}", e);
                     gui.msg_window.open(msg);
                  });
               }
            }
         } else {
            match send_token(
               ctx.clone(),
               chain,
               from,
               recipient_address,
               currency,
               amount,
            )
            .await
            {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.send_crypto.amount_field.reset();
                  });
               }
               Err(e) => {
                  tracing::error!("Error sending transaction: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.send_crypto.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     let msg = format!("Transaction Error: {}", e);
                     gui.msg_window.open(msg);
                  });
               }
            }
         }
      });
      Ok(())
   }
}

fn value(
   ctx: &mut ZeusContext,
   currency: Currency,
   amount: String,
   should_fetch_price: bool,
) -> NumericValue {
   let price = ctx.get_currency_price(&currency);
   let amount = amount.parse().unwrap_or(0.0);
   let value = NumericValue::value(amount, price.f64());

   if should_fetch_price {
      let price_manager = ctx.price_manager.clone();
      let pool_manager = ctx.pool_manager.clone();
      let chain = currency.chain_id();

      RT.spawn(async move {
         let ctx = SHARED_GUI.write(|gui| {
            gui.send_crypto.price_syncing = true;
            gui.send_crypto.last_price_update.insert(currency.address(), Instant::now());
            gui.ctx.clone()
         });

         match price_manager
            .calculate_prices(
               ctx,
               chain,
               pool_manager,
               vec![currency.to_erc20().into_owned()],
            )
            .await
         {
            Ok(_) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.price_syncing = false;
               });
            }
            Err(_e) => {
               SHARED_GUI.write(|gui| {
                  gui.send_crypto.price_syncing = false;
               });
               #[cfg(feature = "dev")]
               tracing::error!("Error calculating price: {:?}", _e);
            }
         }
      });
   }

   value
}

async fn send_eth(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   amount: NumericValue,
   currency: Currency,
) -> Result<(), anyhow::Error> {
   let mev_protect = false;
   let dapp = "".to_string();
   let interact_to = recipient;
   let value = amount.wei();
   let call_data = Bytes::default();
   let auth_list = Vec::new();
   let eth = NativeCurrency::from(chain.id());

   let client = ctx.get_zeus_client();

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let accounts = vec![from, recipient];

   let eth_balance_before = client
      .request(chain.id(), |client| {
         let accounts2 = accounts.clone();
         async move {
            batch::get_eth_balances(
               client,
               chain.id(),
               Some(block_id),
               accounts2.clone(),
            )
            .await
         }
      })
      .await?;

   if eth_balance_before.len() != accounts.len() {
      return Err(anyhow!(
         "Failed to fetch ETH balances for accounts"
      ));
   }

   let sender_eth_balance_before = &eth_balance_before[0].balance;
   let recipient_eth_balance_before = &eth_balance_before[1].balance;

   let mut prefetch_accounts = Vec::new();
   prefetch_accounts.push(AccountPrefetch::eoa(from));
   prefetch_accounts.push(AccountPrefetch::eoa(recipient));
   prefetch_accounts.push(AccountPrefetch::eoa(block.header.beneficiary));

   let accounts_info = fetch_accounts_info(
      ctx.clone(),
      chain.id(),
      block_id,
      prefetch_accounts,
   )
   .await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let mut transfer_params = TransferParams {
      currency: eth.clone().into(),
      sender: from,
      recipient,
      ..Default::default()
   };

   let sender_eth_balance_after;
   let _real_amount_sent;
   let logs;
   let gas_used;

   {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let res = simulate::simulate_transaction(
         &mut evm,
         from,
         recipient,
         call_data.clone(),
         value,
         auth_list,
      )?;

      let state = evm.balance(recipient);
      let recipient_eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      _real_amount_sent = if recipient_eth_balance_after > *recipient_eth_balance_before {
         recipient_eth_balance_after - recipient_eth_balance_before
      } else {
         return Err(anyhow!(
            "Simulation Error: Recipient did not receive any ETH after the transfer"
         ));
      };

      let state = evm.balance(from);
      sender_eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      gas_used = res.tx_gas_used();
      logs = res.logs().to_vec();
   }

   let eth_cur = eth.clone().into();
   let amount_usd = ctx.get_currency_value_for_amount(amount.f64(), &eth_cur);
   // let real_amount_sent = NumericValue::format_wei(real_amount_sent, eth.decimals);
   // let real_amount_send_usd = ctx.get_currency_value_for_amount(real_amount_sent.f64(), &eth_cur);

   transfer_params.amount = amount;
   transfer_params.amount_usd = Some(amount_usd);
   // transfer_params.real_amount_sent = Some(real_amount_sent);
   // transfer_params.real_amount_sent_usd = Some(real_amount_send_usd);

   let contract_interact = Some(false);
   let auth_list = Vec::new();
   let source_is_zeus = true;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      gas_used,
      *sender_eth_balance_before,
      sender_eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   tx_analysis.set_main_event(DecodedEvent::Transfer(transfer_params));

   let (_, _) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(tx_analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         ..Default::default()
      },
   )
   .await?;

   match update_balances(ctx.clone(), chain.id(), currency, from, recipient).await {
      Ok(_) => {}
      Err(e) => {
         tracing::error!("Error updating balances: {:?}", e);
      }
   }

   Ok(())
}

async fn send_token(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   currency: Currency,
   amount: NumericValue,
) -> Result<(), anyhow::Error> {
   let token = currency.to_erc20().into_owned();

   let mev_protect = false;
   let dapp = "".to_string();
   let interact_to = token.address;
   let value = U256::ZERO;
   let call_data = token.encode_transfer(recipient, amount.wei());
   let auth_list = Vec::new();

   let client = ctx.get_zeus_client();

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before = native_balance_at(ctx.clone(), chain, from, block_id).await?;

   let recipient_token_balance_before = client
      .request(chain.id(), |client| {
         let token_clone = token.clone();
         async move { token_clone.balance_of(client.clone(), recipient, Some(block_id)).await }
      })
      .await?;

   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::eoa(recipient));
   accounts.push(AccountPrefetch::contract(token.address));
   accounts.push(AccountPrefetch::eoa(block.header.beneficiary));

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let mut transfer_params = TransferParams {
      currency: token.clone().into(),
      sender: from,
      recipient,
      ..Default::default()
   };

   let real_amount_sent;
   let eth_balance_after;
   let logs;
   let gas_used;

   {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      let res = simulate::simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
         auth_list,
      )?;

      let recipient_token_balance_after =
         revm_simulate::erc20_balance(&mut evm, token.address, recipient)?;

      let real_amount = if recipient_token_balance_after > recipient_token_balance_before {
         recipient_token_balance_after - recipient_token_balance_before
      } else {
         return Err(anyhow!(
            "Simulation Error: Recipient did not receive any tokens after transfer, you are probably try to interact with a malicious token"
         ));
      };

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      real_amount_sent = real_amount;
      gas_used = res.tx_gas_used();
      logs = res.logs().to_vec();
   }

   let amount_usd = ctx.get_token_value_for_amount(amount.f64(), &token);
   let real_amount_sent = NumericValue::format_wei(real_amount_sent, token.decimals);
   let real_amount_send_usd = ctx.get_token_value_for_amount(real_amount_sent.f64(), &token);

   transfer_params.amount = amount;
   transfer_params.amount_usd = Some(amount_usd);
   transfer_params.real_amount_sent = Some(real_amount_sent);
   transfer_params.real_amount_sent_usd = Some(real_amount_send_usd);

   let contract_interact = Some(true);
   let auth_list = Vec::new();
   let source_is_zeus = true;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      gas_used,
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   tx_analysis.set_main_event(DecodedEvent::Transfer(transfer_params));

   let (_, _) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(tx_analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         ..Default::default()
      },
   )
   .await?;

   match update_balances(ctx.clone(), chain.id(), currency, from, recipient).await {
      Ok(_) => {}
      Err(e) => {
         tracing::error!("Error updating balances: {:?}", e);
      }
   }

   Ok(())
}

/// Send one NFT to `recipient`.
///
/// The NFT counterpart of [`send_token`], sharing its stages: pin a head, prefetch the accounts the
/// call touches, fork, simulate, analyse, then the one send pipeline. `interact_to` is the
/// **collection** — an NFT is not an ERC-20 contract and the collection is what the call goes to —
/// and `value` stays zero, since no ether moves.
///
/// Deliberately no `set_main_event`: `DecodedEvent` has no NFT variant yet, so the confirm window
/// shows what the simulation's own logs decode to. An ERC-721/1155 transfer event is the next phase's
/// work; inventing a `Transfer` here would describe an NFT as a fungible amount.
async fn send_nft(
   ctx: ZeusCtx,
   chain: ChainId,
   from: Address,
   recipient: Address,
   nft: NftToken,
   amount: U256,
) -> Result<(), anyhow::Error> {
   let mev_protect = false;
   let dapp = String::new();
   let interact_to = nft.collection;
   let value = U256::ZERO;
   let auth_list = Vec::new();

   // `amount` is always 1 for ERC-721 (its `safeTransferFrom` has no amount), and the user's number
   // for ERC-1155. Empty `data`: nothing is notified of a receiver hook.
   let call_data = match nft.standard {
      NftStandard::Erc721 => erc721::encode_safe_transfer_from(from, recipient, nft.token_id),
      NftStandard::Erc1155 => erc1155::encode_safe_transfer_from(
         from,
         recipient,
         nft.token_id,
         amount,
         Bytes::new(),
      ),
   };

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before = native_balance_at(ctx.clone(), chain, from, block_id).await?;

   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::eoa(recipient));
   accounts.push(AccountPrefetch::contract(nft.collection));
   accounts.push(AccountPrefetch::eoa(block.header.beneficiary));

   let accounts_info = fetch_accounts_info(ctx.clone(), chain.id(), block_id, accounts).await;

   let fork_client = ctx.get_client(chain.id()).await?;
   let mut factory =
      ForkFactory::new_sandbox_factory(fork_client, chain.id(), None, Some(block_id));

   for info in accounts_info {
      factory.insert_account_info(info.address, info.info);
   }

   let fork_db = factory.new_sandbox_fork();

   let eth_balance_after;
   let logs;
   let gas_used;

   {
      let mut evm = new_evm(chain, Some(&block), fork_db);

      // The pre-state comes from the same fork the simulation runs on, and `transact` does not commit,
      // so this read leaves it untouched. ERC-721 needs no pre-read: `ownerOf` after the transfer says
      // everything, and a sender who was not the owner never gets past the simulation.
      let received_before = match nft.standard {
         NftStandard::Erc1155 => Some(revm_simulate::erc1155_balance_of(
            &mut evm,
            interact_to,
            recipient,
            nft.token_id,
         )?),
         NftStandard::Erc721 => None,
      };

      // A revert here (not the owner, not approved, not enough copies) is what refuses the send before
      // the user ever sees a confirm window.
      let res = simulate::simulate_transaction(
         &mut evm,
         from,
         interact_to,
         call_data.clone(),
         value,
         auth_list,
      )?;

      // A call that *succeeds* while moving nothing is what a broken or hostile collection does, and
      // the simulation is the only place to catch it before the user pays for it. `send_token` reads
      // the recipient's balance for the same reason; here the collection itself has to report the new
      // state — the logs cannot be trusted, a fake `Transfer` is one `emit` away.
      let moved = match nft.standard {
         NftStandard::Erc721 => {
            let owner = revm_simulate::erc721_owner_of(&mut evm, interact_to, nft.token_id)
               .map_err(|e| anyhow!("Could not verify the transfer: {}", e))?;

            owner == recipient
         }
         NftStandard::Erc1155 => {
            let received_after =
               revm_simulate::erc1155_balance_of(&mut evm, interact_to, recipient, nft.token_id)
                  .map_err(|e| anyhow!("Could not verify the transfer: {}", e))?;

            received_after.saturating_sub(received_before.unwrap_or_default()) >= amount
         }
      };

      if !moved {
         bail!(
            "Simulation Error: the transfer did not move token #{} — the collection may be broken or malicious",
            nft.token_id
         );
      }

      let state = evm.balance(from);
      eth_balance_after = if let Some(state) = state {
         state.data
      } else {
         U256::ZERO
      };

      gas_used = res.tx_gas_used();
      logs = res.logs().to_vec();
   }

   let contract_interact = Some(true);
   let auth_list = Vec::new();
   let source_is_zeus = true;

   let tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      contract_interact,
      call_data.clone(),
      value,
      logs,
      gas_used,
      eth_balance_before,
      eth_balance_after,
      auth_list.clone(),
   )
   .await?;

   let (_, _) = send_transaction(
      ctx.clone(),
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(call_data)
         .value(value)
         .authorization_list(auth_list)
         .analysis(tx_analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         ..Default::default()
      },
   )
   .await?;

   // What the transfer changed is the public side of both wallets, and possibly this wallet's own
   // record of the NFT.
   if ctx.wallet_exists(recipient) {
      ctx.update_public_data(chain.id(), recipient);
   }

   ctx.update_public_data(chain.id(), from);

   match update_nft(ctx.clone(), chain.id(), from, nft).await {
      Ok(_) => {}
      Err(e) => {
         tracing::error!("Error refreshing the NFT after send: {:?}", e);
      }
   }

   Ok(())
}

/// Re-read one NFT after a send, and make both stores agree with the chain.
///
/// Ownership is the chain's answer, never ours: `NftToken` carries no owner on purpose, and a
/// portfolio entry is only a claim about what this wallet holds. So the question is asked over RPC
/// rather than assumed from having just sent it.
///
/// When the wallet no longer holds it, the token leaves both the portfolio and the tracked catalog —
/// the picker's ERC-721 balance is a constant 1 with no chain call, so a left-behind entry would keep
/// offering a token that cannot be sent. The cached art stays: if the token comes back, it comes back
/// with its picture.
///
/// A transport failure changes nothing and is logged by the caller: dropping a token from the wallet
/// because an RPC hiccuped would be far worse than a stale row.
async fn update_nft(
   ctx: ZeusCtx,
   chain_id: u64,
   owner: Address,
   nft: NftToken,
) -> Result<(), anyhow::Error> {
   let client = ctx.get_client(chain_id).await?;

   // The cached collection when we have it (the usual case — a token the picker listed), otherwise
   // one ERC-165 sweep to learn the standard `verify_ownership` needs.
   let collection = match ctx.read(|ctx| ctx.nft_db.get_collection(chain_id, nft.collection)) {
      Some(collection) => collection,
      None => NftCollection::fetch(client.clone(), chain_id, nft.collection).await?,
   };

   if verify_ownership(client, &collection, nft.token_id, owner).await? {
      return Ok(());
   }

   ctx.write(|ctx| {
      ctx.nft_db.remove_nft(chain_id, nft.collection, nft.token_id);
   });

   ctx.write_wallet_state(|ws| {
      let mut portfolio = ws.portfolio_db.get(chain_id, owner);
      portfolio.remove_nft(&nft);
      ws.portfolio_db.insert_portfolio(chain_id, owner, portfolio);
   });

   // Logs internally.
   ctx.save_nft_db();

   if let Err(e) = ctx.save_wallet_state() {
      tracing::error!(
         "Error saving wallet state after an NFT send: {:?}",
         e
      );
   }

   Ok(())
}

async fn update_balances(
   ctx: ZeusCtx,
   chain: u64,
   currency: Currency,
   sender: Address,
   recipient: Address,
) -> Result<(), anyhow::Error> {
   let exists = ctx.wallet_exists(recipient);
   let manager = ctx.balance_manager();

   manager.update_eth_balance(ctx.clone(), chain, vec![sender], true).await?;

   if currency.is_erc20() {
      let token = currency.to_erc20().into_owned();
      manager
         .update_tokens_balance(ctx.clone(), chain, sender, vec![token], true)
         .await?;
   }

   if exists {
      if currency.is_native() {
         manager.update_eth_balance(ctx.clone(), chain, vec![recipient], true).await?;
      } else {
         let token = currency.to_erc20().into_owned();
         manager
            .update_tokens_balance(ctx.clone(), chain, recipient, vec![token], true)
            .await?;
      }
      ctx.update_public_data(chain, recipient);
   }

   ctx.update_public_data(chain, sender);
   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;

   fn nft(token_id: u64, standard: NftStandard) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: Address::from([0xbc; 20]),
         token_id: U256::from(token_id),
         standard,
         metadata_uri: None,
      }
   }

   /// An ERC-721 has no amount to enter — one token, always — so the field is ignored rather than
   /// required, however it happens to be filled.
   #[test]
   fn erc721_always_sends_exactly_one() {
      let mut send = SendCryptoUi::new();
      send.set_nft(nft(7, NftStandard::Erc721));

      assert_eq!(send.nft_transfer_amount(), Some(U256::from(1)));

      send.nft_amount = "9".to_string();
      assert_eq!(
         send.nft_transfer_amount(),
         Some(U256::from(1)),
         "a typed amount cannot turn one ERC-721 into nine"
      );
   }

   /// An ERC-1155 with no usable amount cannot be sent: empty, zero and nonsense all block it, and a
   /// real number goes through as typed.
   #[test]
   fn erc1155_amounts_have_to_be_usable() {
      let mut send = SendCryptoUi::new();
      send.set_nft(nft(7, NftStandard::Erc1155));

      assert_eq!(
         send.nft_transfer_amount(),
         None,
         "nothing typed yet"
      );

      for bad in ["", "  ", "0", "abc", "-3", "1.5"] {
         send.nft_amount = bad.to_string();
         assert_eq!(
            send.nft_transfer_amount(),
            None,
            "{bad:?} is not an amount"
         );
      }

      send.nft_amount = " 12 ".to_string();
      assert_eq!(
         send.nft_transfer_amount(),
         Some(U256::from(12)),
         "padded input still parses"
      );
   }

   /// Nothing selected is nothing to send.
   #[test]
   fn without_a_selection_there_is_no_amount() {
      let send = SendCryptoUi::new();
      assert_eq!(send.nft_transfer_amount(), None);
   }

   /// `set_nft` is the mode switch — there is no way to hold an NFT while staying on the fungible
   /// path, so the send button can never render NFT state over a token amount field.
   #[test]
   fn selecting_an_nft_switches_the_mode() {
      let mut send = SendCryptoUi::new();
      assert_eq!(send.get_mode(), SendMode::Fungible);

      send.set_nft(nft(7, NftStandard::Erc721));
      assert_eq!(send.get_mode(), SendMode::Nft);
      assert!(send.selected_nft.is_some());
   }

   /// Every hook that means "the token for this chain changed" drops the NFT and returns to the
   /// fungible path, or a reopened view comes back holding a token from the previous chain.
   #[test]
   fn changing_the_token_forgets_the_nft_and_the_mode() {
      for hook in 0..3 {
         let mut send = SendCryptoUi::new();
         send.set_nft(nft(7, NftStandard::Erc1155));
         send.nft_amount = "3".to_string();

         match hook {
            0 => send.close(),
            1 => send.set_currency(Currency::from(
               NativeCurrency::from_chain_id(1).unwrap(),
            )),
            _ => send.default_currency(false, 1),
         }

         assert_eq!(send.get_mode(), SendMode::Fungible, "hook {hook}");
         assert_eq!(send.selected_nft, None, "hook {hook}");
         assert!(send.nft_amount.is_empty(), "hook {hook}");
      }
   }
}
