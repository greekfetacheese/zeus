use eframe::egui::{
   Align, Checkbox, FontId, Id, Layout, Margin, Order, RichText, Spinner, Ui, vec2,
};

use std::{
   collections::HashMap,
   str::FromStr,
   sync::Arc,
   time::{Duration, Instant},
};

use crate::core::types::EnsRecipientGuard;
use crate::core::urls::ZeusUrl;
use crate::core::{
   DecodedEvent, SendTxOptions, SendTxRequest, ShieldParams, TransactionAnalysis, WalletStateKey,
   ZeusContext, ZeusCtx, bundler_url_dir, ensure_allowance, ensure_approval_for_all,
   ensure_erc721_approve, send_transaction,
};
use crate::{
   gui::ui::common::show_with_fade,
   utils::{TimeStamp, write_private_atomic},
};

use super::privacy::{
   LARGE_POOL_SHARE, PoolDepth, RecipientLink, UnshieldPrivacy, assess_shield, assess_unshield,
   shallow_pool_warnings,
};
use super::{RailgunAsset, expect_single_event, railgun_ready, settle_railgun_op};
use crate::assets::icons::Icons;
use crate::gui::{
   SHARED_GUI,
   ui::{
      ContactsUi, RecipientSelectionWindow, TokenSelectionWindow,
      common::{AmountField, AmountFieldParams, recipient_field},
      token_selection::{PickerMode, nft_collection_name},
   },
};
use crate::utils::simulate::{
   AccountPrefetch, ForkPrefetch, ForkSim, ForkSimRequest, StoragePrefetch, native_balance_at,
   pinned_head, railgun_common_accounts, simulate_on_fork,
};
use egui_elements::{Button, Label, Modal, SecureTextEdit, Theme};
use elegance::{Badge, BadgeTone};

use zeus_eth::{
   alloy_primitives::{Address, U256},
   alloy_rpc_types::BlockId,
   currency::{Currency, ERC20Token, NativeCurrency},
   nft::{NftStandard, NftToken},
   types::ChainId,
   utils::NumericValue,
};

use zeus_railgun::{
   RailgunAddress,
   caip::AssetId,
   privacy::{RiskBand, UnshieldAmountAdvice},
   rand::SeedableRng,
   rand_chacha::ChaCha12Rng,
};

use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use tracing::{error, warn};

use super::unshield::{default_bundler_url, unshield};

const POOL_UPDATE_TIMEOUT: u64 = 60;

/// Bound ciphertext to this logical slot (AAD).
const BUNDLER_URL_AAD: &[u8] = b"zeus-bundler-url-v1";

const SELF_BROADCAST_TIP: &str = "Submits the unshield from your public wallet. Breaks anonymity only use if private broadcast is unavailable.";
const UNWRAP_TO_ETH_TIP: &str =
   "Unwraps WETH to ETH. Useful if the recipient doesn't have native ETH for gas.";

/// Why the default bundler is Zeus's own: what it is, who pays, and how to stop relying on it.
const PAYMASTER_TIP: &str = "Uses Railgun Privacy Paymaster. Fee is paid from private WETH balance. \
                             Point this at a self-hosted Alto for less reliance on public Pimlico.";

/// How long the amount field is left alone before a privacy check runs.
///
/// The check reads the events snapshot and the chain, so it must not run on every keystroke — but it
/// should land while the user is still deciding, not when they press the button.
const PRIVACY_CHECK_DELAY: Duration = Duration::from_millis(450);

/// What a privacy verdict belongs to.
///
/// A changed input does not make the old verdict stale, it makes it wrong: it would describe a
/// withdrawal nobody is about to send. So a verdict is only ever shown for the key it was computed
/// for, and a newer key drops the older answer.
#[derive(Clone, PartialEq, Eq, Debug)]
struct PrivacyKey {
   chain: u64,
   asset: AssetId,
   /// What is being moved, as typed: an amount for a fungible token, the count and id for an NFT.
   amount: String,
   recipient: Address,
   owner: Address,
}

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
   /// The privacy check's verdict, and the form inputs it belongs to.
   privacy: Option<UnshieldPrivacy>,
   /// The key `privacy` was computed for; also the key whose check is in flight.
   privacy_key: Option<PrivacyKey>,
   /// A key waiting out the debounce before it is checked.
   privacy_pending: Option<(PrivacyKey, Instant)>,
   privacy_loading: bool,
   /// The shield side's pool verdict: how much activity the token's Railgun pool carries, and the
   /// `(chain, asset)` it was computed for.
   ///
   /// A shield is answered by the pool it would join and by nothing else — the amount is public and
   /// the recipient is the user's own `0zk` — so unlike the check above there is no amount and no
   /// address to key on. `None` is "nothing to say": a deep pool, or a check that could not run.
   shield_pool: Option<PoolDepth>,
   shield_pool_key: Option<(u64, AssetId)>,
   /// Whether the tips list is expanded.
   privacy_tips_open: bool,
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
         privacy: None,
         privacy_key: None,
         privacy_pending: None,
         privacy_loading: false,
         shield_pool: None,
         shield_pool_key: None,
         privacy_tips_open: false,
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

   /// Forget the selected NFT and the quantity typed for it.
   ///
   /// Called from the hook that means "the token for this chain changed" (`default_currency`). A
   /// selection names a collection on the chain that was active when it was picked, while
   /// `send_transaction` pairs it with the chain active *now* (`ctx.chain`) — keeping it builds a
   /// transfer of a foreign contract's token. The *mode* is deliberately left alone, unlike the send
   /// view's: here it picks shield or unshield, not fungible-or-NFT, and re-defaulting the token says
   /// nothing about which direction the user is going.
   fn clear_nft(&mut self) {
      self.nft = None;
      self.nft_amount.clear();
   }

   pub fn default_currency(&mut self, chain_id: u64) {
      self.clear_nft();

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

   /// The first tip: the broadcaster relays the unshield from the user's IP address.
   ///
   /// This was three lines of panel copy above the amount field, which the form cannot spare — but it
   /// is standing advice rather than a live verdict, so it belongs with the other tips. It still names
   /// whichever party actually sees the IP, because "a bundler sees your IP" is the wrong sentence
   /// when the user is broadcasting from their own wallet, or has pointed the unshield at someone
   /// else's bundler.
   fn ip_tip(&self, chain_id: u64) -> &'static str {
      if self.self_broadcast {
         "IP address: self-broadcast sends this from your own wallet, so your IP address goes out with \
          the transaction. Use a VPN, and a machine you do not use for your identified life."
      } else if self.uses_custom_bundler(chain_id) {
         "IP address: the bundler set in Broadcast options sees your IP address and your unshields. Use \
          a VPN, and a machine you do not use for your identified life."
      } else {
         "IP address: private broadcast hides the origin of your funds, but the bundler still sees your \
          IP address. Point Broadcast options at a bundler you run yourself, and use a VPN."
      }
   }

   /// Whether the form is pointed at something Zeus refuses to shield.
   fn erc1155_shield_blocked(&self) -> bool {
      self.mode.is_shield()
         && self.nft.as_ref().is_some_and(|nft| nft.standard == NftStandard::Erc1155)
   }

   /// The notice at the shield button, when an ERC-1155 is selected.
   fn show_erc1155_block(&self, theme: &Theme, ui: &mut Ui) {
      let text = "Shielding ERC-1155 is disabled: Railgun does not support it yet, and Zeus will enable \
                  it only after verifying that support.";
      ui.add(
         Label::new(
            RichText::new(text).size(theme.typography.small).color(theme.colors.info),
            None,
         )
         .wrap()
         .fill_width(true)
         .interactive(false),
      );
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

         self.privacy_tips(theme, ctx.chain.id(), ui);

         let frame = theme.frame1;

         // Centred both ways, as the other pages are. The form was anchored to the top only because it
         // was taller than the panel; the memo and the broadcast switches now live in the Broadcast
         // options modal, which gave back the rows they cost, so the card fits where it belongs.
         ui.vertical_centered(|ui| {
            frame.show(ui, |ui| {
                  ui.set_width(self.size.0);
                  ui.set_max_height(self.size.1);
                  ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
                  ui.spacing_mut().button_padding = theme.button_padding;

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

                  recipient_field(
                     ctx,
                     theme,
                     icons.clone(),
                     recipient_selection,
                     contacts_ui,
                     recipient_privacy_mode,
                     send_chain,
                     chain,
                     ui,
                  );

                  let recipient = recipient_selection.get_recipient();

                  // A name whose registration lapsed past its grace period may now belong to
                  // someone else, so the address it resolved to is no longer what the name means.
                  let now = TimeStamp::now_as_secs().unwrap_or_default().timestamp();
                  let recipient_name_lapsed = !recipient.name_binding_trusted(
                     now
                  );

                  // Carried to the confirm step, which re-checks the name with its own clock.
                  let recipient_guard = recipient.ens_guard();

                  // The memo and the broadcast switches are in the Broadcast options modal now: the
                  // form is a fixed stack with no scroll area, so the rows it keeps are rows the action
                  // button gets. The row below still shows what is set in there.
                  if self.mode.is_unshield() {
                     self.broadcast_options_row(theme, ui);
                  }

                  // Read before the address fields are moved out of `recipient`.
                  let recipient_chain = recipient.chain;

                  let recipient_str = if self.mode.is_shield() {
                     recipient.zk_address
                  } else {
                     recipient.evm_address
                  };

                  // What this move reveals, computed off the frame path. The check above is about
                  // the withdrawal; the shield side has one thing to say, about the pool the deposit
                  // would join.
                  if self.mode.is_unshield() {
                     self.poll_privacy(chain, owner, &recipient_str, ui);
                     self.show_privacy_check(theme, ui);
                  } else {
                     // The check reads Railgun's own state, so it is only armed once the chain has
                     // synced: armed earlier it could only fail, and a failed verdict latches.
                     let railgun_synced = ctx.railgun_status().synced(chain.id());
                     self.poll_shield_pool(chain, railgun_synced);
                     self.show_shield_pool_check(theme, ui);
                  }

                  ui.add_space(10.0);

                  // The refusal is explained where it happens: right above the button it disables.
                  if self.erc1155_shield_blocked() {
                     self.show_erc1155_block(theme, ui);
                     ui.add_space(theme.spacing.sm);
                  }

                  self.action_button(
                     ctx,
                     theme,
                     owner,
                     recipient_str,
                     recipient_chain,
                     recipient_name_lapsed,
                     recipient_guard,
                     ui,
                  );
               });
            });
      });
   }

   /// The way into the broadcast options, and what is currently set inside them.
   ///
   /// The memo and the switches live in the modal, but a self-broadcast unshield sends from the user's
   /// own wallet and breaks anonymity: the state has to stay visible from the form, or the setting
   /// would only be discovered by opening the modal again. Both badges are the form's own, so they
   /// cost no row: the row already carries the button.
   fn broadcast_options_row(&mut self, theme: &Theme, ui: &mut Ui) {
      let inner_frame = theme.frame2;

      inner_frame.show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         ui.horizontal(|ui| {
            let text = RichText::new("Broadcast options").size(theme.typography.normal);
            let button = Button::new(text).visuals(theme.button_visuals());

            if ui.add(button).clicked() {
               self.open_broadcast_options();
            }

            if self.self_broadcast {
               ui.add_space(10.0);

               let q_mark = RichText::new("Self-broadcast").size(theme.typography.normal);
               let badge = Badge::new(q_mark, BadgeTone::Warning);
               let tip_text = RichText::new(SELF_BROADCAST_TIP).size(theme.typography.normal);
               ui.add(badge).on_hover_text(tip_text);
            }

            // ? Maybe in the future we could replace this with swaps
            // ? Eg. going from USDC to ETH and not just limited to WETH > ETH
            if self.unwrap_to_eth && self.currency.is_native_wrapped() && !self.self_broadcast {
               ui.add_space(10.0);

               let text = RichText::new("Unwrap to ETH").size(theme.typography.normal);
               let badge = Badge::new(text, BadgeTone::Info);
               let tip_text = RichText::new(UNWRAP_TO_ETH_TIP).size(theme.typography.normal);
               ui.add(badge).on_hover_text(tip_text);
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
         // The card carries the memo and the switches the form used to hold, so its width is set here
         // rather than left to whatever the centred header measured last frame.
         .max_width(620.0)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
            ui.spacing_mut().button_padding = theme.button_padding;

            ui.set_width(560.0);

            // The switches come first: self-broadcast replaces the bundler below it entirely.
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

            if self.mode.is_unshield() {
               ui.add_space(theme.spacing.sm);

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
            }

            ui.add_space(theme.spacing.md);

            ui.add_enabled_ui(!self.self_broadcast, |ui| {
               let ui_size = vec2(ui.available_width(), 45.0);

               ui.allocate_ui(ui_size, |ui| {
                  ui.horizontal_centered(|ui| {
                     ui.label(RichText::new("Bundler URL").size(theme.typography.normal));

                     ui.add_space(6.0);

                     // What the default bundler is, who pays the fee and how to stop relying on it: a
                     // hover, because it is reference material rather than a decision.
                     let mark = RichText::new("?").size(theme.typography.normal);
                     let badge = Badge::new(mark, BadgeTone::Info);
                     let tip_text = RichText::new(PAYMASTER_TIP).size(theme.typography.normal);
                     ui.add(badge).on_hover_text(tip_text);

                     ui.add_space(8.0);

                     ui.add(
                        SecureTextEdit::singleline(&mut self.bundler_url)
                           .visuals(text_edit_visuals)
                           .hint_text(
                              RichText::new(format!(
                                 "{}/{{chainId}}/rpc",
                                 ZeusUrl::PimlicoBundler.base()
                              ))
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
                     crate::runtime::rt().spawn_blocking(move || {
                        persist_bundler_url(url);
                     });
                  }

                  ui.add_space(10.0);

                  let text = RichText::new("Save").size(theme.typography.small);
                  let button = Button::new(text).visuals(theme.button_visuals());

                  if ui.add(button).clicked() {
                     let url = BundlerUrl::new(self.bundler_url.clone());
                     crate::runtime::rt().spawn_blocking(move || {
                        persist_bundler_url(url);
                     });
                  }
               });

               ui.add_space(theme.spacing.sm);
            });

            if self.self_broadcast {
               ui.label(
                  RichText::new("Bundler options disabled while self-broadcast is enabled.")
                     .size(theme.typography.small)
                     .color(theme.colors.warning),
               );
            }

            ui.add_space(theme.spacing.md);

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

   /// The asset the form is about to move.
   fn privacy_asset(&self) -> RailgunAsset {
      match &self.nft {
         Some(nft) => RailgunAsset::Nft(nft.clone()),
         None => RailgunAsset::Fungible(self.currency.clone()),
      }
   }

   /// The asset identity the check belongs to.
   fn privacy_asset_key(&self) -> AssetId {
      self.privacy_asset().asset_id()
   }

   /// What is being moved, as typed: the amount for a token, the id and count for an NFT.
   fn privacy_amount_key(&self) -> String {
      match &self.nft {
         Some(nft) => format!(
            "{}#{}x{}",
            nft.collection,
            nft.token_id,
            self.nft_amount.trim()
         ),
         None => self.amount_field.amount.trim().to_string(),
      }
   }

   /// The value the check scores, in the asset's own units.
   ///
   /// An NFT moves one token and has no amount to be distinctive or to take a share of a pool, so the
   /// check answers only for the recipient; the number is not consulted.
   fn privacy_amount_wei(&self) -> U256 {
      match &self.nft {
         Some(_) => U256::ZERO,
         None => NumericValue::parse_to_wei(
            &self.amount_field.amount,
            self.currency.decimals(),
         )
         .wei(),
      }
   }

   /// Forget the verdict — the form no longer describes what it was computed for.
   fn reset_privacy(&mut self) {
      self.privacy = None;
      self.privacy_key = None;
      self.privacy_pending = None;
      self.privacy_loading = false;
   }

   /// The check's key for this form: what a verdict would be about.
   fn privacy_key(&self, chain: ChainId, owner: Address, recipient: Address) -> PrivacyKey {
      PrivacyKey {
         chain: chain.id(),
         asset: self.privacy_asset_key(),
         amount: self.privacy_amount_key(),
         recipient,
         owner,
      }
   }

   /// Run the privacy check once the form's inputs have settled.
   ///
   /// The check reads the events snapshot and the chain, so it waits out the debounce rather than
   /// running on every keystroke, and a newer key always supersedes the one on screen.
   fn poll_privacy(&mut self, chain: ChainId, owner: Address, recipient: &str, ui: &mut Ui) {
      let Ok(recipient_address) = Address::from_str(recipient.trim()) else {
         self.reset_privacy();
         return;
      };

      let key = self.privacy_key(chain, owner, recipient_address);

      // Answered, or in flight, for exactly this form: nothing to do.
      if self.privacy_key.as_ref() == Some(&key) {
         return;
      }

      // Nothing to check yet, so nothing to show either.
      if key.amount.is_empty() {
         self.reset_privacy();
         return;
      }

      match &self.privacy_pending {
         Some((pending, staged_at)) if pending == &key => {
            let waited = staged_at.elapsed();
            if waited < PRIVACY_CHECK_DELAY {
               // The debounce is a timer: without a repaint the check would wait for the next click.
               ui.ctx().request_repaint_after(PRIVACY_CHECK_DELAY - waited);
               return;
            }

            self.privacy_pending = None;
            self.privacy_key = Some(key.clone());
            self.privacy_loading = true;
            self.privacy = None;

            spawn_privacy_check(
               key,
               chain,
               self.privacy_asset(),
               self.privacy_amount_wei(),
               recipient_address,
               owner,
            );
         }
         _ => {
            // A different form: the old verdict is wrong rather than stale, so it goes now.
            self.privacy_pending = Some((key, Instant::now()));
            self.privacy = None;
            self.privacy_key = None;
            self.privacy_loading = true;
            ui.ctx().request_repaint_after(PRIVACY_CHECK_DELAY);
         }
      }
   }

   /// What this withdrawal reveals, and what to do about it.
   fn show_privacy_check(&mut self, theme: &Theme, ui: &mut Ui) {
      if !self.mode.is_unshield() {
         return;
      }

      let Some(privacy) = self.privacy.clone() else {
         if self.privacy_loading {
            // The check is debounced and runs off the frame path, so this line can sit there for a
            // moment: the spinner is what says it is working rather than stuck.
            ui.horizontal(|ui| {
               ui.add(Spinner::new().size(theme.typography.small).color(theme.colors.text_muted));

               ui.add(
                  Label::new(
                     RichText::new("Checking what this withdrawal reveals…")
                        .size(theme.typography.small)
                        .color(theme.colors.text_muted),
                     None,
                  )
                  .interactive(false),
               );
            });
         }
         return;
      };

      let decimals = self.currency.decimals();
      let symbol = self.currency.symbol().to_string();
      let inner_frame = theme.frame2;
      let mut apply_suggestion = None;

      inner_frame.show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);

         // One row carries the verdict and the way into the tips: this block sits directly above the
         // action button, so every row it spends is a row the button loses.
         ui.horizontal(|ui| {
            if let Some(advice) = &privacy.amount {
               let color = match advice.band {
                  RiskBand::Low => theme.colors.text_muted,
                  RiskBand::Medium => theme.colors.text,
                  RiskBand::High => theme.colors.warning,
                  RiskBand::Critical => theme.colors.error,
                  _ => theme.colors.text_muted,
               };

               ui.add(
                  Label::new(
                     RichText::new(format!(
                        "Amount privacy risk: {}",
                        advice.band.label()
                     ))
                     .size(theme.typography.large)
                     .color(color),
                     None,
                  )
                  .interactive(false),
               );
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
               let button =
                  Button::new(RichText::new("Privacy tips").size(theme.typography.normal))
                     .visuals(theme.button_visuals());

               if ui.add(button).clicked() {
                  self.privacy_tips_open = true;
               }
            });
         });

         if let Some(advice) = &privacy.amount {
            // One line: this block sits above the action button in a form with no scroll area.
            let reason = amount_reason(advice);
            ui.add(
               Label::new(
                  RichText::new(reason)
                     .size(theme.typography.small)
                     .color(theme.colors.text_muted),
                  None,
               )
               .wrap()
               .fill_width(true)
               .interactive(false),
            );

            if advice.user.duplicate_shields > 0 {
               ui.add(
                  Label::new(
                     RichText::new(
                        "You shielded this exact amount. Unshielding it again links the two: \
                         consider smaller amounts, spread over days.",
                     )
                     .size(theme.typography.small)
                     .color(theme.colors.warning),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            } else if advice.user.withdraws_remainder {
               ui.add(
                  Label::new(
                     RichText::new(
                        "This is what is left of a deposit you already partly withdrew: the two \
                         withdrawals add up to it.",
                     )
                     .size(theme.typography.small)
                     .color(theme.colors.warning),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            }

            if let Some(suggestion) = advice.suggestion {
               let suggested = NumericValue::format_wei(U256::from(suggestion), decimals).flatten();
               ui.horizontal(|ui| {
                  ui.add(
                     Label::new(
                        RichText::new(format!("Try {suggested} {symbol} instead"))
                           .size(theme.typography.normal),
                        None,
                     )
                     .interactive(false),
                  );

                  ui.add_space(8.0);

                  let button = Button::new(RichText::new("Use it").size(theme.typography.normal))
                     .visuals(theme.button_visuals());

                  if ui.add(button).clicked() {
                     apply_suggestion = Some(U256::from(suggestion));
                  }
               });
            }
         }

         if let Some(reason) = &privacy.unavailable {
            ui.add(
               Label::new(
                  RichText::new(format!(
                     "No privacy check for this withdrawal: {reason}"
                  ))
                  .size(theme.typography.small)
                  .color(theme.colors.text_muted),
                  None,
               )
               .wrap()
               .fill_width(true)
               .interactive(false),
            );
         }

         if let Some(pool) = &privacy.pool {
            // The sentences are shared with the shield side, so the two directions speak of a pool the
            // same way whether it is being joined or withdrawn from. Every reason that is true gets a
            // row: a pool thin in two ways is not explained by the louder of them.
            let warnings = shallow_pool_warnings(&symbol, pool);

            for text in &warnings {
               ui.add(
                  Label::new(
                     RichText::new(text.as_str())
                        .size(theme.typography.small)
                        .color(theme.colors.warning),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            }

            // The share of the pool this one withdrawal takes is only worth saying when the pool
            // itself is not the story: a thin pool has just spoken for itself.
            if let Some(share) = pool
               .share
               .filter(|share| *share > LARGE_POOL_SHARE)
               .filter(|_| warnings.is_empty())
            {
               ui.add(
                  Label::new(
                     RichText::new(format!(
                        "This withdrawal would take {:.0}% of the entire {symbol} pool.",
                        share * 100.0
                     ))
                     .size(theme.typography.small)
                     .color(theme.colors.warning),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            }
         }

         // Neither on-chain history nor being one of the user's own wallets is a link — unshielding to
         // a wallet nobody can connect to you is the point — so only a tie back to the person is
         // reported, and only the loudest one of those. An address with no tie at all says so in the
         // success colour: the form is otherwise silent about the one thing the recipient got right.
         //
         // The link is judged from the local history plus one `nonce` call, and neither of those can
         // fail its way into "no link": the history is what it is, and an unreadable `nonce` only
         // leaves `fresh` unknown. That is why this line is allowed to be reassuring.
         let (color, text) = match privacy.recipient.link() {
            Some(RecipientLink::Transacted) => (
               theme.colors.warning,
               "You have transacted with this address from one of your wallets, so it can be tied \
                back to you. A fresh address cannot."
                  .to_string(),
            ),
            Some(RecipientLink::PriorUnshields(1)) => (
               theme.colors.warning,
               "You have already unshielded to this address once.".to_string(),
            ),
            Some(RecipientLink::PriorUnshields(times)) => (
               theme.colors.warning,
               format!("You have already unshielded to this address {times} times."),
            ),
            None => (
               theme.colors.success,
               "Recipient doesn't link back to you.".to_string(),
            ),
         };

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

      if let Some(suggested) = apply_suggestion {
         self.amount_field.amount = NumericValue::format_wei(suggested, decimals).flatten();
      }
   }

   /// Run the pool check for the token the shield form would deposit, once per `(chain, asset)`.
   ///
   /// The key moves when the token or the chain is picked and at no other time — the check reads no
   /// amount and no recipient — so unlike the unshield check there is nothing to debounce, and the
   /// warning lands as soon as it is known. A key computed for another token does not make the old
   /// verdict stale but wrong, so it is dropped here, and the in-flight check publishes only for the
   /// key that asked for it.
   ///
   /// Armed only once the chain's Railgun state has synced: a check that runs before its data exists
   /// can only fail, and a failed verdict is latched as solidly as an answer — still silent, by design
   /// — so an early check would silence the warning for the rest of the visit.
   fn poll_shield_pool(&mut self, chain: ChainId, railgun_synced: bool) {
      let key = (chain.id(), self.privacy_asset_key());

      // A key change makes the old verdict wrong rather than stale, so it goes now — and the latch
      // goes with it: a latch left behind reads as "answered" while its verdict was dropped, which
      // would stop the check from ever re-arming for that key.
      if self.shield_pool_key.as_ref() != Some(&key) {
         self.shield_pool_key = None;
         self.shield_pool = None;
      }

      // Not synced yet, or answered / in flight for exactly this token: nothing to do. The key is
      // deliberately left unlatched while the chain is unsynced, so the check arms after sync.
      if !shield_check_due(
         self.shield_pool_key.as_ref(),
         &key,
         railgun_synced,
      ) {
         return;
      }

      self.shield_pool_key = Some(key.clone());
      self.shield_pool = None;

      spawn_shield_pool_check(key, chain, self.privacy_asset());
   }

   /// Every sentence the pool earns, above the shield form's button: a token whose Railgun pool
   /// carries too little activity — or too little value — to hide a later withdrawal in.
   ///
   /// Silent until there is something to say, and silent when the check could not run — unlike the
   /// unshield check there is no reassurance to protect here, since a deposit is unaffected either
   /// way, so a blank line never has to stand in for a verdict. A native shield deposits the chain's
   /// wrapped base token, and that is the pool the sentence names.
   fn show_shield_pool_check(&self, theme: &Theme, ui: &mut Ui) {
      let Some(pool) = &self.shield_pool else {
         return;
      };

      let token = self.currency.to_erc20();

      // Every reason the pool is thin gets a row: a pool that is thin by traffic *and* by value is
      // not explained by either sentence alone.
      for text in shallow_pool_warnings(&token.symbol, pool) {
         ui.add(
            Label::new(
               RichText::new(text).size(theme.typography.small).color(theme.colors.warning),
               None,
            )
            .wrap()
            .fill_width(true)
            .interactive(false),
         );
      }
   }

   /// The habits that outlast any single verdict, in a modal because the form has no room for them.
   fn privacy_tips(&mut self, theme: &Theme, chain_id: u64, ui: &mut Ui) {
      if !self.privacy_tips_open {
         return;
      }

      let ip_tip = self.ip_tip(chain_id);

      let title = RichText::new("Privacy tips")
         .size(theme.typography.large)
         .color(theme.colors.text);

      let id = Id::new("shield_ui_privacy_tips");
      let mut open = self.privacy_tips_open;

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         // A centred header asserts the width it measured last frame as a minimum, so the card needs
         // an explicit cap or it widens a little every frame.
         .max_width(560.0)
         .closable(true)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            ui.set_width(500.0);

            for tip in std::iter::once(ip_tip).chain(PRIVACY_TIPS.iter().copied()) {
               ui.add(
                  Label::new(
                     RichText::new(format!("• {tip}"))
                        .size(theme.typography.normal)
                        .color(theme.colors.text),
                     None,
                  )
                  .wrap()
                  .fill_width(true)
                  .interactive(false),
               );
            }
         });

      // X / Esc / backdrop must stick.
      self.privacy_tips_open = open;
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
      recipient_name_lapsed: bool,
      recipient_guard: Option<EnsRecipientGuard>,
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
      let erc1155_blocked = self.erc1155_shield_blocked();
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
         && !recipient_name_lapsed
         && !sending_tx
         && is_synced
         && !erc1155_blocked;

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

      // A refusal outranks everything above it: the asset itself cannot be shielded, so a transient
      // reason (or anything typed in the amount field) is beside the point. The notice above the button
      // carries the why.
      if erc1155_blocked {
         button_text = "ERC-1155 not supported".to_string();
      }

      if recipient_name_lapsed {
         button_text = "ENS name expired".to_string();
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
         self.send_transaction(ctx, recipient, recipient_guard);
      }
   }

   fn send_transaction(
      &mut self,
      ctx: &mut ZeusContext,
      recipient: String,
      recipient_guard: Option<EnsRecipientGuard>,
   ) {
      // Belt and braces: the button is disabled for this, but no path may put an ERC-1155 on chain as a
      // shield while Railgun's support for it is unverified.
      if self.erc1155_shield_blocked() {
         warn!("Refusing to shield an ERC-1155: Railgun's support for it is not verified");
         self.sending_tx = false;
         return;
      }

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
         crate::runtime::rt().spawn(async move {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
            });

            match shield(
               ctx.clone(),
               chain,
               asset,
               amount,
               from,
               recipient,
               recipient_guard,
            )
            .await
            {
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
         // Drive the non-Send future on the process runtime via `runtime::rt().block_on`
         // from a blocking thread (no Send bound, block_in_place still works).
         crate::runtime::rt().spawn_blocking(move || {
            let ctx = SHARED_GUI.write(|gui| {
               gui.loading_window.open("Wait while magic happens");
               gui.request_repaint();
               gui.ctx.clone()
            });

            let result = crate::runtime::rt().block_on(unshield(
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
               recipient_guard,
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

      crate::runtime::rt().spawn(async move {
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

/// The standing advice behind the check's **Privacy tips** modal: the habits that matter more than
/// any single score. The first tip — the IP address — is built by [`ShieldUi::ip_tip`] instead, since
/// who sees it depends on how the unshield is broadcast.
///
/// Each is the honest answer to something the check cannot measure — an IP address, a bundler's
/// logs, an exchange's records, the timing of two transactions — which is why they are shown
/// whether or not the amount looks distinctive.
const PRIVACY_TIPS: [&str; 3] = [
   "Amount and timing: unshield in smaller amounts, at varied times. Not the amount you just \
    shielded, and not the exact sum of a few deposits.",
   "Reads leak too: a public RPC sees which addresses you ask about. Use a private RPC or your own \
    node for the queries that matter.",
   "Recipient: a fresh 0x address that has never held funds. Never one used for earlier unshields \
    or other activity that can link back to you",
];

/// Run the check for `key` and publish the verdict if the form still matches it.
///
/// The context is taken inside the task: the frame holds the GUI lock while this is scheduled, and
/// the verdict is only ever written back for the key that asked for it.
fn spawn_privacy_check(
   key: PrivacyKey,
   chain: ChainId,
   asset: RailgunAsset,
   amount_wei: U256,
   recipient: Address,
   owner: Address,
) {
   crate::runtime::rt().spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let privacy = assess_unshield(ctx, chain, &asset, amount_wei, recipient, owner).await;

      SHARED_GUI.write(|gui| {
         // A newer key owns the form now: this verdict describes a withdrawal nobody is sending.
         if gui.shield_ui.privacy_key.as_ref() == Some(&key) {
            gui.shield_ui.privacy = Some(privacy);
            gui.shield_ui.privacy_loading = false;
         }
         gui.request_repaint();
      });
   });
}

/// Should the shield pool check be armed for `key`?
///
/// A latched key is already answered, or in flight, and a check is only armed once the chain's
/// Railgun state has synced: before its data exists it can only fail, and a failed verdict is latched
/// like any other — so it must not be armed at all until there is something for it to read.
fn shield_check_due(
   latched: Option<&(u64, AssetId)>,
   key: &(u64, AssetId),
   railgun_synced: bool,
) -> bool {
   railgun_synced && latched != Some(key)
}

/// Run the shield pool check for `key` and publish the verdict if the form still matches it.
///
/// Same contract as [`spawn_privacy_check`]: the context is taken inside the task, and a verdict
/// whose key has moved on is dropped rather than shown for another token.
fn spawn_shield_pool_check(key: (u64, AssetId), chain: ChainId, asset: RailgunAsset) {
   crate::runtime::rt().spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
      let pool = assess_shield(ctx, chain, &asset).await;

      SHARED_GUI.write(|gui| {
         if gui.shield_ui.shield_pool_key.as_ref() == Some(&key) {
            gui.shield_ui.shield_pool = pool;
         }
         gui.request_repaint();
      });
   });
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

      crate::runtime::rt().spawn(async move {
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
   recipient_guard: Option<EnsRecipientGuard>,
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

   // ERC-20 still needs an on-chain approval of RailgunSmartWallet before shield. An NFT does too, but
   // only for the token being moved: `RailgunLogic.transferTokenIn` calls `transferFrom` for an ERC-721,
   // which accepts a per-token `approve`, so nothing grants the wallet a right over the rest of the
   // collection. ERC-1155 has no per-token approval to give — and the protocol reverts on it outright
   // ("RailgunLogic: ERC1155 not yet supported"), which is why shielding one is refused in the UI.
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
         RailgunAsset::Nft(nft) => match nft.standard {
            NftStandard::Erc721 => {
               ensure_erc721_approve(
                  ctx.clone(),
                  chain,
                  from,
                  nft.collection,
                  nft.token_id,
                  railgun_address,
                  "Railgun",
                  "Approval required to shield this token",
               )
               .await?;
            }
            NftStandard::Erc1155 => {
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
         },
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
      block_number,
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
   tx_analysis.set_block_number(block_number);
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
         ens_recipient: recipient_guard,
         ..Default::default()
      },
   )
   .await?;

   crate::runtime::rt().spawn(settle_railgun_op(
      ctx,
      chain,
      from,
      // Which half waits on the chain is the operation's business: only an NFT shield moves public
      // ownership, and only an ERC-20 one has a token balance to re-read. A native-ETH shield wraps
      // inside the Railgun contract, so the wallet's public wrapped-native balance never moves.
      asset.settled_op(false),
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

/// The line under the amount's verdict: what the check counted, in deposits, with the direction the
/// band went.
///
/// Every number here counts deposits that could explain the amount — ones close to its size, and
/// ones that add up to it — and they all run the same way: the more there are, the more cover the
/// amount has. A bare count does not say which way it cuts, so a crowd thin enough to be what the
/// band is resting on is named as thin, and the sums get a clause only when they are what carries
/// it. The ±1% count the scorer keeps is left out: a deposit that is *exactly* this size is the
/// sharper fact, and the one the user can act on.
fn amount_reason(advice: &UnshieldAmountAdvice) -> String {
   let capitalize = |clause: &str| {
      let mut clause = clause.to_string();
      if let Some(head) = clause.get_mut(0..1) {
         head.make_ascii_uppercase();
      }
      clause
   };

   let exact = advice.matches_by_size[0];
   let pairs = advice.matches_by_size[1];
   let near = advice.crowding.within_ten_percent;
   let thin = matches!(
      RiskBand::from_score(advice.crowd_score),
      RiskBand::High | RiskBand::Critical
   );
   let sums_lead = advice.match_score > advice.crowd_score;

   let crowd = match near {
      0 => format!(
         "no deposit in the last 180 days is within 10% {}",
         if sums_lead { "of it" } else { "of this size" }
      ),
      near => {
         let clause = format!(
            "{}{near} deposits in the last 180 days are within 10% {}",
            if thin { "only " } else { "" },
            if sums_lead { "of it" } else { "of this size" },
         );
         if exact > 0 && !sums_lead {
            // The exact count sits inside the 10% one, so it reads as part of it, not as a second
            // population. When the sums lead, that same count is the first clause already.
            format!("{clause}, {exact} of them exactly this size")
         } else {
            clause
         }
      }
   };

   if !sums_lead {
      return format!("{}.", capitalize(&crowd));
   }

   // The smallest set that matches is the sharpest explanation of the amount, so it is the one
   // stated: a deposit of this size, else a pair, else a triple.
   let sums = if exact > 0 {
      format!(
         "{exact} deposit{} in the last 180 days {} exactly this size",
         if exact == 1 { "" } else { "s" },
         if exact == 1 { "is" } else { "are" },
      )
   } else if pairs > 0 {
      "2 deposits in the last 180 days add up to this size".to_string()
   } else {
      "3 deposits in the last 180 days add up to this size".to_string()
   };

   format!("{}, and {crowd}.", capitalize(&sums))
}

#[cfg(test)]
mod tests {
   use super::*;

   use zeus_railgun::privacy::{Crowding, MatchSets, UserExposure};

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

   /// Shielding an ERC-1155 is refused until Railgun's support for it has been verified, and only in
   /// this form — the unshield path refuses it in its own entry point (`unshield`), because a note
   /// that is already private is a different path.
   #[test]
   fn an_erc1155_cannot_be_shielded() {
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Shield);

      ui.nft = Some(nft(NftStandard::Erc1155));
      assert!(ui.erc1155_shield_blocked());

      // An ERC-721 and the fungible form are untouched.
      ui.nft = Some(nft(NftStandard::Erc721));
      assert!(!ui.erc1155_shield_blocked());
      ui.nft = None;
      assert!(!ui.erc1155_shield_blocked());

      // Same token, other mode: this guard says nothing about it.
      ui.nft = Some(nft(NftStandard::Erc1155));
      ui.set_mode(RailgunMode::Unshield);
      assert!(!ui.erc1155_shield_blocked());
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

   /// A chain switch forgets the NFT, because the send path pairs the selection with the chain active at
   /// send time rather than the one it was picked on.
   ///
   /// The mode survives — it says which direction the user is going, not which asset moves.
   #[test]
   fn a_chain_switch_forgets_the_nft() {
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Unshield);
      ui.nft = Some(nft(NftStandard::Erc1155));
      ui.nft_amount = "3".to_string();

      ui.default_currency(8453);

      assert!(
         ui.nft.is_none(),
         "the previous chain's NFT cannot be sent from this one"
      );
      assert!(
         ui.nft_amount.is_empty(),
         "nor the quantity typed for it"
      );
      assert!(
         ui.mode.is_unshield(),
         "only the asset is forgotten, not the mode"
      );
   }

   /// A privacy verdict belongs to the form that asked for it: editing the amount, the token or the
   /// recipient makes it the answer to another question, and it must not be shown as if it were this
   /// one's.
   #[test]
   fn a_privacy_key_follows_the_form() {
      let weth = Currency::from(ERC20Token::weth());
      let owner = Address::from([1u8; 20]);
      let recipient = Address::from([2u8; 20]);
      let other_recipient = Address::from([3u8; 20]);
      let chain = ChainId::new(1).unwrap();

      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Unshield);
      ui.currency = weth.clone();
      ui.amount_field.amount = "1.5".to_string();

      let key = ui.privacy_key(chain, owner, recipient);
      assert_eq!(key.asset, AssetId::Erc20(weth.to_erc20().address));
      assert_eq!(key.chain, 1);
      assert_eq!(key, ui.privacy_key(chain, owner, recipient));

      // The same form on another chain is another pool.
      assert_ne!(
         key,
         ui.privacy_key(ChainId::new(11155111).unwrap(), owner, recipient)
      );

      ui.amount_field.amount = "1.6".to_string();
      assert_ne!(
         key,
         ui.privacy_key(chain, owner, recipient),
         "an edited amount is another withdrawal"
      );

      ui.amount_field.amount = "1.5".to_string();
      assert_ne!(
         key,
         ui.privacy_key(chain, owner, other_recipient),
         "another recipient is another withdrawal"
      );
      assert_ne!(
         key,
         ui.privacy_key(chain, Address::from([4u8; 20]), recipient),
         "the wallet's own history is part of the question"
      );

      ui.currency = Currency::from(ERC20Token::usdc());
      assert_ne!(
         key,
         ui.privacy_key(chain, owner, recipient),
         "another asset is another pool"
      );

      // Nothing typed yet: the form is not something a check can answer for.
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Unshield);
      ui.currency = weth;
      assert!(ui.privacy_key(chain, owner, recipient).amount.is_empty());
   }

   /// A native shield goes in as the chain's wrapped base token, so ETH and WETH are one pool — and
   /// one key for the shield pool check, which is why switching between them re-reads nothing.
   #[test]
   fn a_native_shield_shares_the_wrapped_tokens_pool() {
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Shield);

      ui.currency = Currency::native(1);
      let native = ui.privacy_asset_key();

      ui.currency = Currency::from(ERC20Token::weth());
      assert_eq!(native, ui.privacy_asset_key());
      assert_eq!(native, AssetId::Erc20(ERC20Token::weth().address));
   }

   /// A check that runs before its data exists can only fail, and a failed verdict latches like an
   /// answer — so an unsynced chain arms nothing: neither a fresh key nor one already latched, whose
   /// latch could then never be explained by a check that ran while the data was missing.
   #[test]
   fn an_unsynced_chain_does_not_arm_the_shield_pool_check() {
      let key = (1u64, AssetId::Erc20(ERC20Token::weth().address));

      assert!(
         !shield_check_due(None, &key, false),
         "an unsynced chain must not arm the check"
      );
      assert!(
         !shield_check_due(Some(&key), &key, false),
         "nor re-arm a key that is already latched"
      );
   }

   /// Once the chain has synced the check arms for a free key, and only once: a latched key is
   /// answered (or in flight), so asking again would discard the verdict the form is showing.
   #[test]
   fn a_synced_chain_arms_the_shield_pool_check_once() {
      let key = (1u64, AssetId::Erc20(ERC20Token::weth().address));

      assert!(
         shield_check_due(None, &key, true),
         "a synced chain arms the check for a free key"
      );
      assert!(
         !shield_check_due(Some(&key), &key, true),
         "but does not arm it again once it is latched"
      );
   }

   /// A key change drops the old verdict immediately, synced or not: it describes another pool, so it
   /// is wrong rather than stale, and the latch goes with it — the key is not left latched without a
   /// verdict, which would read as answered and stall the check.
   #[test]
   fn a_key_change_drops_the_old_verdict_even_while_unsynced() {
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Shield);

      // A verdict for one asset, already latched under its key.
      ui.currency = Currency::from(ERC20Token::weth());
      let latched = (1u64, ui.privacy_asset_key());
      ui.shield_pool_key = Some(latched.clone());
      ui.shield_pool = Some(PoolDepth {
         balance_wei: U256::from(1_000u64),
         usd: Some(400.0),
         share: None,
         deposits: Some(1_224),
      });

      // The form moves to another asset while the chain is unsynced: the verdict and its latch both go
      // now, so the check can arm for whichever key is picked once sync completes.
      ui.currency = Currency::from(ERC20Token::usdc());
      ui.poll_shield_pool(ChainId::new(1).unwrap(), false);

      assert!(
         ui.shield_pool.is_none(),
         "another key's verdict must not linger"
      );
      assert_eq!(
         ui.shield_pool_key, None,
         "the latch is dropped with the verdict, so the check can re-arm after sync"
      );
   }

   /// A key round trip inside an unsynced window must not stall the check: the latch is dropped
   /// with the verdict on the way out, so the check is due again once sync completes.
   #[test]
   fn a_key_round_trip_while_unsynced_still_arms_the_check_after_sync() {
      let mut ui = ShieldUi::new();
      ui.set_mode(RailgunMode::Shield);

      // An answered check for WETH on chain 1: latched, verdict shown.
      ui.currency = Currency::from(ERC20Token::weth());
      let key_a = (1u64, ui.privacy_asset_key());
      ui.shield_pool_key = Some(key_a.clone());
      ui.shield_pool = Some(PoolDepth {
         balance_wei: U256::from(1_000u64),
         usd: Some(400.0),
         share: None,
         deposits: Some(1_224),
      });

      // The chain drops out of sync; the user compares another asset and comes back.
      ui.currency = Currency::from(ERC20Token::usdc());
      ui.poll_shield_pool(ChainId::new(1).unwrap(), false);
      ui.currency = Currency::from(ERC20Token::weth());
      ui.poll_shield_pool(ChainId::new(1).unwrap(), false);

      // The latch must not survive the move: left behind it would read as "answered" while its
      // verdict was dropped, and the check could never re-arm for it.
      assert_eq!(
         ui.shield_pool_key, None,
         "a key change drops the latch so the check can re-arm after sync"
      );
      assert!(
         shield_check_due(ui.shield_pool_key.as_ref(), &key_a, true),
         "once synced, the check is due again for the returned key"
      );
      assert!(
         ui.shield_pool.is_none(),
         "and no stale verdict is shown"
      );
   }

   /// The shield warning is at most two wrapped sentences in the slot the ERC-1155 notice already
   /// uses — the pool can be thin by traffic and by value at once, and both are said — and the shield
   /// form carries fewer fixed rows than the unshield one (no broadcast row, no verdict block, no tips
   /// button), so it needs no budget of its own. Measured all the same, because a wrapped line is
   /// exactly the kind of row that grows without anyone noticing.
   #[test]
   fn the_shield_pool_warnings_stay_on_two_rows() {
      use egui_elements::theme::ThemeKind;

      let theme = Theme::new(ThemeKind::TokyoNight);
      let mut ui_state = ShieldUi::new();
      ui_state.set_mode(RailgunMode::Shield);
      ui_state.currency = Currency::from(ERC20Token::weth());
      // Both sentences the shield form can draw at once, at the width the card actually gives them.
      ui_state.shield_pool = Some(PoolDepth {
         balance_wei: U256::from(1_000u64),
         usd: Some(400.0),
         share: None,
         deposits: Some(1_224),
      });

      let ctx = eframe::egui::Context::default();
      ctx.set_fonts(eframe::egui::FontDefinitions::default());
      ctx.all_styles_mut(|style| *style = theme.style());

      let input = eframe::egui::RawInput {
         screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            vec2(1280.0, 900.0),
         )),
         ..Default::default()
      };

      let mut height = 0.0;
      let mut output = ctx.run_ui(input, |ui| {
         // The card's inner width in the running app (card ~520 px wide, minus its padding).
         ui.set_max_width(490.0);
         let block = ui.vertical(|ui| ui_state.show_shield_pool_check(&theme, ui));
         height = block.response.rect.height();
      });

      // The probe paints into no window, so the texture uploads have nowhere to go.
      output.textures_delta.clear();

      // Measured: 35 px — both sentences the pool can draw at once. The bound is headroom above
      // that, not the number itself.
      assert!(
         height <= 48.0,
         "the shield pool warnings are {height:.1} px, past the two rows the shield form can spare"
      );
   }

   /// What "Use it" writes into the amount field is the suggestion as a plain decimal, which is what
   /// the field — and `parse_to_wei` — reads back.
   #[test]
   fn a_suggestion_is_written_as_a_plain_amount() {
      let suggestion = 1_020_000_000_000_000_000u128;

      let text = NumericValue::format_wei(U256::from(suggestion), 18).flatten();

      assert_eq!(text, "1.02");
      assert_eq!(
         NumericValue::parse_to_wei(&text, 18).wei(),
         U256::from(suggestion),
         "and it round-trips back to the amount that was suggested"
      );
   }

   /// The tips modal is an overlay, so the form's budget says nothing about it: measure it, and check
   /// that a centred header does not widen the card frame after frame.
   #[test]
   fn the_privacy_tips_modal_fits_and_holds_its_width() {
      use egui_elements::theme::ThemeKind;

      let theme = Theme::new(ThemeKind::TokyoNight);
      let mut ui_state = ShieldUi::new();
      ui_state.set_mode(RailgunMode::Unshield);
      ui_state.currency = Currency::from(ERC20Token::weth());
      ui_state.privacy_tips_open = true;

      let ctx = eframe::egui::Context::default();
      ctx.set_fonts(eframe::egui::FontDefinitions::default());
      ctx.all_styles_mut(|style| *style = theme.style());

      let input = eframe::egui::RawInput {
         screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            vec2(1280.0, 900.0),
         )),
         ..Default::default()
      };

      // The card's Area, as the modal registers it: `elegance_modal_window` salted with the id.
      let area = eframe::egui::Id::new("elegance_modal_window").with(eframe::egui::Id::new(
         eframe::egui::Id::new("shield_ui_privacy_tips"),
      ));

      let mut rects = Vec::new();
      for _ in 0..4 {
         let mut output = ctx.run_ui(input.clone(), |ui| {
            ui.set_max_width(490.0);
            ui_state.privacy_tips(&theme, 1, ui);
         });
         output.textures_delta.clear();
         rects.push(ctx.memory(|memory| memory.area_rect(area)));
      }

      let last = rects.last().copied().flatten().expect("the modal's area is registered");
      assert!(
         last.height() <= 880.0,
         "the tips modal is taller than the window: {last:?}"
      );
      assert!(
         last.width() <= 561.0,
         "the tips modal is wider than its cap: {last:?}"
      );

      // Flat after the first pass, or the header band is ratcheting the card wider every frame.
      let warm = rects[1].expect("measured by the second frame");
      assert!(
         (last.width() - warm.width()).abs() < 1.0,
         "the modal's width drifts across frames: {rects:?}"
      );
   }

   /// The broadcast options modal holds what the form gave up — the memo and the two switches — and it
   /// is an overlay, so the form's budget says nothing about it. Measured the same way all the same,
   /// because a centred header is the thing that ratchets.
   #[test]
   fn the_broadcast_options_modal_fits_and_holds_its_width() {
      use egui_elements::theme::ThemeKind;

      let theme = Theme::new(ThemeKind::TokyoNight);
      let mut ui_state = ShieldUi::new();
      ui_state.set_mode(RailgunMode::Unshield);
      ui_state.currency = Currency::from(ERC20Token::weth());
      ui_state.open_broadcast_options = true;
      ui_state.memo = "a note that has to fit".to_string();

      let ctx = eframe::egui::Context::default();
      ctx.set_fonts(eframe::egui::FontDefinitions::default());
      ctx.all_styles_mut(|style| *style = theme.style());

      let input = eframe::egui::RawInput {
         screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            vec2(1280.0, 900.0),
         )),
         ..Default::default()
      };

      // `Modal::new` salts with an id it hashes once more, so the area is a double wrap.
      let area = eframe::egui::Id::new("elegance_modal_window").with(eframe::egui::Id::new(
         eframe::egui::Id::new("shield_ui_advanced_broadcast_options"),
      ));

      let mut rects = Vec::new();
      for _ in 0..4 {
         let mut output = ctx.run_ui(input.clone(), |ui| {
            ui_state.broadcast_options(&theme, 1, ui);
         });
         output.textures_delta.clear();
         rects.push(ctx.memory(|memory| memory.area_rect(area)));
      }

      let last = rects.last().copied().flatten().expect("the modal's area is registered");
      assert!(
         last.height() <= 880.0,
         "the broadcast modal is taller than the window: {last:?}"
      );
      assert!(
         last.width() <= 621.0,
         "the broadcast modal is wider than its cap: {last:?}"
      );

      let warm = rects[1].expect("measured by the second frame");
      assert!(
         (last.width() - warm.width()).abs() < 1.0,
         "the modal's width drifts across frames: {rects:?}"
      );
   }

   /// Render the check block headlessly and report the height it took.
   fn check_block_height(theme: &Theme, privacy: UnshieldPrivacy) -> f32 {
      let mut ui_state = ShieldUi::new();
      ui_state.set_mode(RailgunMode::Unshield);
      ui_state.currency = Currency::from(ERC20Token::weth());
      ui_state.privacy = Some(privacy);

      let ctx = eframe::egui::Context::default();
      ctx.set_fonts(eframe::egui::FontDefinitions::default());
      ctx.all_styles_mut(|style| *style = theme.style());

      let mut height = 0.0;
      let input = eframe::egui::RawInput {
         screen_rect: Some(eframe::egui::Rect::from_min_size(
            eframe::egui::Pos2::ZERO,
            vec2(1280.0, 900.0),
         )),
         ..Default::default()
      };

      let mut output = ctx.run_ui(input, |ui| {
         // The card's inner width in the running app (card ~520 px wide, minus its padding).
         ui.set_max_width(490.0);
         let block = ui.vertical(|ui| ui_state.show_privacy_check(theme, ui));
         height = block.response.rect.height();
      });

      // The probe paints into no window, so the texture uploads have nowhere to go.
      output.textures_delta.clear();

      height
   }

   /// The sentence under the verdict, for each way the check can come out.
   ///
   /// Every number in it counts deposits, and they all cut the same way — more deposits that could
   /// explain the amount means more cover — so "only" appears exactly when the crowd is thin enough
   /// to be what the band is resting on. The sums get a clause only when they carry the band, which
   /// is what keeps the line to one clause in the ordinary case.
   #[test]
   fn the_amount_reason_reads_the_verdict_off_the_numbers() {
      fn advice(
         matches_by_size: [u32; 3],
         crowding: Crowding,
         crowd_score: u8,
         match_score: u8,
      ) -> UnshieldAmountAdvice {
         let score = crowd_score.max(match_score);
         UnshieldAmountAdvice {
            score,
            amount_score: score,
            crowd_score,
            match_score,
            band: RiskBand::from_score(score),
            pool_size: 1_212,
            matches_by_size,
            sets: MatchSets::default(),
            crowding,
            suggestion: None,
            user: UserExposure::default(),
         }
      }

      // The Sepolia run: a thin crowd, one deposit exactly the size, the crowd carrying the band.
      assert_eq!(
         amount_reason(&advice(
            [1, 344, 51_676],
            Crowding {
               within_one_percent: 1,
               within_ten_percent: 5,
            },
            87,
            69,
         )),
         "Only 5 deposits in the last 180 days are within 10% of this size, 1 of them exactly this size."
      );

      // A crowd worth hiding in: no "only", and the sums stay out of the line entirely.
      assert_eq!(
         amount_reason(&advice(
            [91, 0, 0],
            Crowding {
               within_one_percent: 91,
               within_ten_percent: 95,
            },
            3,
            1,
         )),
         "95 deposits in the last 180 days are within 10% of this size, 91 of them exactly this size."
      );

      // The sums carry the band, so they lead — and the exact count is not said twice.
      assert_eq!(
         amount_reason(&advice(
            [1, 0, 0],
            Crowding {
               within_one_percent: 1,
               within_ten_percent: 5,
            },
            20,
            40,
         )),
         "1 deposit in the last 180 days is exactly this size, and 5 deposits in the last 180 days are within 10% of it."
      );

      // A pair is the sharpest explanation there is.
      assert_eq!(
         amount_reason(&advice(
            [0, 3, 0],
            Crowding {
               within_one_percent: 0,
               within_ten_percent: 4,
            },
            30,
            55,
         )),
         "2 deposits in the last 180 days add up to this size, and only 4 deposits in the last 180 days are within 10% of it."
      );

      // A triple, with nothing near the size at all.
      assert_eq!(
         amount_reason(&advice([0, 0, 2], Crowding::default(), 60, 80)),
         "3 deposits in the last 180 days add up to this size, and no deposit in the last 180 days is within 10% of it."
      );

      // Nothing near it and nothing adding up to it: one clause, at the thinnest crowd there is.
      assert_eq!(
         amount_reason(&advice([0, 0, 0], Crowding::default(), 100, 0)),
         "No deposit in the last 180 days is within 10% of this size."
      );
   }

   /// The check block sits directly above the action button in a form with no scroll area, so every
   /// row it draws is a row the button loses.
   ///
   /// Both ends are measured: the state the form usually shows — a low-risk amount with nothing
   /// matching it, and an address whose only sin is history, which is now silent — and the loudest
   /// block it can ever draw, where every warning fires at once.
   #[test]
   fn the_check_block_fits_its_height_budget() {
      use egui_elements::theme::ThemeKind;
      use zeus_railgun::privacy::{
         Crowding, MatchSets, RiskBand, UnshieldAmountAdvice, UserExposure,
      };

      use super::super::privacy::{PoolDepth, RecipientAdvice};
      use super::UnshieldPrivacy;

      let theme = Theme::new(ThemeKind::TokyoNight);

      let advice =
         |band, score, matches_by_size, crowding, suggestion, user| UnshieldAmountAdvice {
            score,
            amount_score: score,
            crowd_score: score,
            match_score: score,
            band,
            pool_size: 9_184,
            matches_by_size,
            sets: MatchSets::default(),
            crowding,
            suggestion,
            user,
         };

      let quiet = UnshieldPrivacy {
         amount: Some(advice(
            RiskBand::Low,
            5,
            [0, 0, 0],
            Crowding::default(),
            None,
            UserExposure {
               duplicate_shields: 0,
               withdraws_remainder: false,
            },
         )),
         unavailable: None,
         pool: None,
         recipient: RecipientAdvice {
            fresh: Some(false),
            ..RecipientAdvice::default()
         },
      };

      let loudest = UnshieldPrivacy {
         amount: Some(advice(
            RiskBand::Critical,
            97,
            [4, 548, 37_692],
            Crowding {
               within_one_percent: 36,
               within_ten_percent: 41,
            },
            Some(90_000_000_000_000_000),
            UserExposure {
               duplicate_shields: 1,
               withdraws_remainder: true,
            },
         )),
         unavailable: None,
         pool: Some(PoolDepth {
            balance_wei: U256::from(1_000u64),
            usd: Some(500.0),
            share: Some(0.4),
            deposits: Some(900),
         }),
         recipient: RecipientAdvice {
            fresh: Some(false),
            transacted_with: true,
            prior_unshields: 3,
         },
      };

      // Measured: 89 px quiet, 221 px loudest. The block is the only part of the form whose height
      // follows the data, so it is the part that gets a budget — the card is centred in the panel, and
      // a block taller than the fixed rows leave room for pushes the action button off the bottom of
      // the window. The bounds stay where they were when the block measured 97 and 229 and the form
      // still carried the memo and the switches: they catch rows being re-added, not metric drift.
      // The quiet state is the one that reassures about the recipient, so it is a row taller than it
      // used to be. The loudest state gained the second pool sentence — a pool can be thin by traffic
      // and by value at once, and both are said — which is 24 px of its 260, and it is still inside.
      for (name, privacy, budget) in [("quiet", quiet, 130.0), ("loudest", loudest, 260.0)] {
         let height = check_block_height(&theme, privacy);
         assert!(
            height <= budget,
            "the {name} check block is {height:.1} px, over its {budget:.0} px budget: the Unshield \
             form has no room for another row"
         );
      }
   }
}
