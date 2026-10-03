use eframe::egui::{
   Align, Checkbox, CursorIcon, FontId, Id, Layout, Margin, OpenUrl, Order, RichText, Sense, Ui,
   vec2,
};

use std::{
   collections::HashMap,
   str::FromStr,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::core::{
   DecodedEvent, SendTxOptions, SendTxRequest, ShieldParams, TransactionAnalysis, WalletStateKey,
   ZeusContext, ZeusCtx, bundler_url_dir, ensure_allowance, ensure_approval_for_all,
   send_transaction,
};
use crate::{
   gui::ui::common::show_with_fade,
   utils::{RT, write_private_atomic},
};

use super::{RailgunAsset, expect_single_event, railgun_ready, settle_railgun_op};
use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow,
      common::{AmountField, AmountFieldParams},
      token_selection::{PickerMode, nft_collection_name},
   },
};
use crate::utils::simulate::{
   AccountPrefetch, ForkPrefetch, ForkSim, ForkSimRequest, StoragePrefetch, native_balance_at,
   pinned_head, railgun_common_accounts, simulate_on_fork,
};
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme};
use egui_lucide::Lucide;
use elegance::{Badge, BadgeTone};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token, NativeCurrency},
   nft::{NftStandard, NftToken},
   types::ChainId,
   utils::NumericValue,
};

use zeus_railgun::{RailgunAddress, rand::SeedableRng, rand_chacha::ChaCha12Rng};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::error;

use super::unshield::{default_bundler_url, unshield};

const POOL_UPDATE_TIMEOUT: u64 = 60;

/// Bound ciphertext to this logical slot (AAD).
const BUNDLER_URL_AAD: &[u8] = b"zeus-bundler-url-v1";

const SELF_BROADCAST_TIP: &str = "Submits the unshield from your public wallet. Breaks anonymity only use if private broadcast is unavailable.";
const UNWRAP_TO_ETH_TIP: &str =
   "Unwraps WETH to ETH. Useful if the recipient doesn't have native ETH for gas.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundlerUrl {
   pub url: String,
}

impl Default for BundlerUrl {
   fn default() -> Self {
      Self {
         url: default_bundler_url(1),
      }
   }
}

impl BundlerUrl {
   pub fn new(url: String) -> Self {
      Self { url }
   }

   pub fn save(&self, key: &WalletStateKey) -> Result<(), anyhow::Error> {
      let sealed = key.seal_json(self, BUNDLER_URL_AAD)?;
      write_private_atomic(&Self::dir()?, &sealed)?;
      Ok(())
   }

   pub fn load(key: &WalletStateKey) -> Result<Self, anyhow::Error> {
      let sealed = std::fs::read(Self::dir()?)?;
      key.open_json(&sealed, BUNDLER_URL_AAD)
   }

   pub fn dir() -> Result<std::path::PathBuf, anyhow::Error> {
      bundler_url_dir()
   }

   pub fn exists() -> Result<bool, anyhow::Error> {
      Ok(Self::dir()?.exists())
   }
}

/// Enum to determine which railgun mode to use.
///
/// This is to avoid duplicating ui code for shield and unshield.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RailgunMode {
   Shield,
   Unshield,
}

impl RailgunMode {
   pub fn is_shield(&self) -> bool {
      matches!(self, RailgunMode::Shield)
   }

   pub fn is_unshield(&self) -> bool {
      matches!(self, RailgunMode::Unshield)
   }
}

pub struct ShieldUi {
   open: bool,
   mode: RailgunMode,
   currency: Currency,
   /// The NFT being shielded, when one is. `None` means the fungible `currency` is the asset: the two are
   /// exclusive, and picking either clears the other.
   nft: Option<NftToken>,
   /// The quantity of the selected ERC-1155, as typed.
   ///
   /// An ERC-721 moves exactly one and never reads this — there is no field for it. An ERC-1155 is a
   /// quantity of an id, so the number is what moves, counted in whole units.
   nft_amount: String,
   amount_field: AmountField,
   recipient: String,
   recipient_name: Option<String>,
   search_query: String,
   size: (f32, f32),
   price_syncing: bool,
   syncing_balance: bool,
   sending_tx: bool,
   last_price_update: HashMap<Address, Instant>,
   /// Emergency path: submit unshield from the user's EOA (breaks anonymity).
   self_broadcast: bool,
   /// Post unshield call to unwrap WETH to ETH
   unwrap_to_eth: bool,
   /// Bundler JSON-RPC URL for paymaster UserOps (ignored when self_broadcast).
   bundler_url: String,
   /// Set when user clicks Merge Notes; consumed by central panel.
   open_merge_notes: bool,
   /// Broadcast options window
   open_broadcast_options: bool,
   /// Optional memo for unshield (written on change notes for private history).
   memo: String,
}

impl ShieldUi {
   pub fn new() -> Self {
      Self {
         open: false,
         mode: RailgunMode::Shield,
         currency: Currency::from(NativeCurrency::from_chain_id(1).unwrap()),
         nft: None,
         nft_amount: String::new(),
         amount_field: AmountField::new(),
         recipient: String::new(),
         recipient_name: None,
         search_query: String::new(),
         size: (500.0, 620.0),
         price_syncing: false,
         syncing_balance: false,
         sending_tx: false,
         last_price_update: HashMap::new(),
         self_broadcast: false,
         unwrap_to_eth: false,
         bundler_url: BundlerUrl::default().url,
         open_merge_notes: false,
         open_broadcast_options: false,
         memo: String::new(),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, mode: RailgunMode) {
      self.mode = mode;
      self.open = true;
   }

   pub fn close(&mut self) {
      let currency = self.currency.clone();
      let bundler_url = self.bundler_url.clone();
      *self = Self::new();
      self.currency = currency;
      self.bundler_url = bundler_url;
   }

   pub fn set_bundler_url(&mut self, url: String) {
      self.bundler_url = url;
   }

   pub fn set_mode(&mut self, mode: RailgunMode) {
      self.mode = mode;
   }

   pub fn default_currency(&mut self, chain_id: u64) {
      let currency = match self.mode {
         RailgunMode::Shield => Currency::from(NativeCurrency::from(chain_id)),
         RailgunMode::Unshield => Currency::from(ERC20Token::wrapped_native_token(chain_id)),
      };
      self.currency = currency;
   }

   pub fn clear_recipient(&mut self) {
      self.recipient_name = None;
      self.recipient = String::new();
   }

   pub fn clear_search_query(&mut self) {
      self.search_query = String::new();
   }

   fn open_broadcast_options(&mut self) {
      self.open_broadcast_options = true;
   }

   /// If the user clicked Merge Notes this frame, return the currency to merge.
   pub fn take_open_merge_notes(&mut self) -> Option<Currency> {
      if self.open_merge_notes {
         self.open_merge_notes = false;
         Some(self.currency.clone())
      } else {
         None
      }
   }

   fn show_not_supported(&self, theme: &Theme, ui: &mut Ui) {
      let frame = theme.frame1;
      ui.vertical_centered(|ui| {
         frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
            ui.spacing_mut().button_padding = theme.button_padding;

            let text = RichText::new("Railgun is not supported for the selected chain")
               .size(theme.typography.very_large);
            ui.label(text);
         });
      });
   }

   fn show_not_enabled(&self, theme: &Theme, ui: &mut Ui) {
      let frame = theme.frame1;
      ui.vertical_centered(|ui| {
         frame.show(ui, |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
            ui.spacing_mut().button_padding = theme.button_padding;

            let text = RichText::new("Railgun is disabled").size(theme.typography.very_large);
            ui.label(text);
            ui.label(
               RichText::new("Enable it in Settings/Railgun to shield and unshield.")
                  .size(theme.typography.large),
            );
         });
      });
   }

   /// True when unshield is pointed at a bundler other than the chain default.
   fn uses_custom_bundler(&self, chain_id: u64) -> bool {
      !self.self_broadcast && self.bundler_url.trim() != default_bundler_url(chain_id).as_str()
   }

   /// The broadcaster relays the unshield from the user's IP address.
   fn show_ip_warning(&self, theme: &Theme, chain_id: u64, ui: &mut Ui) {
      if !self.mode.is_unshield() || self.self_broadcast {
         return;
      }

      let (text, color) = if self.uses_custom_bundler(chain_id) {
         (
            "Using a custom bundler. Its operator sees your IP address and your unshields.",
            theme.colors.info,
         )
      } else {
         (
            "Private broadcast keeps the origin of your funds anonymous, but the bundler still sees your IP address. Point Broadcast options at a bundler you run yourself to keep it private.",
            theme.colors.text_muted,
         )
      };

      let content_width = ui.available_width();
      ui.vertical(|ui| {
         ui.set_width(content_width);
         ui.add(
            Label::new(
               RichText::new(text).size(theme.typography.small).color(color),
               None,
            )
            .wrap()
            .fill_width(true)
            .interactive(false),
         );
      });
   }

   /// The "Select NFT" / "Change" button: opens the picker on its NFT list.
   ///
   /// The same affordance SendCrypto's NFT selector has. The picker opens listing what this mode moves
   /// — shielded tokens for an unshield, public ones for a shield — and on its NFT tab; the picker's
   /// own Tokens/NFTs switch is still there, which is the way back to a fungible token.
   fn nft_select_button(
      theme: &Theme,
      nft_selected: bool,
      privacy_mode: bool,
      token_selection: &mut TokenSelectionWindow,
      chain_id: u64,
      owner: Address,
      ui: &mut Ui,
   ) {
      let text = RichText::new(match nft_selected {
         true => "Change",
         false => "Select NFT",
      })
      .size(theme.typography.normal);

      let button = Button::new(text).min_size(vec2(90.0, 25.0)).visuals(theme.button_visuals());

      if ui.add(button).clicked() {
         token_selection.open(privacy_mode, chain_id, owner);
         token_selection.set_mode(PickerMode::Nft);
      }
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      token_selection: &mut TokenSelectionWindow,
      recipient_selection: &mut RecipientSelectionWindow,
      contacts_ui: &mut ContactsUi,
      ui: &mut Ui,
   ) {
      show_with_fade(ui, "shield_ui_fade", self.open, |ui| {
         if !ctx.railgun_is_supported(ctx.chain) {
            self.show_not_supported(theme, ui);
            return;
         }

         if !ctx.is_railgun_enabled(ctx.chain.id()) {
            self.show_not_enabled(theme, ui);
            return;
         }

         self.broadcast_options(theme, ctx.chain.id(), ui);

         let frame = theme.frame1;

         ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
                  ui.spacing_mut().button_padding = theme.button_padding;

                  let text_edit_visuals = theme.text_edit_visuals();

                  let title = match self.mode {
                     RailgunMode::Shield => "Shield",
                     RailgunMode::Unshield => "Unshield",
                  };

                  ui.horizontal(|ui| {
                  let ui_size = vec2(ui.available_width(), 20.0);
                  ui.allocate_ui(ui_size, |ui| {
                     ui.vertical_centered(|ui| {
                     ui.label(RichText::new(title).size(theme.typography.heading));
                     });
                  });

                  ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  if self.mode.is_unshield() {
                     ui.add_space(6.0);
                     let merge_text =
                        RichText::new("Merge Notes").size(theme.typography.normal);
                     let merge_btn = Button::new(merge_text)
                        .min_size(vec2(100.0, 30.0))
                        .visuals(theme.button_visuals());
                     if ui
                        .add_enabled(!self.sending_tx && self.currency.is_erc20(), merge_btn)
                        .on_hover_text(
                           "Combine small private notes into larger ones so unshields stay efficient.",
                        )
                        .clicked()
                     {
                        // Handled by caller via `take_open_merge_notes`.
                        self.open_merge_notes = true;
                     }
                  }
                  });
               });

                  let owner = ctx.current_wallet_info().address;
                  let chain = ctx.chain;

                  // Keep default bundler URL in sync with the active chain when still on public Pimlico.
                  if self.mode.is_unshield() {
                     let default_for_chain = default_bundler_url(chain.id());
                     let looks_like_default = self.bundler_url.contains("public.pimlico.io");
                     if self.bundler_url.is_empty() || looks_like_default {
                        if !self.bundler_url.contains(&format!("/{}/rpc", chain.id())) {
                           self.bundler_url = default_for_chain;
                        }
                     }
                  }

                  ui.add_space(theme.spacing.md);

                  self.show_ip_warning(theme, chain.id(), ui);

                  ui.add_space(theme.spacing.md);

                  let inner_frame = theme.frame2;

                  // Currency Selection
                  let balance = self.balance_for_mode(ctx, owner);
                  let max_amount = balance.clone();

                  let amount = self.amount_field.amount.clone();
                  let currency = self.currency.clone();
                  let data_syncing = self.price_syncing || self.syncing_balance;
                  let should_calculate_price = self.should_calculate_price(&currency);
                  // An NFT has no pool price, so there is nothing to value it by.
                  let value = match &self.nft {
                     Some(_) => NumericValue::default(),
                     None => value(ctx, currency, amount, should_calculate_price),
                  };

                  // Token list: public tokens for shield, private notes for unshield.
                  let token_privacy_mode = self.mode.is_unshield();
                  // Recipient: 0zk for shield, public 0x for unshield.
                  let recipient_privacy_mode = self.mode.is_shield();

                  inner_frame.show(ui, |ui| {
                     ui.set_width(ui.available_width());

                     // An NFT has no amount to enter — it is one token, and `RailgunAsset` fixes its value
                     // at 1 — so the field is replaced by what is being moved. Either way there is a way
                     // to change the choice: picking an NFT used to be a one-way door until the window
                     // was reopened.
                     if let Some(nft) = &self.nft {
                        let collection = nft_collection_name(
                           ctx.nft_db.get_collection(chain.id(), nft.collection).as_ref(),
                           nft.collection,
                        );

                        let icon = icons
                           .nft_icon_x64(
                              chain.id(),
                              nft.collection,
                              nft.token_id,
                              theme.image_tint_recommended,
                           )
                           .fit_to_exact_size(vec2(24.0, 24.0));

                        let label = Label::new(
                           RichText::new(format!("{} #{}", collection, nft.token_id))
                              .size(theme.typography.large),
                           Some(icon),
                        )
                        .spacing(3.0)
                        .interactive(false);

                        // The shape SendCrypto's NFT selector uses: what is being moved on the left, a
                        // way to change it on the right.
                        ui.horizontal(|ui| {
                           ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                              ui.add(label);
                           });

                           ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                              Self::nft_select_button(
                                 theme,
                                 self.nft.is_some(),
                                 token_privacy_mode,
                                 token_selection,
                                 chain.id(),
                                 owner,
                                 ui,
                              );
                           });
                        });

                        // An ERC-1155 moves a quantity of an id, so it needs a number; an ERC-721 is one
                        // token and has nothing to ask for.
                        if nft.standard == NftStandard::Erc1155 {
                           let max = Self::nft_max(nft, self.mode, ctx, owner);

                           ui.horizontal(|ui| {
                              ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                                 ui.add(
                                    Label::new(
                                       RichText::new("Amount").size(theme.typography.large),
                                       None,
                                    )
                                    .interactive(false),
                                 );
                              });

                              ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                 Self::nft_amount_input(&mut self.nft_amount, theme, max, ui);
                              });
                           });
                        }
                     } else {
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
                     }
                  });

                  if let Some(nft) = token_selection.get_selected_nft().cloned() {
                     // An NFT and a fungible token are exclusive: picking one clears the other. A newly
                     // picked token starts with no quantity, rather than inheriting the last one's.
                     self.nft = Some(nft);
                     self.nft_amount.clear();
                     token_selection.reset();
                     self.sync_balance(owner);
                  } else if let Some(currency) = token_selection.get_selected_currency() {
                     self.currency = currency.clone();
                     self.nft = None;
                     self.nft_amount.clear();
                     token_selection.reset();
                     self.sync_balance(owner);
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
                        let hint_text = match recipient_privacy_mode {
                           false => "Search contacts, ENS or enter an address",
                           true => "Search contacts or enter a 0zk address",
                        };

                        let hint = RichText::new(hint_text)
                           .size(theme.typography.normal)
                           .color(theme.colors.text_muted);

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

                  if self.mode.is_unshield() {
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

                  if self.mode.is_unshield() {
                     self.unshield_options(theme, ui);
                  }

                  // Read before the address fields are moved out of `recipient`.
                  let recipient_chain = recipient.chain;

                  let recipient_str = if self.mode.is_shield() {
                     recipient.zk_address
                  } else {
                     recipient.evm_address
                  };

                  ui.add_space(10.0);

                  self.action_button(ctx, theme, owner, recipient_str, recipient_chain, ui);
               });
            });
      });
   }

   fn unshield_options(&mut self, theme: &Theme, ui: &mut Ui) {
      let inner_frame = theme.frame2;

      inner_frame.show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         ui.horizontal(|ui| {
            let text = RichText::new("Self-broadcast").size(theme.typography.large);
            let checkbox = Checkbox::new(&mut self.self_broadcast, text);
            ui.add(checkbox);

            ui.add_space(10.0);

            let q_mark = RichText::new("Breaks Anonymity").size(theme.typography.large);
            let badge = Badge::new(q_mark, BadgeTone::Warning);
            let tip_text = RichText::new(SELF_BROADCAST_TIP).size(theme.typography.normal);
            ui.add(badge).on_hover_text(tip_text);
         });

         // ? Maybe in the future we could replace this with swaps
         // ? Eg. going from USDC to ETH and not just limited to WETH > ETH
         if self.currency.is_native_wrapped() && !self.self_broadcast {
            ui.horizontal(|ui| {
               let text = RichText::new("Unwrap to ETH").size(theme.typography.large);
               let checkbox = Checkbox::new(&mut self.unwrap_to_eth, text);
               ui.add(checkbox);

               ui.add_space(10.0);

               let text = RichText::new("For empty wallets without funds for gas")
                  .size(theme.typography.normal);
               let badge = Badge::new(text, BadgeTone::Info);
               let tip_text = RichText::new(UNWRAP_TO_ETH_TIP).size(theme.typography.normal);
               ui.add(badge).on_hover_text(tip_text);
            });
         }

         let text = RichText::new("Broadcast options").size(theme.typography.normal);
         let button = Button::new(text).visuals(theme.button_visuals());
         ui.horizontal(|ui| {
            if ui.add(button).clicked() {
               self.open_broadcast_options();
            }
         });
      });
   }

   fn broadcast_options(&mut self, theme: &Theme, chain_id: u64, ui: &mut Ui) {
      if !self.open_broadcast_options {
         return;
      }

      let text_edit_visuals = theme.text_edit_visuals();

      let title = RichText::new("Advanced broadcast options")
         .size(theme.typography.large)
         .color(theme.colors.text);

      let id = Id::new("shield_ui_advanced_broadcast_options");
      let mut open = self.open_broadcast_options;
      let mut ok_clicked = false;

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
            ui.spacing_mut().button_padding = theme.button_padding;

            ui.add_enabled_ui(!self.self_broadcast, |ui| {
               ui.set_width(450.0);
               ui.set_max_height(250.0);

               let ui_size = vec2(ui.available_width() * 0.9, 45.0);

               ui.allocate_ui(ui_size, |ui| {
                  ui.horizontal_centered(|ui| {
                     ui.label(RichText::new("Bundler URL").size(theme.typography.normal));

                     ui.add_space(8.0);

                     ui.add(
                        SecureTextEdit::singleline(&mut self.bundler_url)
                           .visuals(text_edit_visuals)
                           .hint_text(
                              RichText::new("https://public.pimlico.io/v2/{chainId}/rpc")
                                 .size(theme.typography.small)
                                 .color(theme.colors.text_muted),
                           )
                           .desired_width(ui.available_width())
                           .margin(Margin::same(6))
                           .font(FontId::proportional(theme.typography.small)),
                     );
                  });
               });

               ui.horizontal(|ui| {
                  let text = RichText::new("Reset to default").size(theme.typography.small);
                  let button = Button::new(text).visuals(theme.button_visuals());

                  if ui.add(button).clicked() {
                     self.bundler_url = default_bundler_url(chain_id);
                     let url = BundlerUrl::new(self.bundler_url.clone());
                     RT.spawn_blocking(move || {
                        persist_bundler_url(url);
                     });
                  }

                  ui.add_space(10.0);

                  let text = RichText::new("Save").size(theme.typography.small);
                  let button = Button::new(text).visuals(theme.button_visuals());

                  if ui.add(button).clicked() {
                     let url = BundlerUrl::new(self.bundler_url.clone());
                     RT.spawn_blocking(move || {
                        persist_bundler_url(url);
                     });
                  }
               });

               ui.add_space(theme.spacing.md);

               let text = "Uses Railgun Privacy Paymaster.\nFee is paid from private WETH balance.\nPoint this at a self-hosted Alto for less reliance on public Pimlico.";

               ui.label(RichText::new(text).size(theme.typography.normal));

               let note = "The bundler operator sees your IP address and your unshields. A VPN hides your IP address, a bundler you run yourself hides both.";
               ui.add(
                  Label::new(
                     RichText::new(note)
                        .size(theme.typography.small)
                        .color(theme.colors.text_muted),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            });

            if self.self_broadcast {
               ui.label(
                  RichText::new("Bundler options disabled while self-broadcast is enabled.")
                     .size(theme.typography.small)
                     .color(theme.colors.warning),
               );
            }

            ui.add_space(10.0);

               let text = RichText::new("OK").size(theme.typography.normal);
               let button = Button::new(text).visuals(theme.button_visuals());

               ui.vertical_centered(|ui| {
                  if ui.add(button).clicked() {
                     ok_clicked = true;
                  }
               });
         });

      if ok_clicked {
         open = false;
      }

      self.open_broadcast_options = open;
   }

   fn valid_recipient(&self, recipient: &str) -> bool {
      if self.mode.is_unshield() {
         let addr = Address::from_str(recipient).unwrap_or(Address::ZERO);
         return !addr.is_zero();
      }

      true
   }

   fn action_button(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      owner: Address,
      recipient: String,
      recipient_chain: Option<u64>,
      ui: &mut Ui,
   ) {
      let is_synced = ctx.railgun_status().synced(ctx.chain.id());
      let button_visuals = theme.button_visuals();
      let sending_tx = self.sending_tx;
      // What the selected NFT moves, if one is selected at all: one for an ERC-721, whatever was typed for
      // an ERC-1155 — and `None` inside the `Some` means the field does not hold a usable count yet.
      let nft_quantity = self.nft.as_ref().map(|nft| nft_quantity(nft, &self.nft_amount));
      let nft_selected = self.nft.is_some();

      let valid_amount = match nft_quantity {
         Some(quantity) => quantity.is_some(),
         None => self.valid_amount(),
      };
      let has_balance = match nft_quantity {
         Some(quantity) => {
            let max = self.nft.as_ref().and_then(|nft| Self::nft_max(nft, self.mode, ctx, owner));

            // An unknown ceiling is not a refusal: only a chain that has spoken can say «not enough».
            quantity.map_or(false, |quantity| {
               max.map_or(true, |max| quantity <= max)
            })
         }
         None => self.sufficient_balance(ctx, owner),
      };
      let has_entered_amount = match &self.nft {
         // An ERC-721 has no field to have entered anything in; an ERC-1155 needs one filled.
         Some(nft) => nft.standard == NftStandard::Erc721 || !self.nft_amount.trim().is_empty(),
         None => !self.amount_field.amount.is_empty(),
      };
      let has_recipient = !recipient.trim().is_empty();
      let valid_recipient = self.valid_recipient(&recipient);
      let valid_token = if self.mode == RailgunMode::Unshield {
         nft_selected || self.currency.is_erc20()
      } else {
         true
      };

      // An unshield pays out to a public address on the active chain, so a chain-specific
      // recipient has to agree with it. A shield sends to a `0zk` address, which has no chain.
      let wrong_chain = match self.mode.is_unshield() {
         true => recipient_chain.filter(|chain| *chain != ctx.chain.id()),
         false => None,
      };

      let valid_inputs = has_balance
         && has_entered_amount
         && valid_amount
         && valid_token
         && has_recipient
         && valid_recipient
         && wrong_chain.is_none()
         && !sending_tx
         && is_synced;

      let mut button_text = match self.mode {
         RailgunMode::Shield => "Shield".to_string(),
         RailgunMode::Unshield => {
            if self.self_broadcast {
               "Unshield (self-broadcast)".to_string()
            } else {
               "Unshield (private broadcast)".to_string()
            }
         }
      };

      if has_entered_amount && !valid_amount {
         button_text = "Invalid Amount".to_string();
      }

      if !has_balance {
         button_text = format!("Insufficient {} Balance", self.currency.symbol());
      }

      if !valid_token {
         button_text = "Invalid Token".to_string();
      }

      if !has_recipient {
         button_text = "Enter Recipient".to_string();
      }

      if !valid_recipient {
         button_text = "Invalid Recipient".to_string();
      }

      if !is_synced {
         button_text = "Railgun is not synced".to_string();
      }

      // Last, so it wins: sending to a recipient resolved for another chain is the mistake worth
      // blocking, and the picker has already asked before switching.
      if let Some(chain) = wrong_chain {
         button_text = match ChainId::new(chain) {
            Ok(chain) => format!("Switch to {} to unshield", chain.name()),
            Err(_) => "Unsupported chain".to_string(),
         };
      }

      let text = RichText::new(button_text).size(theme.typography.large);
      let send = Button::new(text)
         .min_size(vec2(ui.available_width() * 0.8, 45.0))
         .visuals(button_visuals);

      if ui.add_enabled(valid_inputs, send).clicked() {
         self.sending_tx = true;
         self.send_transaction(ctx, recipient);
      }
   }

   fn send_transaction(&mut self, ctx: &mut ZeusContext, recipient: String) {
      let chain = ctx.chain;
      let from = ctx.current_wallet_info().address;

      // The two are exclusive: an NFT wins when one is selected, and it fixes its own value at 1.
      let asset = match &self.nft {
         Some(nft) => RailgunAsset::Nft(nft.clone()),
         None => RailgunAsset::Fungible(self.currency.clone()),
      };

      let amount = match &self.nft {
         // An ERC-1155 moves the count that was typed — whole units, so there are no decimals to scale it
         // by. An ERC-721 moves exactly one and has no field: `value()` answers one for it whatever arrives
         // here, but the quantity is still what the validation agreed on.
         Some(nft) => match nft_quantity(nft, &self.nft_amount) {
            Some(quantity) => NumericValue::format_wei(U256::from(quantity), 0),
            None => NumericValue::default(),
         },
         None => NumericValue::parse_to_wei(
            &self.amount_field.amount,
            self.currency.decimals(),
         ),
      };

      ctx.railgun_status.set_op_in_progress(chain.id(), true);

      if self.mode.is_shield() {
         RT.spawn(async move {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
            });

            match shield(ctx.clone(), chain, asset, amount, from, recipient).await {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open(format!("Transaction Error: {}", e.to_string()));
                     gui.request_repaint();
                  });
               }
            }

            ctx.write(|ctx| {
               ctx.railgun_status.set_op_in_progress(chain.id(), false);
            });
         });
      } else {
         let self_broadcast = self.self_broadcast;
         // Unwrapping WETH to ETH is a fungible-only call: there is nothing to unwrap for an NFT.
         let unwrap_to_eth = self.unwrap_to_eth && self.nft.is_none();
         let bundler_url = self.bundler_url.clone();
         let memo = self.memo.clone();
         // Unshield futures are not `Send` (`PimlicoBundler` / `&dyn Signer` across awaits).
         // Do NOT spin up a nested current_thread runtime: revm's ForkDB uses
         // `tokio::task::block_in_place`, which panics outside a multi-thread runtime.
         // Drive the non-Send future on the existing multi-thread `RT` via `block_on`
         // from a blocking thread (no Send bound, block_in_place still works).
         RT.spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let result = RT.block_on(unshield(
               ctx.clone(),
               chain,
               asset,
               amount,
               from,
               recipient,
               self_broadcast,
               unwrap_to_eth,
               bundler_url,
               memo,
            ));

            match result {
               Ok(_) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                     gui.loading_window.reset();
                     gui.request_repaint();
                  });
               }
               Err(e) => {
                  SHARED_GUI.write(|gui| {
                     gui.shield_ui.sending_tx = false;
                     gui.notification.reset();
                     gui.loading_window.reset();
                     gui.msg_window.open(format!("Unshield Error: {}", e.to_string()));
                     gui.request_repaint();
                  });
               }
            }

            ctx.write(|ctx| {
               ctx.railgun_status.set_op_in_progress(chain.id(), false);
            });
         });
      }
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

   fn sync_balance(&mut self, owner: Address) {
      self.syncing_balance = true;
      let currency = self.currency.clone();
      let chain = currency.chain_id();
      let privacy = self.mode.is_unshield();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

         if privacy {
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
            gui.shield_ui.syncing_balance = false;
         });
      });
   }

   /// How much of the selected NFT the wallet can move, from the side this mode reads.
   ///
   /// A shield moves what is owned publicly, which the balance manager asks the chain for — `None` there
   /// means nobody has asked, not that there is none. An unshield moves what is shielded, and only the
   /// private scan's notes know that, so a token their map does not mention has nothing to spend.
   fn nft_max(
      nft: &NftToken,
      mode: RailgunMode,
      ctx: &mut ZeusContext,
      owner: Address,
   ) -> Option<u64> {
      let chain = ctx.chain.id();

      match mode.is_unshield() {
         true => Some(ctx.read_wallet_state(|ws| {
            ws.portfolio_db
               .get(chain, owner)
               .private_nft_amounts()
               .get(&(nft.collection, nft.token_id))
               .copied()
               .unwrap_or(0)
         })),
         false => ctx.get_nft_balance(chain, owner, nft.collection, nft.token_id),
      }
   }

   /// The ERC-1155 quantity input: whole numbers, with what the wallet can move as the ceiling.
   ///
   /// Call inside a right-to-left layout. The hint is added first so that it lands to the *right* of the
   /// box, reading as «[ 3 ] of 5».
   fn nft_amount_input(nft_amount: &mut String, theme: &Theme, max: Option<u64>, ui: &mut Ui) {
      if let Some(max) = max {
         ui.add(
            Label::new(
               RichText::new(format!("of {max}"))
                  .size(theme.typography.normal)
                  .color(theme.colors.text_muted),
               None,
            )
            .interactive(false),
         );
         ui.add_space(6.0);
      }

      let hint = RichText::new("0").color(theme.colors.text_muted).size(theme.typography.large);

      let input = SecureTextEdit::singleline(nft_amount)
         .visuals(theme.text_edit_visuals())
         .font(FontId::proportional(theme.typography.large))
         .hint_text(hint)
         .margin(Margin::same(8))
         .desired_width(110.0);

      ui.add(input);
   }

   fn valid_amount(&self) -> bool {
      let amount = self.amount_field.amount.parse().unwrap_or(0.0);
      amount > 0.0
   }

   fn balance_for_mode(&self, ctx: &mut ZeusContext, owner: Address) -> NumericValue {
      if self.mode.is_shield() {
         return ctx.get_currency_balance(ctx.chain.id(), owner, &self.currency);
      }

      // Private note balances from portfolio cache
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

   fn sufficient_balance(&self, ctx: &mut ZeusContext, sender: Address) -> bool {
      let balance = self.balance_for_mode(ctx, sender);
      let amount = NumericValue::parse_to_wei(
         &self.amount_field.amount,
         self.currency.decimals(),
      );
      balance.wei() >= amount.wei()
   }
}

/// The quantity a selected NFT moves, from the field the user typed in.
///
/// An ERC-721 moves exactly one and has no field, so it is always `Some(1)`. An ERC-1155 moves what was
/// typed, in whole units — there are no decimals to scale a count by — and anything that is not a usable
/// positive number is `None`. Both the button's validation and the value that gets sent read this one
/// answer, so they cannot disagree about what is about to move.
fn nft_quantity(nft: &NftToken, typed: &str) -> Option<u64> {
   match nft.standard {
      NftStandard::Erc721 => Some(1),
      NftStandard::Erc1155 => match typed.trim().parse::<u64>() {
         Ok(amount) if amount > 0 => Some(amount),
         _ => None,
      },
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
      let chain = currency.chain_id();

      RT.spawn(async move {
         let now = Instant::now();
         let ctx = SHARED_GUI.write(|gui| {
            gui.shield_ui.price_syncing = true;
            gui.shield_ui.last_price_update.insert(currency.address(), now);
            gui.ctx.clone()
         });
         let price_manager = ctx.price_manager();
         let pool_manager = ctx.pool_manager();
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
                  gui.shield_ui.price_syncing = false;
               });
            }
            Err(_e) => {
               SHARED_GUI.write(|gui| {
                  gui.shield_ui.price_syncing = false;
               });
               #[cfg(feature = "dev")]
               tracing::error!("Error calculating price: {:?}", _e);
            }
         }
      });
   }

   value
}

async fn shield(
   ctx: ZeusCtx,
   chain: ChainId,
   asset: RailgunAsset,
   amount: NumericValue,
   from: Address,
   recipient: String,
) -> Result<(), anyhow::Error> {
   let railgun_provider = railgun_ready(ctx.clone(), chain).await?;

   let recipient = match RailgunAddress::from_zk_address(&recipient) {
      Ok(address) => address,
      Err(e) => {
         return Err(anyhow!("Invalid Railgun Address {}", e));
      }
   };

   let railgun_address = railgun_provider.railgun_address();
   let relay_adapt = railgun_provider.chain_config().relay_adapt_contract;
   let is_native = asset.is_native();

   // ERC-20 still needs an on-chain approval of RailgunSmartWallet before shield, and an NFT collection
   // needs `setApprovalForAll` — one approval that covers every token in it.
   // Native ETH uses RelayAdapt wrap+shield in one self-broadcast tx (no approval).
   if !is_native {
      match &asset {
         RailgunAsset::Fungible(currency) => {
            let token = currency.to_erc20().into_owned();

            ensure_allowance(
               ctx.clone(),
               chain,
               from,
               &token,
               railgun_address,
               amount.wei(),
               "Railgun",
               "Token approval required to shield",
            )
            .await?;
         }
         RailgunAsset::Nft(nft) => {
            ensure_approval_for_all(
               ctx.clone(),
               chain,
               from,
               nft.collection,
               nft.standard,
               railgun_address,
               "Railgun",
               "Collection approval required to shield",
            )
            .await?;
         }
      }
   }

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
   });

   // An ERC-721 moves exactly one: the token id is the asset, so the amount the UI holds is not consulted.
   let amount_u128: u128 = asset.value(amount.wei()).try_into()?;

   let shield_tx = {
      let mut rng = ChaCha12Rng::from_os_rng();
      let builder = railgun_provider.shield();
      let builder = if is_native {
         builder.shield_native(recipient.clone(), amount_u128)
      } else {
         builder.shield(recipient.clone(), asset.asset_id(), amount_u128)
      };
      builder.build(&mut rng)?
   };

   let shield_tx = shield_tx
      .into_iter()
      .next()
      .ok_or_else(|| anyhow!("Shield builder returned no transaction"))?;

   let calldata = shield_tx.data.clone();
   let interact_to = shield_tx.to;
   let value = shield_tx.value;

   let (block, block_id) = pinned_head(ctx.clone(), chain, BlockId::latest()).await?;

   let eth_balance_before_fut = native_balance_at(ctx.clone(), chain, from, block_id);

   // Prefetch accounts and storage for the sim
   let mut accounts = Vec::new();
   accounts.push(AccountPrefetch::eoa(from));
   accounts.push(AccountPrefetch::contract(
      asset.asset_id().address(),
   ));
   accounts.push(AccountPrefetch::contract(railgun_address));
   accounts.push(AccountPrefetch::contract(interact_to));
   accounts.push(AccountPrefetch::contract(relay_adapt));
   accounts.push(AccountPrefetch::eoa(block.header.beneficiary));

   let common_accounts = railgun_common_accounts(chain.id());
   accounts.extend(common_accounts.into_iter().map(AccountPrefetch::contract));

   let sim = simulate_on_fork(
      ctx.clone(),
      chain,
      ForkPrefetch {
         block,
         accounts,
         storage: StoragePrefetch::Railgun(railgun_address),
      },
      ForkSimRequest {
         from,
         interact_to,
         call_data: calldata.clone(),
         value,
         gas_limit: None,
         authorization_list: vec![],
      },
   )
   .await?;

   let ForkSim {
      sim_res,
      logs,
      balance_after: eth_balance_after,
      ..
   } = sim;

   let mut shield_events = Vec::new();

   for log in &logs {
      if let Ok(params) = ShieldParams::from_log(ctx.clone(), chain.id(), log).await {
         shield_events.extend(params);
      }
   }

   let mut shield_params = expect_single_event(
      shield_events,
      "More than one shield event found",
      || "No shield event found".to_string(),
   )?;

   shield_params.recipient = Some(recipient.address.clone());

   let eth_balance_before = eth_balance_before_fut.await?;

   let mut tx_analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      Some(true),
      calldata.clone(),
      value,
      logs,
      sim_res.tx_gas_used(),
      eth_balance_before,
      eth_balance_after,
      vec![],
   )
   .await?;

   // Transact logs are not public ERC-20 transfers, so record the intent.
   tx_analysis.set_main_event(DecodedEvent::Shield(shield_params));

   let (_, _) = send_transaction(
      ctx.clone(),
      true,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(calldata)
         .value(value)
         .analysis(tx_analysis),
      SendTxOptions {
         dapp: "Railgun".to_string(),
         keep_intent_event: true,
         ..Default::default()
      },
   )
   .await?;

   RT.spawn(settle_railgun_op(
      ctx,
      chain,
      from,
      match &asset {
         // An NFT has no ERC-20 balance to refresh after the fact; it shows up through the private scan.
         RailgunAsset::Fungible(currency) if !is_native => Some(currency.to_erc20().into_owned()),
         _ => None,
      },
   ));

   Ok(())
}

fn persist_bundler_url(url: BundlerUrl) {
   let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
   let key = match ctx.read_vault(|vault| vault.wallet_state_key()) {
      Ok(k) => k,
      Err(e) => {
         error!("Error saving Bundler URL: {:?}", e);
         return;
      }
   };
   if let Err(e) = url.save(&key) {
      error!("Error saving Bundler URL: {:?}", e);
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   fn nft(standard: NftStandard) -> NftToken {
      NftToken {
         chain_id: 1,
         collection: Address::from([0xbc; 20]),
         token_id: U256::from(1),
         standard,
         metadata_uri: None,
      }
   }

   /// An ERC-721 moves exactly one and has no field, so whatever `nft_amount` happens to hold cannot
   /// change what it moves. An ERC-1155 moves a whole number the user typed — and only a usable positive
   /// one: an empty field, a zero, a decimal or words are all «nothing to move», which is what the button
   /// reads to stay disabled.
   #[test]
   fn an_erc721_moves_one_and_an_erc1155_moves_what_was_typed() {
      let erc721 = nft(NftStandard::Erc721);
      assert_eq!(nft_quantity(&erc721, ""), Some(1));
      assert_eq!(
         nft_quantity(&erc721, "0"),
         Some(1),
         "the field is never read"
      );
      assert_eq!(nft_quantity(&erc721, "not a number"), Some(1));

      let erc1155 = nft(NftStandard::Erc1155);
      assert_eq!(nft_quantity(&erc1155, "3"), Some(3));
      assert_eq!(
         nft_quantity(&erc1155, " 3 "),
         Some(3),
         "surrounding space is fine"
      );

      assert_eq!(nft_quantity(&erc1155, ""), None);
      assert_eq!(nft_quantity(&erc1155, "0"), None);
      assert_eq!(nft_quantity(&erc1155, "1.5"), None);
      assert_eq!(nft_quantity(&erc1155, "-1"), None);
      assert_eq!(nft_quantity(&erc1155, "three"), None);
   }

   #[test]
   fn test_bundler_url_seal_open_roundtrip() {
      let key = WalletStateKey::generate().unwrap();
      let url = BundlerUrl::new("https://example.invalid/rpc".into());
      let sealed = key.seal_json(&url, BUNDLER_URL_AAD).unwrap();
      let loaded: BundlerUrl = key.open_json(&sealed, BUNDLER_URL_AAD).unwrap();
      assert_eq!(loaded.url, url.url);
      assert!(key.open_json::<BundlerUrl>(&sealed, b"wrong-aad").is_err());
   }
}
