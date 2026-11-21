//! UI for viewing and revoking ERC20 / Permit2 / NFT token approvals.

use crate::assets::icons::Icons;
use crate::core::{
   DecodedEvent, NftApproveParams, PermitParams, SendTxOptions, SendTxRequest, TokenApproveParams,
   TransactionAnalysis, WalletInfo, ZeusContext, ZeusCtx, send_transaction, signature,
};
use crate::gui::ui::common::{
   SpokenAddress, chain_label, drawn_and_spoken, name_address, name_text, usable_chain,
};
use crate::gui::{SHARED_GUI, ui::show_with_fade};
use crate::utils::{TimeStamp, simulate::simulate_for_analysis};
use anyhow::anyhow;
use egui::{Align, Layout, Margin, Response, RichText, Sense, TextWrapMode, Ui, vec2};
use egui_elements::{
   Button, Chip, ChipRole, ComboBox, Frame, Label, Motion, Spinner, Tabs, Theme, animate_with,
   layout::{Column, Table, TableScroll},
};
use std::collections::HashMap;
use std::sync::Arc;
use zeus_eth::{
   abi::{
      erc721, erc1155,
      permit::{allowance, encode_permit_single_call},
   },
   alloy_primitives::{Address, Bytes, U256},
   currency::{Currency, ERC20Token},
   nft::NftStandard,
   types::ChainId,
   utils::{NumericValue, address_book, batch},
};

const ZEUS_TIP: &str = "Zeus only shows approvals that have been been made in-app.\n
It cannot track approvals made from other wallets.";

const DEFAULT_ROWS_PER_PAGE: usize = 10;

/// Max `(token, spender)` allowance pairs per Multicall3 aggregate so the eth_call stays under gas limits.
const ALLOWANCE_PAIR_BATCH: usize = 20;

/// What the approvals table is listing.
///
/// An enum rather than a `bool` for the same reason the token picker's `PickerMode` is one: a mode is
/// exactly one of these, so "neither" and "both" cannot be represented. `Fungible` is the default and
/// what `open` restores, which keeps the ERC-20 view the one the page lands on.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Default)]
enum ApprovalMode {
   #[default]
   Fungible,
   Nft,
}

#[derive(Debug, Clone)]
enum ApprovalKind {
   Erc20(TokenApproveParams),
   Permit2(PermitParams),
   Nft(NftApproveParams),
}

impl ApprovalKind {
   pub fn is_permit2(&self) -> bool {
      matches!(self, Self::Permit2(_))
   }

   pub fn expiration(&self) -> TimeStamp {
      match self {
         Self::Permit2(params) => params.expiration,
         _ => TimeStamp::default(),
      }
   }

   /// What the Type column calls this approval.
   fn type_label(&self) -> &'static str {
      match self {
         Self::Erc20(_) => "ERC-20",
         Self::Permit2(_) => "Permit2",
         Self::Nft(params) => match params.standard {
            Some(NftStandard::Erc721) => "ERC-721",
            Some(NftStandard::Erc1155) => "ERC-1155",
            // `ApprovalForAll` carries no standard in the log and the probe could not be made: the Type
            // column says what is known rather than naming one the event does not contain.
            None => "NFT",
         },
      }
   }
}

/// What an approval is *over*, which is also what decides how the row is drawn and sorted.
///
/// A fungible approval has a token and an amount. An NFT approval has a **collection**, and covers
/// either the whole collection (`ApprovalForAll` and nothing else) or one id — so it carries an
/// `Option<U256>` where the fungible one carries a number. They are one enum rather than two row
/// types because everything else about a row — who granted it, to whom, on which chain, revoke —
/// is identical, and the columns line up because of it.
#[derive(Debug, Clone)]
enum ApprovalAsset {
   Token {
      currency: Currency,
      amount: NumericValue,
   },
   Nft {
      collection: Address,
      /// `None` covers the whole collection; `Some` is one id.
      token_id: Option<U256>,
      /// ERC-5216 grants an allowance per id. The other two shapes carry no amount at all.
      amount: Option<U256>,
   },
}

impl ApprovalAsset {
   /// What rows sort by.
   ///
   /// A token's symbol is already at hand; a collection's *name* is only known once a context is
   /// there to resolve it, and sorting half by name and half by address would make the order depend
   /// on cache warmth — so a collection sorts by its address, which is stable either way.
   fn sort_key(&self) -> String {
      match self {
         Self::Token { currency, .. } => currency.symbol().to_owned(),
         Self::Nft { collection, .. } => collection.to_string(),
      }
   }
}

#[derive(Debug, Clone)]
struct ApprovalRow {
   chain: u64,
   owner: Address,
   asset: ApprovalAsset,
   /// The address being trusted: an ERC-20 spender, a Permit2 spender, or an NFT operator.
   spender: Address,
   kind: ApprovalKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
struct CacheKey {
   wallet: Option<Address>,
   chain: Option<u64>,
   /// Part of the key because the two modes show different rows: without it, switching mode would
   /// find the key unchanged and keep serving the other mode's cache.
   mode: ApprovalMode,
}

impl CacheKey {
   fn invalid() -> Self {
      Self {
         wallet: None,
         chain: Some(u64::MAX),
         mode: ApprovalMode::default(),
      }
   }
}

pub struct ApprovalsUi {
   open: bool,
   loading: bool,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   mode: ApprovalMode,
   cached_rows: Vec<ApprovalRow>,
   cache_key: CacheKey,
   /// Bumped whenever the content below the filters is replaced, which restarts its fade-in.
   fade_gen: u64,
   current_page: usize,
   rows_per_page: usize,
   /// The words the page hands back to the frame, which owns the announcement channel.
   ///
   /// The page draws from a borrowed theme and cannot announce. A revoke finishes on the runtime
   /// after the frame that started it, and the frame keeps only the last announcement of a frame,
   /// so the sentence waits here until a later frame drains it.
   notice: Option<String>,
}

impl ApprovalsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         selected_wallet: None,
         selected_chain: None,
         mode: ApprovalMode::default(),
         cached_rows: Vec::new(),
         cache_key: CacheKey::default(),
         fade_gen: 0,
         current_page: 0,
         rows_per_page: DEFAULT_ROWS_PER_PAGE,
         notice: None,
      }
   }

   /// A page already open on `rows`, with the cache key that keeps `show` from starting a fetch.
   ///
   /// The approval manager a real page reads does not exist in a unit test, so the rows are seeded
   /// directly and every other part of the page — filters, table, notice channel — is the real one.
   #[cfg(test)]
   fn for_test(rows: Vec<ApprovalRow>) -> Self {
      let mut page = Self::new();
      page.open = true;
      page.cached_rows = rows;
      let key = page.current_cache_key();
      page.cache_key = key;
      page
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if self.open {
         return;
      }

      self.open = true;
      self.mode = ApprovalMode::default();
      self.cached_rows.clear();
      self.cache_key = CacheKey::invalid();
      self.current_page = 0;
   }

   pub fn close(&mut self) {
      if !self.open && self.cached_rows.is_empty() {
         return;
      }

      self.open = false;
      self.selected_wallet = None;
      self.selected_chain = None;
      self.cached_rows = Vec::new();
      self.cache_key = CacheKey::default();
      self.current_page = 0;
   }

   /// The words this page should say, drained by the frame right after it is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Switch which kind of approval the table lists.
   ///
   /// Only flips the mode and drops what is shown: the cache key carries the mode, so the next frame
   /// rebuilds — and a rebuild is what reads the manager for the other kind. Clearing here (rather
   /// than leaving the old rows up) is what shows the spinner instead of the previous mode's rows for
   /// the frames the rebuild takes.
   fn set_mode(&mut self, mode: ApprovalMode) {
      if self.mode == mode {
         return;
      }

      self.mode = mode;
      self.cached_rows.clear();
      self.loading = true;
      self.fade_gen += 1;
      self.current_page = 0;
   }

   fn current_cache_key(&self) -> CacheKey {
      CacheKey {
         wallet: self.selected_wallet.as_ref().map(|w| w.address),
         chain: self.selected_chain.map(|c| c.id()),
         mode: self.mode,
      }
   }

   fn rebuild_cache(&mut self) {
      if !self.open {
         self.cached_rows.clear();
         return;
      }

      let key = self.current_cache_key();
      if key == self.cache_key {
         return;
      }

      // Claim the key before spawning so we don't queue a rebuild every frame.
      self.cache_key = key.clone();
      self.loading = true;

      let selected_wallet = self.selected_wallet.clone();
      let selected_chain = self.selected_chain;
      let mode = self.mode;

      crate::runtime::rt().spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let manager = ctx.approval_manager();

         let mut rows = Vec::new();

         // Each mode builds only its own rows. For the ERC-20 side that also means the Permit2
         // allowance calls below do not run at all while the NFT table is up.
         if mode == ApprovalMode::Fungible {
            for (chain, params) in manager.get_all_active_token_approvals() {
               if ctx.is_chain_disabled(chain) {
                  continue;
               }

               if let Some(chain_filter) = selected_chain
                  && chain_filter.id() != chain
               {
                  continue;
               }

               if let Some(wallet) = &selected_wallet
                  && wallet.address != params.owner
               {
                  continue;
               }

               rows.push(ApprovalRow {
                  chain,
                  owner: params.owner,
                  asset: ApprovalAsset::Token {
                     currency: Currency::from(params.token.clone()),
                     amount: params.amount.clone(),
                  },
                  spender: params.spender,
                  kind: ApprovalKind::Erc20(params),
               });
            }

            let mut permit_groups: HashMap<(u64, Address), Vec<PermitParams>> = HashMap::new();
            for params in manager.get_all_active_permits() {
               if ctx.is_chain_disabled(params.chain) {
                  continue;
               }

               if let Some(chain_filter) = selected_chain
                  && chain_filter.id() != params.chain
               {
                  continue;
               }

               if let Some(wallet) = &selected_wallet
                  && wallet.address != params.owner
               {
                  continue;
               }

               permit_groups.entry((params.chain, params.owner)).or_default().push(params);
            }

            let now = TimeStamp::now_as_secs().ok().map(|t| t.timestamp());

            for ((chain, owner), permits) in permit_groups {
               let pairs: Vec<(Address, Address)> =
                  permits.iter().map(|p| (p.token.address(), p.spender)).collect();
               let onchain = live_permit2_allowances(ctx.clone(), chain, owner, pairs).await;

               for params in permits {
                  let key = (params.token.address(), params.spender);
                  let still_valid = match onchain.get(&key) {
                     Some(&(amount, expiration)) => permit_row_still_valid(amount, expiration, now),
                     // RPC miss — keep the in-app row rather than hiding a live permit.
                     None => true,
                  };

                  if still_valid {
                     rows.push(ApprovalRow {
                        chain: params.chain,
                        owner: params.owner,
                        asset: ApprovalAsset::Token {
                           currency: params.token.clone(),
                           amount: params.amount.clone(),
                        },
                        spender: params.spender,
                        kind: ApprovalKind::Permit2(params),
                     });
                  }
               }
            }
         }

         // NFT approvals. The manager already holds them per shape; the row keeps the collection and
         // the id, and the revoke builds the calldata the shape needs. Nothing here touches the
         // network — the manager has it all — so this mode's rebuild is immediate.
         if mode == ApprovalMode::Nft {
            for params in manager.get_all_active_nft_approvals() {
               if ctx.is_chain_disabled(params.chain) {
                  continue;
               }

               if let Some(chain_filter) = selected_chain
                  && chain_filter.id() != params.chain
               {
                  continue;
               }

               if let Some(wallet) = &selected_wallet
                  && wallet.address != params.owner
               {
                  continue;
               }

               rows.push(ApprovalRow {
                  chain: params.chain,
                  owner: params.owner,
                  asset: ApprovalAsset::Nft {
                     collection: params.collection,
                     token_id: params.token_id,
                     amount: params.amount,
                  },
                  spender: params.operator,
                  kind: ApprovalKind::Nft(params),
               });
            }
         }

         // Token symbol / collection address, then spender — stable enough for browsing.
         rows.sort_by(|a, b| {
            a.asset
               .sort_key()
               .cmp(&b.asset.sort_key())
               .then(a.spender.cmp(&b.spender))
               .then(a.chain.cmp(&b.chain))
         });

         SHARED_GUI.write(|gui| {
            // A superseded fetch owns nothing here, not even the flag: a newer one is in flight, and
            // clearing `loading` would let "no active approvals match your filters" flash before its
            // rows land.
            if gui.approvals.cache_key != key {
               return;
            }
            gui.approvals.cached_rows = rows;
            gui.approvals.loading = false;
            gui.approvals.fade_gen += 1;
            gui.request_repaint();
         });
      });
   }

   /// Force a cache rebuild after a successful revoke.
   fn invalidate_cache(&mut self) {
      self.cached_rows.clear();
      self.cache_key = CacheKey::invalid();
      self.current_page = 0;
   }

   /// The wallet an approval belongs to, named or shortened, and the words for the same address.
   fn wallet_label(&self, ctx: &mut ZeusContext, address: Address) -> (String, SpokenAddress) {
      let name = ctx.get_wallet_name(address);

      drawn_and_spoken(name.as_deref(), &address.to_string())
   }

   /// The spender or operator an approval grants, named or shortened.
   fn spender_label(
      &self,
      ctx: &mut ZeusContext,
      chain: u64,
      spender: Address,
   ) -> (String, SpokenAddress) {
      let name = ctx.get_address_name(chain, spender);

      drawn_and_spoken(name.as_deref(), &spender.to_string())
   }

   /// The name of an NFT collection an approval is over, and the words for the same address.
   ///
   /// `get_address_name` already names a collection Zeus has cached — an approval is often the first
   /// thing the user sees the collection in, though, so an unknown one falls back to its address
   /// rather than to a blank cell.
   fn collection_label(
      &self,
      ctx: &mut ZeusContext,
      chain: u64,
      collection: Address,
   ) -> (String, SpokenAddress) {
      let name = ctx.get_address_name(chain, collection);

      drawn_and_spoken(name.as_deref(), &collection.to_string())
   }

   /// What the Revoke button calls the approval it takes away: the asset the row itself draws.
   ///
   /// Every row draws the same word, so a reader that lists the window hears a column of identical
   /// "Revoke" buttons unless each node says which row it belongs to. The token symbol and the
   /// collection name are already the Asset column's words; this reads the same two sources so the
   /// button can never drift from the cell beside it.
   fn revoke_name(&self, ctx: &mut ZeusContext, row: &ApprovalRow) -> String {
      let asset = match &row.asset {
         ApprovalAsset::Token { currency, .. } => currency.symbol().to_string(),
         ApprovalAsset::Nft { collection, .. } => {
            self.collection_label(ctx, row.chain, *collection).0
         }
      };

      format!("Revoke {asset} approval")
   }

   /// The Tokens / NFTs switch.
   fn mode_switch(&mut self, theme: &Theme, ui: &mut Ui) {
      let tabs_str = ["Tokens", "NFTs"];
      let mut tab = usize::from(self.mode == ApprovalMode::Nft);

      let large = theme.typography.large;

      ui.allocate_ui(vec2(250.0, 30.0), |ui| {
         Tabs::new("approvals_ui_tab", &mut tab, tabs_str).text_size(large).show(ui);
      });

      match tab {
         0 => {
            self.set_mode(ApprovalMode::Fungible);
         }
         _ => {
            self.set_mode(ApprovalMode::Nft);
         }
      }
   }

   fn amount_label(amount: &NumericValue) -> String {
      // ERC20 unlimited is U256::MAX; Permit2 amounts are uint160.
      let wei = amount.wei();
      let u160_max = (U256::from(1u8) << 160) - U256::from(1u8);
      if wei == U256::MAX || wei >= u160_max {
         "Unlimited".to_string()
      } else {
         format!("{:.10}", amount.abbreviated())
      }
   }

   /// The Amount column: how much is approved.
   ///
   /// For an NFT that means the *scope* — which id, or the whole collection — plus the allowance on
   /// the one shape that has one (ERC-5216), since "approved: yes" is not a number.
   fn amount_cell(ui: &mut Ui, asset: &ApprovalAsset, expire: Option<String>, theme: &Theme) {
      let text = match asset {
         ApprovalAsset::Token { amount, .. } => Self::amount_label(amount),
         ApprovalAsset::Nft {
            token_id, amount, ..
         } => match (token_id, amount) {
            // ERC-5216's allowance is a count of units for that id, not a decimal-scaled token
            // amount, so it is shown as the integer it is.
            (Some(id), Some(amount)) => {
               let allowance = match amount == &U256::MAX {
                  true => "Unlimited".to_string(),
                  false => amount.to_string(),
               };
               format!("#{} × {}", id, allowance)
            }
            (Some(id), None) => format!("#{}", id),
            (None, _) => "All tokens".to_string(),
         },
      };

      ui.spacing_mut().item_spacing.x = theme.spacing.xs;

      let unlimited = text == "Unlimited";
      let response =
         ui.label(RichText::new(text).size(theme.typography.normal).color(theme.colors.text));

      // "Unlimited" is a formatting artefact, not a description of what is unlimited: the node
      // says what the number is an allowance of, while the drawn cell keeps the short word.
      if unlimited {
         name_text(ui, &response, "Unlimited allowance");
      }

      if let Some(text) = expire {
         let expire_text = RichText::new(text.as_str()).size(theme.typography.normal);
         let q_mark = RichText::new("?").size(theme.typography.normal);
         let info_tip = Chip::new(q_mark).role(ChipRole::Info);
         let response = ui.add(info_tip).on_hover_text(expire_text);
         // When the allowance lapses is part of the row's meaning, not a tip about it: the
         // sentence rides on the node instead of waiting for a pointer.
         describe(ui, &response, text);
      }
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let frame = Frame::new().inner_margin(10).outer_margin(Margin::symmetric(10, 0));

      show_with_fade(ui, "approvals_ui_fade", self.open, |ui| {
         frame.show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.md);
            ui.spacing_mut().button_padding = theme.button_padding;

            self.rebuild_cache();

            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.spacing_mut().item_spacing.x = theme.spacing.xl;

               let combo_visuals = theme.combo_box_visuals();
               let label_visuals = theme.label_visuals();
               let expansion = Some(6.0);

               // Tokens / NFTs — first, because it decides what the rest of the page is about.
               ui.scope(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.sm;
                  self.mode_switch(theme, ui);
               });

               // Wallet filter
               let wallets = ctx.all_wallets_info_ordered();
               let selected_wallet_name =
                  self.selected_wallet.clone().map_or("All Wallets".to_string(), |wallet| {
                     wallet.name_with_id_short()
                  });

               let text = RichText::new(selected_wallet_name).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .visuals(label_visuals)
                  .fill_width(true)
                  .interactive(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("approvals_wallet_filter", label)
                  .visuals(combo_visuals)
                  .width(200.0)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     let text = RichText::new("All Wallets").size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .fill_width(true)
                        .interactive(true)
                        .sense(Sense::click())
                        .expand(expansion);

                     if ui.add(label).clicked() && self.selected_wallet.is_some() {
                        self.selected_wallet = None;
                        self.current_page = 0;
                     }

                     for wallet in wallets {
                        let text =
                           RichText::new(wallet.name_with_source()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .interactive(true)
                           .sense(Sense::click())
                           .fill_width(true)
                           .expand(expansion);

                        if ui.add(label).clicked()
                           && self.selected_wallet.as_ref().map(|w| w.address)
                              != Some(wallet.address)
                        {
                           self.selected_wallet = Some(wallet.clone());
                           self.current_page = 0;
                        }
                     }
                  });

               // Chain filter
               let selected_chain_name =
                  self.selected_chain.map_or("All Chains".to_string(), |chain| {
                     chain.name().to_string()
                  });

               let text = RichText::new(selected_chain_name).size(theme.typography.normal);
               let label = Label::new(text, None)
                  .visuals(label_visuals)
                  .fill_width(true)
                  .interactive(true)
                  .sense(Sense::click())
                  .expand(expansion);

               ComboBox::new("approvals_chain_filter", label)
                  .visuals(combo_visuals)
                  .width(200.0)
                  .show_ui(ui, |ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     let text = RichText::new("All Chains").size(theme.typography.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .fill_width(true)
                        .interactive(true)
                        .sense(Sense::click())
                        .expand(expansion);

                     if ui.add(label).clicked() && self.selected_chain.is_some() {
                        self.selected_chain = None;
                        self.current_page = 0;
                     }

                     for chain in ChainId::supported_chains() {
                        if ctx.is_chain_disabled(chain.id()) {
                           continue;
                        }

                        let text = RichText::new(chain.name()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .sense(Sense::click())
                           .interactive(true)
                           .fill_width(true)
                           .expand(expansion);

                        if ui.add(label).clicked() && self.selected_chain != Some(chain) {
                           self.selected_chain = Some(chain);
                           self.current_page = 0;
                        }
                     }
                  });
            });

            ui.separator();

            let motion = Motion::from_ui(ui);
            let opacity = animate_with(
               ui,
               ("approvals_ui_content_fade", self.fade_gen),
               1.0,
               motion,
            );

            ui.multiply_opacity(opacity);

            if self.loading {
               ui.vertical_centered(|ui| {
                  ui.horizontal(|ui| {
                     ui.add(Spinner::new().size(20.0));
                     ui.label(
                        RichText::new("Loading approvals")
                           .size(theme.typography.normal)
                           .color(theme.colors.text),
                     );
                  });
               });
               return;
            }

            if self.cached_rows.is_empty() {
               ui.horizontal(|ui| {
                  ui.add_space(300.0);
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                  ui.label(
                     RichText::new("No active approvals match your filters")
                        .size(theme.typography.large)
                        .color(theme.colors.text),
                  );

                  approvals_tip(ui, theme);
               });
               return;
            }

            let total_rows = self.cached_rows.len();
            let total_pages = (total_rows as f64 / self.rows_per_page as f64).ceil() as usize;
            self.current_page = self.current_page.min(total_pages.saturating_sub(1));

            let button_visuals = theme.button_visuals();

            ui.horizontal(|ui| {
               ui.horizontal(|ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.md;
                  ui.spacing_mut().button_padding = vec2(theme.spacing.xs, theme.spacing.sm);

                  let prev_enabled = self.current_page > 0;
                  let text = RichText::new("Previous").size(theme.typography.small);
                  let prev_button = Button::new(text).visuals(button_visuals);
                  if ui.add_enabled(prev_enabled, prev_button).clicked() {
                     self.current_page -= 1;
                  }

                  ui.label(
                     RichText::new(format!(
                        "Page {} of {}",
                        self.current_page + 1,
                        total_pages.max(1)
                     ))
                     .size(theme.typography.small)
                     .color(theme.colors.text),
                  );

                  let next_enabled = (self.current_page + 1) < total_pages;
                  let text = RichText::new("Next").size(theme.typography.small);
                  let next_button = Button::new(text).visuals(button_visuals);
                  if ui.add_enabled(next_enabled, next_button).clicked() {
                     self.current_page += 1;
                  }
               });

               ui.add_space(200.0);
               ui.spacing_mut().item_spacing.x = theme.spacing.xs;

               ui.label(
                  RichText::new(format!("{} active approval(s)", total_rows))
                     .size(theme.typography.large)
                     .color(theme.colors.text),
               );

               approvals_tip(ui, theme);
            });

            ui.add_space(10.0);

            let label_visuals = theme.label_visuals();
            let tint = theme.image_tint_recommended;

            let start = self.current_page * self.rows_per_page;
            let end = start.saturating_add(self.rows_per_page).min(total_rows);
            let rows: &[ApprovalRow] = if start < end {
               &self.cached_rows[start..end]
            } else {
               &[]
            };

            // The trusted-address column is "Operator" in NFT mode: it is the same slot, but an
            // NFT approval grants an operator, which is the word the rest of the UI uses for it.
            let headers = match self.mode {
               ApprovalMode::Fungible => {
                  ["Asset", "Chain", "Wallet", "Spender", "Amount", "Type", ""]
               }
               ApprovalMode::Nft => ["Asset", "Chain", "Wallet", "Operator", "Amount", "Type", ""],
            };

            let mut revoke_request: Option<ApprovalRow> = None;

            let row_frame = Frame::new()
               .interactive(true)
               .inner_margin(theme.inner_margin)
               .corner_radius(theme.corner_radius)
               .visuals(theme.frame1_visuals());

            Table::new("approvals", rows)
               .columns([
                  Column::Fr(2.0),      // Asset
                  Column::Fr(1.3),      // Chain
                  Column::Fr(1.6),      // Wallet
                  Column::Fr(2.2),      // Spender / Operator
                  Column::Fr(1.6),      // Amount
                  Column::Fr(1.3),      // Type
                  Column::Fixed(100.0), // Revoke
               ])
               .gap(20.0)
               // 12 pt, not xs: a themed `Button` paints ~6 pt above and below its band.
               .row_gap(theme.spacing.md)
               .scroll(
                  TableScroll::new()
                     .auto_shrink([false; 2])
                     .content_margin(15)
                     .max_height(ui.available_height() * 0.9),
               )
               .row_frame(row_frame)
               .header_rule(true)
               .key((&self.cache_key, self.current_page))
               .show(ui, |t| {
                  t.header(|h| {
                     for text in headers {
                        h.cell(|ui| {
                           if !text.is_empty() {
                              ui.label(
                                 RichText::new(text)
                                    .strong()
                                    .size(theme.typography.large)
                                    .color(theme.colors.text),
                              );
                           }
                        });
                     }
                  });

                  t.rows(|r, row| {
                     // Asset — a token's symbol, or the collection an NFT approval is over.
                     r.cell(|ui| {
                        let (icon, title, hover, spoken) = match &row.asset {
                           ApprovalAsset::Token { currency, .. } => (
                              icons.currency_icon_x32(currency, tint),
                              currency.symbol().to_string(),
                              currency.name().to_string(),
                              None,
                           ),
                           ApprovalAsset::Nft {
                              collection,
                              token_id,
                              ..
                           } => {
                              // A collection-wide approval has no id, so any of the
                              // collection's cached art is what identifies it — asking for a
                              // fixed id usually lands on art that was never fetched and shows
                              // the placeholder instead.
                              let icon = match token_id {
                                 Some(id) => icons.nft_icon_x64(row.chain, *collection, *id, tint),
                                 None => {
                                    icons.nft_collection_icon_x64(row.chain, *collection, tint)
                                 }
                              };
                              let (title, spoken) =
                                 self.collection_label(ctx, row.chain, *collection);
                              let hover = format!("{}\n{}", title, collection);
                              (icon, title, hover, Some(spoken))
                           }
                        };

                        ui.add(icon);
                        let text = RichText::new(title)
                           .size(theme.typography.normal)
                           .color(theme.colors.text);
                        let label = Label::new(text, None).wrap().visuals(label_visuals);
                        let response = ui.add(label).on_hover_text(hover.as_str());

                        // A token draws only its symbol, and the full name used to be the hover: a
                        // reader gets the name on the node. A collection already goes through
                        // `name_address`, whose description must not be overwritten.
                        if let Some(spoken) = &spoken {
                           name_address(ui, &response, spoken);
                        } else {
                           describe(ui, &response, hover);
                        }
                     });

                     // Chain
                     r.cell(|ui| {
                        let chain = chain_label(row.chain);
                        let text = RichText::new(&chain)
                           .size(theme.typography.normal)
                           .color(theme.colors.text);
                        let label = Label::new(text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .visuals(label_visuals);
                        let response = ui.add(label).on_hover_text(chain.as_str());
                        // The drawn name can be truncated; the full one stays on the node.
                        describe(ui, &response, chain);
                     });

                     // Wallet
                     r.cell(|ui| {
                        let (name, spoken) = self.wallet_label(ctx, row.owner);
                        let text = RichText::new(&name)
                           .size(theme.typography.normal)
                           .color(theme.colors.text);
                        let label = Label::new(text, None)
                           .wrap_mode(TextWrapMode::Truncate)
                           .visuals(label_visuals);
                        // `name_address` names the address and keeps the full form in the
                        // description; the old hover repeated the same two things.
                        let response = ui.add(label);
                        name_address(ui, &response, &spoken);
                     });

                     // Spender
                     r.cell(|ui| {
                        let (name, spoken) = self.spender_label(ctx, row.chain, row.spender);

                        match spender_explorer_url(row.chain, row.spender) {
                           Some(link) => {
                              let text = RichText::new(&name)
                                 .size(theme.typography.normal)
                                 .color(theme.colors.info);
                              let response = ui.hyperlink_to(text, link);
                              name_address(ui, &response, &spoken);
                           }
                           // No chain Zeus can name, so no correct explorer for it — drop the
                           // link and still show the spender.
                           None => {
                              let text = RichText::new(&name)
                                 .size(theme.typography.normal)
                                 .color(theme.colors.text);
                              let label = Label::new(text, None)
                                 .wrap_mode(TextWrapMode::Truncate)
                                 .visuals(label_visuals);
                              // `name_address` already keeps the full address as the
                              // description; the hover only repeated it.
                              let response = ui.add(label);
                              name_address(ui, &response, &spoken);
                           }
                        }
                     });

                     // Amount
                     let expire = if row.kind.is_permit2() {
                        Some(format!(
                           "Expires {}",
                           row.kind.expiration().to_relative()
                        ))
                     } else {
                        None
                     };

                     r.cell(|ui| {
                        Self::amount_cell(ui, &row.asset, expire, theme);
                     });

                     // Type
                     r.cell(|ui| {
                        ui.label(
                           RichText::new(row.kind.type_label())
                              .size(theme.typography.normal)
                              .color(theme.colors.text),
                        );
                     });

                     // Revoke — hug the right inner edge of the row.
                     r.cell(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                           let text = RichText::new("Revoke").size(theme.typography.normal);
                           let button = Button::new(text).visuals(button_visuals);
                           // The drawn word stays "Revoke"; the node says which approval, so a
                           // reader that lists the page does not hear a dozen identical buttons.
                           let name = self.revoke_name(ctx, row);
                           if ui.add(button).accessible_name(name).clicked() {
                              revoke_request = Some(row.clone());
                           }
                        });
                     });
                  });
               });

            if let Some(row) = revoke_request {
               self.revoke(row);
            }
         });
      });
   }

   fn revoke(&mut self, row: ApprovalRow) {
      match row.kind {
         ApprovalKind::Erc20(params) => {
            let chain = row.chain;
            let token = params.token.clone();
            let owner = params.owner;
            let spender = params.spender;
            crate::runtime::rt().spawn(async move {
               if let Err(e) = revoke_erc20_approval(chain, token, owner, spender).await {
                  tracing::error!("Failed to revoke ERC20 approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_failed();
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_sent();
                     gui.request_repaint();
                  });
               }
            });
         }
         ApprovalKind::Permit2(params) => {
            let chain = params.chain;
            let owner = params.owner;
            let token = params.token.clone();
            let spender = params.spender;
            crate::runtime::rt().spawn(async move {
               if let Err(e) = revoke_permit2_approval(chain, owner, token, spender).await {
                  tracing::error!("Failed to revoke Permit2 approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_failed();
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_sent();
                     gui.request_repaint();
                  });
               }
            });
         }
         ApprovalKind::Nft(params) => {
            let chain = params.chain;
            crate::runtime::rt().spawn(async move {
               if let Err(e) = revoke_nft_approval(chain, params).await {
                  tracing::error!("Failed to revoke NFT approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_failed();
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.revoke_sent();
                     gui.request_repaint();
                  });
               }
            });
         }
      }
   }

   /// Record a revoke the chain accepted: its rows are stale, and the user is told once.
   ///
   /// Success used to clear the cache silently, so the row vanished and nothing said the
   /// transaction had been sent. This runs after the frame that started it, and the frame keeps one
   /// announcement per frame, so the sentence waits in the page's channel until a frame drains it.
   fn revoke_sent(&mut self) {
      self.invalidate_cache();
      self.notice = Some("Revocation sent.".to_string());
   }

   /// Record a revoke that failed: the message window already carries the reason, and this is the
   /// one sentence a reader who cannot see that window needs.
   fn revoke_failed(&mut self) {
      self.notice = Some("Revocation failed.".to_string());
   }
}

/// Put a sentence a drawn cell only shows on hover on the node the label already made.
///
/// A tooltip is a pointer-only surface: a reader never asks for one, so the words that explain a
/// cell have to be part of the cell's own node.
fn describe(ui: &Ui, response: &Response, text: impl Into<String>) {
   let text = text.into();
   ui.ctx().accesskit_node_builder(response.id, move |node| {
      node.set_description(text);
   });
}

/// The `?` that explains what the approvals page lists and why it cannot be complete.
///
/// The chip keeps the drawn question mark it is, but its node carries the name and the sentence:
/// the tip is the whole point of the chip, and a hover is not reachable without a pointer.
fn approvals_tip(ui: &mut Ui, theme: &Theme) -> Response {
   let q_mark = RichText::new("?").size(theme.typography.normal);
   let info_tip = Chip::new(q_mark).role(ChipRole::Info);
   let response = ui.add(info_tip).on_hover_text(ZEUS_TIP);

   name_text(ui, &response, "About approvals");
   describe(ui, &response, ZEUS_TIP);

   response
}

/// Whether a Permit2 row is still worth showing: any allowance left, and not expired.
///
/// Not "still at the amount that was recorded" — spending part of an allowance does not stop the
/// spender spending the rest, and the row is where the Revoke button lives, so a partially spent
/// allowance is the one that most needs showing.
fn permit_row_still_valid(remaining: U256, expiration: u64, now: Option<u64>) -> bool {
   let expired = now.map(|n| expiration < n).unwrap_or(false);
   !remaining.is_zero() && !expired
}

/// The explorer link for an approval's spender, or `None` when the row names a chain Zeus has no
/// entry for.
///
/// An approval row is rebuilt from the store and its chain is not re-validated on load, so the link
/// cannot be built with the panicking `ChainId::from` — and there is no correct explorer for a chain
/// Zeus does not know. The spender cell keeps naming the spender; only the link is dropped.
fn spender_explorer_url(chain: u64, spender: Address) -> Option<String> {
   let chain = usable_chain(chain)?;
   Some(format!(
      "{}/address/{}",
      chain.block_explorer(),
      spender
   ))
}

/// The chain an approval can be revoked on, or the error every revoke flow returns before it builds
/// any calldata, address or signature.
///
/// The row's chain came out of the store, and a revoke needs a real [`ChainId`] for the fork spec,
/// the address book and the recorded params. There is nothing to guess with — the wallet's own chain
/// would revoke on a chain the approval was never made on — so refuse by id instead.
fn revoke_chain(chain_id: u64) -> Result<ChainId, anyhow::Error> {
   usable_chain(chain_id)
      .ok_or_else(|| anyhow!("this approval is on a chain Zeus cannot use ({chain_id})"))
}

// ! This may return an empty or partial map if any requests fail
async fn live_permit2_allowances(
   ctx: ZeusCtx,
   chain: u64,
   owner: Address,
   pairs: Vec<(Address, Address)>,
) -> HashMap<(Address, Address), (U256, u64)> {
   if pairs.is_empty() {
      return HashMap::new();
   }

   let Ok(permit2) = address_book::permit2_contract(chain) else {
      return HashMap::new();
   };

   let client = ctx.get_client_manager();
   let mut out = HashMap::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request_with(chain, chunk.clone(), |client, chunk| async move {
            batch::get_permit2_allowances(client, permit2, owner, chunk, None).await
         })
         .await
      {
         Ok(rows) => {
            for (token, spender, amount, expiration) in rows {
               out.insert((token, spender), (amount, expiration));
            }
         }
         Err(e) => {
            tracing::warn!("Permit2 allowances failed: {:?}", e);
         }
      }
   }

   out
}

/// The calldata that revokes an NFT approval, with the params the manager will record for it.
///
/// There is no generic "revoke": each of the three shapes is cleared a different way, and they are
/// **not** interchangeable. `setApprovalForAll(operator, false)` against a grant that was made with
/// per-token `approve` revokes nothing at all — the transaction succeeds, the row stays, and the user
/// has paid gas to believe they un-approved someone. So every shape is matched explicitly, and the
/// combinations the three shapes never emit are refused rather than guessed at.
///
/// The returned params are the *cleared* ones on purpose: they are what the manager records, and
/// recording the grant as it was would leave it looking active after its revocation. The one field
/// that stays as it was is the ERC-721 per-token operator — see the arm below.
fn revoke_call(params: &NftApproveParams) -> Result<(Bytes, NftApproveParams), anyhow::Error> {
   let mut recorded = params.clone();

   let calldata = match (params.token_id, params.approved, params.amount) {
      // ERC-721 per-token: clear the operator to the zero address — that is what `approve` writes on
      // chain, and what this calldata sends. The recorded params keep the operator the grant is
      // being taken from instead: the zero address names nobody, and this is the screen the user
      // consents on, so `revoked` is what marks the record as a revocation.
      (Some(id), None, None) => {
         recorded.revoked = true;
         erc721::encode_approve(Address::ZERO, id)
      }
      // `ApprovalForAll`: unset the flag. The operator stays, which is what lets the store find and
      // drop the very row this revokes — a collection-wide entry is keyed by its operator.
      (None, Some(_), None) => {
         recorded.approved = Some(false);
         erc721::encode_set_approval_for_all(params.operator, false)
      }
      // ERC-5216: a zero allowance for this operator and this id.
      (Some(id), None, Some(_)) => {
         recorded.amount = Some(U256::ZERO);
         erc1155::encode_approve(params.operator, id, U256::ZERO)
      }
      _ => {
         return Err(anyhow!(
            "no revoke for this NFT approval shape (id: {:?}, approved: {:?}, amount: {:?})",
            params.token_id,
            params.approved,
            params.amount
         ));
      }
   };

   Ok((calldata, recorded))
}

/// Revoke an NFT approval on the collection that emitted it.
async fn revoke_nft_approval(chain_id: u64, params: NftApproveParams) -> Result<(), anyhow::Error> {
   let chain = revoke_chain(chain_id)?;
   let (calldata, revoked) = revoke_call(&params)?;

   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Revoking the NFT approval…");
      gui.request_repaint();
      gui.ctx.clone()
   });

   let value = U256::ZERO;
   let dapp = "".to_string();
   let mev_protect = false;
   let auth_list = vec![];
   let interact_to = params.collection;
   let source_is_zeus = true;

   let simulated = simulate_for_analysis(
      ctx.clone(),
      chain,
      params.owner,
      interact_to,
      calldata.clone(),
      value,
      Vec::new(),
   )
   .await?;

   let mut analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      params.owner,
      interact_to,
      Some(true),
      calldata.clone(),
      value,
      simulated.logs,
      simulated.gas_used,
      simulated.balance_before,
      simulated.balance_after,
      auth_list.clone(),
   )
   .await?;
   analysis.set_block_number(simulated.block_number);
   analysis.set_main_event(DecodedEvent::NftApprove(revoked));

   let (_, _) = send_transaction(
      ctx,
      source_is_zeus,
      SendTxRequest::new(chain, params.owner, interact_to)
         .call_data(calldata)
         .value(value)
         .authorization_list(auth_list)
         .analysis(analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         // A per-token ERC-721 revoke is `approve(0, id)` on chain, so the receipt log names the
         // zero address, not the operator being cleared. Keep the flow's own intent event — the
         // recorded params carry the operator the grant is being taken from.
         keep_intent_event: true,
         ..Default::default()
      },
   )
   .await?;

   Ok(())
}

async fn revoke_erc20_approval(
   chain_id: u64,
   token: ERC20Token,
   from: Address,
   spender: Address,
) -> Result<(), anyhow::Error> {
   let chain = revoke_chain(chain_id)?;

   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Revoking the approval…");
      gui.request_repaint();
      gui.ctx.clone()
   });

   let calldata = token.encode_approve(spender, U256::ZERO);
   let value = U256::ZERO;
   let dapp = "".to_string();
   let mev_protect = false;
   let auth_list = vec![];
   let interact_to = token.address;
   let source_is_zeus = true;

   let simulated = simulate_for_analysis(
      ctx.clone(),
      chain,
      from,
      interact_to,
      calldata.clone(),
      value,
      Vec::new(),
   )
   .await?;

   let params = TokenApproveParams {
      token: token.clone(),
      amount: NumericValue::default(),
      amount_usd: None,
      owner: from,
      spender,
   };

   let mut analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      from,
      interact_to,
      Some(true),
      calldata.clone(),
      value,
      simulated.logs,
      simulated.gas_used,
      simulated.balance_before,
      simulated.balance_after,
      auth_list.clone(),
   )
   .await?;
   analysis.set_block_number(simulated.block_number);
   analysis.set_main_event(DecodedEvent::TokenApprove(params));

   let (_, _) = send_transaction(
      ctx,
      source_is_zeus,
      SendTxRequest::new(chain, from, interact_to)
         .call_data(calldata)
         .value(value)
         .authorization_list(auth_list)
         .analysis(analysis),
      SendTxOptions {
         dapp,
         mev_protect,
         ..Default::default()
      },
   )
   .await?;

   Ok(())
}

/// Revoke a Permit2 allowance by signing a zero-amount PermitSingle and
/// submitting it via `Permit2.permit`.
async fn revoke_permit2_approval(
   chain_id: u64,
   owner: Address,
   token: Currency,
   spender: Address,
) -> Result<(), anyhow::Error> {
   let chain = revoke_chain(chain_id)?;

   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing the Permit2 revoke…");
      gui.request_repaint();
      gui.ctx.clone()
   });

   let token_addr = token.address();

   let permit2 = address_book::permit2_contract(chain_id)?;
   let client = ctx.get_client_manager();

   let allowance_data = client
      .request(chain_id, |client| async move {
         allowance(client, permit2, owner, token_addr, spender).await
      })
      .await?;

   let current_time = TimeStamp::now_as_secs()?.timestamp();
   let amount = U256::ZERO;
   // Zero expiration is valid for a revoked / empty allowance.
   let expiration = U256::ZERO;
   let sig_deadline = U256::from(current_time + 30 * 60); // 30 minutes

   let msg = signature::generate_permit2_json_value(
      chain_id,
      token_addr,
      spender,
      amount,
      permit2,
      expiration,
      sig_deadline,
      allowance_data.nonce,
   );

   let signature = signature::sign::sign_message(
      ctx.clone(),
      "".to_string(),
      chain,
      Some(msg),
      None,
      Some(owner),
   )
   .await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Revoking the Permit2 approval…");
      gui.request_repaint();
   });

   let calldata = encode_permit_single_call(
      owner,
      token_addr,
      amount,
      expiration,
      allowance_data.nonce,
      spender,
      sig_deadline,
      signature,
   );

   let simulated = simulate_for_analysis(
      ctx.clone(),
      chain,
      owner,
      permit2,
      calldata.clone(),
      U256::ZERO,
      Vec::new(),
   )
   .await?;

   let params = PermitParams {
      event_name: "Revoke Permit".to_string(),
      chain: chain_id,
      owner,
      token,
      spender,
      amount: NumericValue::default(),
      amount_usd: None,
      expiration: TimeStamp::Seconds(0),
   };

   let mut analysis = TransactionAnalysis::new(
      ctx.clone(),
      chain.id(),
      owner,
      permit2,
      Some(true),
      calldata.clone(),
      U256::ZERO,
      simulated.logs,
      simulated.gas_used,
      simulated.balance_before,
      simulated.balance_after,
      vec![],
   )
   .await?;
   analysis.set_block_number(simulated.block_number);
   analysis.set_main_event(DecodedEvent::Permit(params));

   let source_is_zeus = true;

   let (_, _) = send_transaction(
      ctx,
      source_is_zeus,
      SendTxRequest::new(chain, owner, permit2).call_data(calldata).analysis(analysis),
      SendTxOptions::default(),
   )
   .await?;

   Ok(())
}

#[cfg(test)]
mod tests {
   use super::*;
   use alloy_sol_types::SolEvent;
   use zeus_eth::alloy_primitives::{Log, address};

   const OWNER: Address = address!("1111111111111111111111111111111111111111");
   const COLLECTION: Address = address!("BC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
   const OPERATOR: Address = address!("f39fd6e51aad88f6f4ce6ab8827279cfffb92266");
   const ID: u64 = 1071;

   /// A partially spent allowance is the case this table exists for, and the one where revoking
   /// matters most — it used to be the case that hid the row.
   #[test]
   fn a_permit_row_lives_while_any_allowance_is_left() {
      let now = Some(1_700_000_000);
      let live_until = 1_700_000_000 + 3_600;

      // Untouched, then all but spent: both are spendable, so both keep their row.
      assert!(permit_row_still_valid(
         U256::from(100),
         live_until,
         now
      ));
      assert!(permit_row_still_valid(
         U256::from(1),
         live_until,
         now
      ));
      // Nothing left to spend.
      assert!(!permit_row_still_valid(
         U256::ZERO,
         live_until,
         now
      ));
      // Expired is expired however much is left.
      assert!(!permit_row_still_valid(
         U256::from(100),
         1_699_999_999,
         now
      ));
      // A clock that could not be read is not evidence of expiry.
      assert!(permit_row_still_valid(
         U256::from(100),
         live_until,
         None
      ));
   }

   /// The params exactly as the decode ladder produces them, so these tests revoke what a real row
   /// in the table is built from.
   fn erc721_grant() -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: erc721::IERC721::Approval {
            owner: OWNER,
            approved: OPERATOR,
            tokenId: U256::from(ID),
         }
         .encode_log_data(),
      };
      NftApproveParams::from_erc721_approval(1, &log).unwrap()
   }

   fn approval_for_all(approved: bool) -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: erc721::IERC721::ApprovalForAll {
            owner: OWNER,
            operator: OPERATOR,
            approved,
         }
         .encode_log_data(),
      };
      NftApproveParams::from_approval_for_all(1, &log).unwrap()
   }

   fn erc5216_allowance(amount: u64) -> NftApproveParams {
      let log = Log {
         address: COLLECTION,
         data: erc1155::IERC5216::Approval {
            account: OWNER,
            operator: OPERATOR,
            id: U256::from(ID),
            amount: U256::from(amount),
         }
         .encode_log_data(),
      };
      NftApproveParams::from_erc1155_approval(1, &log).unwrap()
   }

   /// An ERC-721 per-token grant is revoked by approving the zero address for that id. The zero
   /// address is what the chain is told; the params the user is shown keep the operator whose grant
   /// is being taken away, because a row naming `0x0` cannot say whose spending rights are going.
   /// `ApprovalForAll` is revoked by unsetting the flag — **not** by `approve(0, id)`, which would
   /// revoke one token of a collection-wide grant and leave the rest of it live — and ERC-5216 with a
   /// zero allowance for that operator and id. No other shape emits a log that cannot be revoked, so
   /// an unrecognized combination is an error rather than a guessed calldata: a wrong revoke sends a
   /// real transaction that revokes something else.
   #[test]
   fn revoke_call_clears_each_shape_and_refuses_the_rest() {
      // Case: an_erc721_grant_is_revoked_with_approve_zero
      let grant = erc721_grant();
      let (calldata, revoked) = revoke_call(&grant).unwrap();

      assert_eq!(
         calldata,
         erc721::encode_approve(Address::ZERO, U256::from(ID)),
         "the chain is still told to clear the operator"
      );
      assert_eq!(
         revoked.operator, grant.operator,
         "the recorded operator is the one whose rights are being removed"
      );
      assert!(!revoked.operator.is_zero());
      assert!(
         revoked.revoked,
         "and the record says it is a revocation"
      );
      assert_eq!(revoked.token_id, Some(U256::from(ID)));
      assert!(revoked.is_revoke());

      // Case: an_approval_for_all_is_revoked_with_the_flag
      let (calldata, revoked) = revoke_call(&approval_for_all(true)).unwrap();

      assert_eq!(
         calldata,
         erc721::encode_set_approval_for_all(OPERATOR, false)
      );
      assert_eq!(
         revoked.operator, OPERATOR,
         "the operator stays: it is what the store keys the revoked row by"
      );
      assert_eq!(revoked.approved, Some(false));
      assert_eq!(revoked.token_id, None);
      assert!(revoked.is_revoke());

      // Case: an_erc5216_allowance_is_revoked_with_zero_amount
      let (calldata, revoked) = revoke_call(&erc5216_allowance(5)).unwrap();

      assert_eq!(
         calldata,
         erc1155::encode_approve(OPERATOR, U256::from(ID), U256::ZERO)
      );
      assert_eq!(revoked.amount, Some(U256::ZERO));
      assert_eq!(revoked.operator, OPERATOR);
      assert!(revoked.is_revoke());

      assert_ne!(
         calldata,
         erc721::encode_approve(Address::ZERO, U256::from(ID)),
         "an ERC-5216 revoke is not an ERC-721 one"
      );

      // Case: an_unrepresentable_shape_is_refused
      let mut params = erc721_grant();
      params.token_id = None;

      assert!(revoke_call(&params).is_err());
   }

   /// A row's chain comes out of the store, so the two decisions the table makes from it have to
   /// refuse a chain Zeus cannot name — the panicking conversion is what they replaced.
   ///
   /// Swept over the supported set, where both must behave exactly as before (a real chain, and its
   /// own explorer link), plus an ordinary unsupported id and one that cannot be a chain at all,
   /// where there is no chain to act on and no explorer to point at.
   #[test]
   fn a_stored_chain_is_refused_when_zeus_cannot_name_it() {
      use zeus_eth::types::SUPPORTED_CHAINS;

      for id in SUPPORTED_CHAINS {
         let chain = ChainId::new(id).unwrap();
         assert_eq!(revoke_chain(id).unwrap().id(), id, "chain {id}");
         assert_eq!(
            spender_explorer_url(id, OPERATOR).unwrap(),
            format!("{}/address/{}", chain.block_explorer(), OPERATOR),
            "chain {id}"
         );
      }

      for id in [137, u64::MAX] {
         let err = revoke_chain(id).unwrap_err().to_string();
         assert!(
            err.contains(&id.to_string()),
            "the refusal must name the chain it refused: {err}"
         );
         assert!(
            spender_explorer_url(id, OPERATOR).is_none(),
            "no explorer exists for chain {id}"
         );
      }
   }

   /// The page's accessible tree: what a reader is handed for a page holding known rows.
   ///
   /// ApprovalRow and ApprovalsUi are private to this file, so the fixtures live here rather than
   /// widening the row type for the shared accessibility suite. The frame below reads the same
   /// platform_output.accesskit_update value that suite reads.
   mod accessibility {
      use super::*;
      use egui::accesskit::{Node, NodeId, Role};
      use egui::{Context, RawInput, Rect, pos2, vec2};
      use egui_elements::ThemeKind;
      use zeus_eth::alloy_primitives::address;
      use zeus_eth::nft::{NftCollection, NftStandard};

      const USDC: Address = address!("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48");

      /// Run one AccessKit-enabled frame and return the node ids and nodes it produced.
      fn frame_nodes(ctx: &Context, add: impl FnMut(&mut Ui)) -> Vec<(NodeId, Node)> {
         ctx.enable_accesskit();

         let mut output = ctx.run_ui(
            RawInput {
               screen_rect: Some(Rect::from_min_size(
                  pos2(0.0, 0.0),
                  vec2(1290.0, 950.0),
               )),
               ..Default::default()
            },
            add,
         );

         let update = output
            .platform_output
            .accesskit_update
            .take()
            .expect("an AccessKit-enabled frame must produce a tree update");

         let nodes = update.nodes.into_iter().collect();
         output.drop_without_applying_deltas();

         nodes
      }

      /// The pieces the page needs: the installed theme, the icon store its cells draw with, and a
      /// frame with no wallet behind it.
      fn page_harness() -> (Context, ZeusCtx, Theme, Arc<Icons>) {
         let ctx = Context::default();
         let mut theme = Theme::new(ThemeKind::TokyoNight);
         theme.install(&ctx);
         egui_extras::install_image_loaders(&ctx);

         let icons = Arc::new(Icons::new(&ctx).expect("icons"));
         let handle = ZeusCtx::new(crate::runtime::test_handle());

         (ctx, handle, theme, icons)
      }

      /// Draw the page for three frames and return the tree of the last one: the content fades in
      /// and the table lays its rows out after the first pass.
      fn render_page(
         page: &mut ApprovalsUi,
         ctx: &Context,
         handle: &ZeusCtx,
         theme: &Theme,
         icons: &Arc<Icons>,
      ) -> Vec<(NodeId, Node)> {
         let mut nodes = Vec::new();

         for _ in 0..3 {
            nodes = frame_nodes(ctx, |ui| {
               handle.write(|zeus| page.show(zeus, theme, icons.clone(), ui));
            });
         }

         nodes
      }

      /// Whether a node is named by the given word, either way egui spells a label for AccessKit.
      fn is_named(node: &Node, name: &str) -> bool {
         node.label() == Some(name) || node.value() == Some(name)
      }

      /// Every name and description in the tree, for a failure message.
      fn spoken(nodes: &[(NodeId, Node)]) -> Vec<String> {
         nodes
            .iter()
            .flat_map(|(_, node)| [node.label(), node.value(), node.description()])
            .flatten()
            .map(str::to_string)
            .collect()
      }

      fn usdc() -> ERC20Token {
         ERC20Token::from_components(1, USDC, "USDC", "USD Coin", 6, U256::ZERO)
      }

      /// An ERC-20 approval over `amount` of USDC.
      fn token_row(amount: NumericValue) -> ApprovalRow {
         let token = usdc();

         ApprovalRow {
            chain: 1,
            owner: OWNER,
            asset: ApprovalAsset::Token {
               currency: Currency::from(token.clone()),
               amount: amount.clone(),
            },
            spender: OPERATOR,
            kind: ApprovalKind::Erc20(TokenApproveParams {
               token,
               amount,
               amount_usd: None,
               owner: OWNER,
               spender: OPERATOR,
            }),
         }
      }

      fn usdc_row() -> ApprovalRow {
         // The Amount column reads the wei form, which `from_f64` does not carry.
         token_row(NumericValue::parse_to_wei("100", 6))
      }

      /// A Permit2 allowance, which is the only row with an expiry to explain.
      fn permit_row() -> ApprovalRow {
         let token = Currency::from(usdc());
         let amount = NumericValue::parse_to_wei("1", 6);

         ApprovalRow {
            chain: 1,
            owner: OWNER,
            asset: ApprovalAsset::Token {
               currency: token.clone(),
               amount: amount.clone(),
            },
            spender: OPERATOR,
            kind: ApprovalKind::Permit2(PermitParams {
               event_name: "Token Permit".to_string(),
               chain: 1,
               owner: OWNER,
               token,
               spender: OPERATOR,
               amount,
               amount_usd: None,
               expiration: TimeStamp::Seconds(1_800_000_000),
            }),
         }
      }

      fn bayc_row() -> ApprovalRow {
         let params = approval_for_all(true);

         ApprovalRow {
            chain: params.chain,
            owner: params.owner,
            asset: ApprovalAsset::Nft {
               collection: params.collection,
               token_id: params.token_id,
               amount: params.amount,
            },
            spender: params.operator,
            kind: ApprovalKind::Nft(params),
         }
      }

      /// The collection name the Asset column draws, so the Revoke button can be checked against
      /// the same word rather than a hard-coded address form.
      fn name_bayc(handle: &ZeusCtx) {
         handle.write(|ctx| {
            ctx.nft_db.insert_collection(
               1,
               NftCollection {
                  chain_id: 1,
                  address: COLLECTION,
                  standard: NftStandard::Erc721,
                  name: Some("BAYC".to_string()),
                  symbol: Some("BAYC".to_string()),
               },
            );
         });
      }

      /// Every row draws the same word, so the button's node must say which approval it takes away:
      /// a reader that lists the page otherwise hears a column of identical "Revoke" buttons.
      #[test]
      fn each_revoke_button_names_the_row_it_revokes() {
         let (ctx, handle, theme, icons) = page_harness();
         name_bayc(&handle);
         let mut page = ApprovalsUi::for_test(vec![usdc_row(), bayc_row()]);

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);
         let names: Vec<&str> = nodes
            .iter()
            .filter(|(_, node)| node.role() == Role::Button)
            .filter_map(|(_, node)| node.label())
            .collect();

         assert!(
            names.contains(&"Revoke USDC approval"),
            "the token row's button must name its asset: {names:?}"
         );
         assert!(
            names.contains(&"Revoke BAYC approval"),
            "the NFT row's button must name its collection: {names:?}"
         );
         assert!(
            !names.contains(&"Revoke"),
            "no button may be left with the bare word: {names:?}"
         );
      }

      /// The token cell draws only a symbol and used to leave the full name in a hover: a reader
      /// gets that name on the node, and the chain cell keeps its (possibly truncated) name there
      /// too.
      #[test]
      fn the_asset_cell_says_the_name_its_hover_carried() {
         let (ctx, handle, theme, icons) = page_harness();
         let mut page = ApprovalsUi::for_test(vec![usdc_row()]);

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);
         let asset = nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Label && is_named(node, "USDC"))
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!(
                  "the token cell must still draw its symbol: {:?}",
                  spoken(&nodes)
               )
            });

         assert_eq!(
            asset.description(),
            Some("USD Coin"),
            "the full name the hover carried must be on the node: {:?}",
            spoken(&nodes)
         );

         let chain = nodes
            .iter()
            .find(|(_, node)| node.role() == Role::Label && is_named(node, "Ethereum"))
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!(
                  "the chain cell must still draw its name: {:?}",
                  spoken(&nodes)
               )
            });

         assert_eq!(
            chain.description(),
            Some("Ethereum"),
            "a chain name cut by the column must still be whole on the node"
         );
      }

      /// "Unlimited" is what the formatting does, not what the allowance is: the node says what
      /// the word is an allowance of, while the cell still draws the short word.
      #[test]
      fn an_unlimited_allowance_is_named_as_one() {
         let (ctx, handle, theme, icons) = page_harness();
         // ERC-20 unlimited is U256::MAX, which amount_label reads from the wei form.
         let mut page = ApprovalsUi::for_test(vec![token_row(NumericValue::currency_balance(
            U256::MAX,
            6,
         ))]);

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);

         assert!(
            nodes.iter().any(|(_, node)| is_named(node, "Unlimited allowance")),
            "an unlimited allowance must say so on its node: {:?}",
            spoken(&nodes)
         );
      }

      /// The empty state's only explanation is the question-mark chip: its sentence is the chip's
      /// description, not a hover a reader never reaches.
      #[test]
      fn the_empty_state_help_is_on_its_node() {
         let (ctx, handle, theme, icons) = page_harness();
         let mut page = ApprovalsUi::for_test(Vec::new());

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);
         let tip = nodes
            .iter()
            .find(|(_, node)| is_named(node, "About approvals"))
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!(
                  "the empty state's ? chip must be named: {:?}",
                  spoken(&nodes)
               )
            });

         assert_eq!(
            tip.description(),
            Some(ZEUS_TIP),
            "the tip must be the chip's description, not only a hover"
         );
      }

      /// A Permit2 allowance lapses, and when it does is part of the row: the chip says it on its
      /// node instead of only inside a hover.
      #[test]
      fn a_permit_row_says_when_the_allowance_expires() {
         let (ctx, handle, theme, icons) = page_harness();
         let mut page = ApprovalsUi::for_test(vec![permit_row()]);

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);
         let chip = nodes
            .iter()
            .find(|(_, node)| node.description().is_some_and(|text| text.starts_with("Expires ")))
            .map(|(_, node)| node)
            .unwrap_or_else(|| {
               panic!(
                  "the Permit2 row's ? chip must carry the expiry: {:?}",
                  spoken(&nodes)
               )
            });

         assert!(
            is_named(chip, "?"),
            "the expiry belongs to the question-mark chip: {:?}",
            spoken(&nodes)
         );
      }

      /// A bare spinner says nothing about what is being waited for; the page says it in words.
      #[test]
      fn the_loading_page_says_loading_approvals() {
         let (ctx, handle, theme, icons) = page_harness();
         let mut page = ApprovalsUi::for_test(Vec::new());
         page.loading = true;

         let nodes = render_page(&mut page, &ctx, &handle, &theme, &icons);

         assert!(
            nodes.iter().any(|(_, node)| is_named(node, "Loading approvals")),
            "the loading page must say what it is waiting for: {:?}",
            spoken(&nodes)
         );
      }

      /// A revoke finishes after the frame that started it, so its sentence waits in the page's
      /// channel: the frame says it once, and the stale rows do not stay up.
      #[test]
      fn a_finished_revoke_is_said_once() {
         let mut page = ApprovalsUi::for_test(vec![usdc_row()]);
         page.revoke_sent();

         assert!(
            page.cached_rows.is_empty(),
            "the rows a sent revoke changes must not stay up"
         );
         assert_eq!(
            page.take_notice().as_deref(),
            Some("Revocation sent.")
         );
         assert_eq!(page.take_notice(), None, "the frame says it once");

         let mut page = ApprovalsUi::for_test(vec![usdc_row()]);
         page.revoke_failed();

         assert_eq!(
            page.take_notice().as_deref(),
            Some("Revocation failed.")
         );
         assert_eq!(
            page.take_notice(),
            None,
            "a failure is said once, like a success"
         );
      }
   }
}
