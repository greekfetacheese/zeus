//! UI for viewing and revoking ERC20 / Permit2 / NFT token approvals.

use crate::assets::icons::Icons;
use crate::core::{
   DecodedEvent, NftApproveParams, PermitParams, SendTxOptions, SendTxRequest, TokenApproveParams,
   TransactionAnalysis, WalletInfo, ZeusContext, ZeusCtx, send_transaction, signature,
};
use crate::gui::{SHARED_GUI, ui::show_with_fade};
use crate::utils::{RT, TimeStamp, simulate::simulate_for_analysis, truncate_address};
use anyhow::anyhow;
use egui::{
   Align, Frame, Layout, Margin, RichText, ScrollArea, Sense, Spinner, TextWrapMode, Ui, UiBuilder,
   vec2,
};
use egui_elements::{Button, ComboBox, Label, Theme};
use elegance::{Badge, BadgeTone};
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
            NftStandard::Erc721 => "ERC-721",
            NftStandard::Erc1155 => "ERC-1155",
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

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CacheKey {
   wallet: Option<Address>,
   chain: Option<u64>,
}

impl CacheKey {
   fn invalid() -> Self {
      Self {
         wallet: None,
         chain: Some(u64::MAX),
      }
   }
}

pub struct ApprovalsUi {
   open: bool,
   loading: bool,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   cached_rows: Vec<ApprovalRow>,
   cache_key: CacheKey,
   current_page: usize,
   rows_per_page: usize,
}

impl ApprovalsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         loading: false,
         selected_wallet: None,
         selected_chain: None,
         cached_rows: Vec::new(),
         cache_key: CacheKey::default(),
         current_page: 0,
         rows_per_page: DEFAULT_ROWS_PER_PAGE,
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if self.open {
         return;
      }

      self.open = true;
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

   fn current_cache_key(&self) -> CacheKey {
      CacheKey {
         wallet: self.selected_wallet.as_ref().map(|w| w.address),
         chain: self.selected_chain.map(|c| c.id()),
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

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let manager = ctx.approval_manager();

         let mut rows = Vec::new();

         for (chain, params) in manager.get_all_active_token_approvals() {
            if ctx.is_chain_disabled(chain) {
               continue;
            }

            if let Some(chain_filter) = selected_chain {
               if chain_filter.id() != chain {
                  continue;
               }
            }

            if let Some(wallet) = &selected_wallet {
               if wallet.address != params.owner {
                  continue;
               }
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

         // NFT approvals. The manager already holds them per shape; the row keeps the collection and
         // the id, and the revoke builds the calldata the shape needs.
         for params in manager.get_all_active_nft_approvals() {
            if ctx.is_chain_disabled(params.chain) {
               continue;
            }

            if let Some(chain_filter) = selected_chain {
               if chain_filter.id() != params.chain {
                  continue;
               }
            }

            if let Some(wallet) = &selected_wallet {
               if wallet.address != params.owner {
                  continue;
               }
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

         let mut permit_groups: HashMap<(u64, Address), Vec<PermitParams>> = HashMap::new();
         for params in manager.get_all_active_permits() {
            if ctx.is_chain_disabled(params.chain) {
               continue;
            }

            if let Some(chain_filter) = selected_chain {
               if chain_filter.id() != params.chain {
                  continue;
               }
            }

            if let Some(wallet) = &selected_wallet {
               if wallet.address != params.owner {
                  continue;
               }
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
                  Some(&(amount, expiration)) => {
                     let expired = now.map(|n| expiration < n).unwrap_or(false);
                     amount >= params.amount.wei() && !expired
                  }
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

         // Token symbol / collection address, then spender — stable enough for browsing.
         rows.sort_by(|a, b| {
            a.asset
               .sort_key()
               .cmp(&b.asset.sort_key())
               .then(a.spender.cmp(&b.spender))
               .then(a.chain.cmp(&b.chain))
         });

         SHARED_GUI.write(|gui| {
            if gui.approvals.cache_key == key {
               gui.approvals.cached_rows = rows;
            }
            gui.approvals.loading = false;
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

   fn wallet_name(&self, ctx: &mut ZeusContext, address: Address) -> String {
      ctx.get_wallet_name(address)
         .unwrap_or_else(|| truncate_address(address.to_string()))
   }

   fn spender_label(&self, ctx: &mut ZeusContext, chain: u64, spender: Address) -> String {
      ctx.get_address_name(chain, spender)
         .map(|s| s.to_string())
         .unwrap_or_else(|| truncate_address(spender.to_string()))
   }

   /// The name of an NFT collection an approval is over.
   ///
   /// `get_address_name` already names a collection Zeus has cached — an approval is often the first
   /// thing the user sees the collection in, though, so an unknown one falls back to its address
   /// rather than to a blank cell.
   fn collection_label(&self, ctx: &mut ZeusContext, chain: u64, collection: Address) -> String {
      ctx.get_address_name(chain, collection)
         .map(|s| s.to_string())
         .unwrap_or_else(|| truncate_address(collection.to_string()))
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

   /// Fixed-size cell. The parent always advances by `width` even if a label
   /// wants more space — otherwise long Chain/Wallet/Spender names shove later
   /// columns to the right.
   fn row_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
      let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
      let mut child =
         ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
      // Don't clip — Button chrome expands a few px and clip cuts the left
      // edge of Revoke. Truncate wrap keeps long labels inside the cell.
      child.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
      add_contents(&mut child);
   }

   /// The Amount column: how much is approved.
   ///
   /// For an NFT that means the *scope* — which id, or the whole collection — plus the allowance on
   /// the one shape that has one (ERC-5216), since "approved: yes" is not a number.
   fn amount_cell(
      ui: &mut Ui,
      width: f32,
      height: f32,
      asset: &ApprovalAsset,
      expire: Option<String>,
      theme: &Theme,
   ) {
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

      Self::row_cell(ui, width, height, |ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.xs;

         ui.label(RichText::new(text).size(theme.typography.normal).color(theme.colors.text));

         if let Some(text) = expire {
            let expire_text = RichText::new(text).size(theme.typography.normal);
            let q_mark = RichText::new("?").size(theme.typography.normal);
            let info_tip = Badge::new(q_mark, BadgeTone::Info);
            ui.add(info_tip).on_hover_text(expire_text);
         }
      });
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

                     if ui.add(label).clicked() {
                        if self.selected_wallet.is_some() {
                           self.selected_wallet = None;
                           self.current_page = 0;
                        }
                     }

                     for wallet in wallets {
                        let text =
                           RichText::new(&wallet.name_with_source()).size(theme.typography.normal);
                        let label = Label::new(text, None)
                           .visuals(label_visuals)
                           .interactive(true)
                           .sense(Sense::click())
                           .fill_width(true)
                           .expand(expansion);

                        if ui.add(label).clicked() {
                           if self.selected_wallet.as_ref().map(|w| w.address)
                              != Some(wallet.address)
                           {
                              self.selected_wallet = Some(wallet.clone());
                              self.current_page = 0;
                           }
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

                     if ui.add(label).clicked() {
                        if self.selected_chain.is_some() {
                           self.selected_chain = None;
                           self.current_page = 0;
                        }
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

                        if ui.add(label).clicked() {
                           if self.selected_chain != Some(chain) {
                              self.selected_chain = Some(chain);
                              self.current_page = 0;
                           }
                        }
                     }
                  });
            });

            ui.separator();

            if self.loading {
               ui.vertical_centered(|ui| {
                  ui.add(Spinner::new().size(20.0).color(theme.colors.text));
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

                  let q_mark = RichText::new("?").size(theme.typography.normal);
                  let info_tip = Badge::new(q_mark, BadgeTone::Info);
                  ui.add(info_tip).on_hover_text(ZEUS_TIP);
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

               let q_mark = RichText::new("?").size(theme.typography.normal);
               let info_tip = Badge::new(q_mark, BadgeTone::Info);
               ui.add(info_tip).on_hover_text(ZEUS_TIP);
            });

            ui.add_space(10.0);

            let label_visuals = theme.label_visuals();
            let tint = theme.image_tint_recommended;

            ScrollArea::vertical()
               .id_salt("approvals_scroll_area")
               .auto_shrink([false; 2])
               .max_height(ui.available_height() * 0.9)
               .show(ui, |ui| {
                  ui.set_width(ui.available_width());

                  // Fixed content height for every column.
                  // Size columns from the *inner* card width (after frame2
                  // padding) so header cells line up with body cells and the
                  // row actually fills the card — leftover used to live after
                  // Revoke because body spacing/padding did not match the header.
                  let row_height = 40.0;
                  let col_spacing = 20.0;
                  let n_cols = 7.0;
                  let row_frame = theme.frame1.outer_margin(Margin::ZERO);
                  let inner_left = row_frame.inner_margin.leftf();
                  let inner_right = row_frame.inner_margin.rightf();
                  let inner_y = row_frame.inner_margin.topf() + row_frame.inner_margin.bottomf();
                  let row_width = ui.available_width();
                  let inner_width = (row_width - inner_left - inner_right).max(0.0);
                  let usable = (inner_width - col_spacing * (n_cols - 1.0)).max(0.0);
                  // Compact action column; remaining width goes to the data columns.
                  let revoke_w = 100.0_f32.min(usable);
                  let rest = (usable - revoke_w).max(0.0);
                  let column_widths = [
                     rest * 0.20, // Asset
                     rest * 0.13, // Chain
                     rest * 0.16, // Wallet
                     rest * 0.22, // Spender
                     rest * 0.16, // Amount (+ expire)
                     rest * 0.13, // Type
                     revoke_w,    // Revoke
                  ];

                  // --- Header (same widths + left inset as body cells) ---
                  ui.horizontal(|ui| {
                     ui.add_space((ui.available_width() - row_width).max(0.0) / 2.0 + inner_left);
                     ui.spacing_mut().item_spacing.x = col_spacing;
                     for (i, header) in
                        ["Asset", "Chain", "Wallet", "Spender", "Amount", "Type", ""]
                           .into_iter()
                           .enumerate()
                     {
                        // Shorter header row — no need for full body height.
                        Self::row_cell(ui, column_widths[i], 28.0, |ui| {
                           if !header.is_empty() {
                              ui.label(
                                 RichText::new(header)
                                    .strong()
                                    .size(theme.typography.large)
                                    .color(theme.colors.text),
                              );
                           }
                        });
                     }
                  });

                  ui.add_space(8.0);

                  // --- Body: one frame2 card per approval ---
                  // Do NOT put Frame inside a Grid cell — Frame becomes a single
                  // cell and every column collapses into the first one.
                  let start = self.current_page * self.rows_per_page;
                  let end = start.saturating_add(self.rows_per_page).min(total_rows);
                  let rows = if start < end {
                     self.cached_rows[start..end].to_vec()
                  } else {
                     Vec::new()
                  };

                  ui.vertical_centered(|ui| {
                     ui.spacing_mut().item_spacing.y = theme.spacing.md;

                     for row in rows {
                        ui.allocate_ui(vec2(row_width, row_height + inner_y), |ui| {
                           row_frame.show(ui, |ui| {
                              ui.set_width(inner_width);
                              ui.spacing_mut().item_spacing.x = col_spacing;

                              ui.horizontal(|ui| {
                                 // Asset — a token's symbol, or the collection an NFT approval is over.
                                 Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                    let (icon, title, hover) = match &row.asset {
                                       ApprovalAsset::Token { currency, .. } => (
                                          icons.currency_icon_x32(currency, tint),
                                          currency.symbol().to_string(),
                                          currency.name().to_string(),
                                       ),
                                       ApprovalAsset::Nft {
                                          collection,
                                          token_id,
                                          ..
                                       } => {
                                          // A collection-wide approval has no id, so any of the
                                          // collection's cached art is what identifies it — asking
                                          // for a fixed id usually lands on art that was never
                                          // fetched and shows the placeholder instead.
                                          let icon = match token_id {
                                             Some(id) => icons.nft_icon_x64(
                                                row.chain,
                                                *collection,
                                                *id,
                                                tint,
                                             ),
                                             None => icons.nft_collection_icon_x64(
                                                row.chain,
                                                *collection,
                                                tint,
                                             ),
                                          };
                                          let name =
                                             self.collection_label(ctx, row.chain, *collection);
                                          let hover = format!("{}\n{}", name, collection);
                                          (icon, name, hover)
                                       }
                                    };

                                    ui.add(icon);
                                    let text = RichText::new(title)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label =
                                       Label::new(text, None).wrap().visuals(label_visuals);
                                    ui.scope(|ui| {
                                       ui.set_max_width(column_widths[0] - 40.0);
                                       ui.add(label).on_hover_text(hover);
                                    });
                                 });

                                 // Chain
                                 Self::row_cell(ui, column_widths[1], row_height, |ui| {
                                    let chain: ChainId = row.chain.into();
                                    let text = RichText::new(chain.name())
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label = Label::new(text, None)
                                       .wrap_mode(TextWrapMode::Truncate)
                                       .visuals(label_visuals);
                                    ui.add(label).on_hover_text(chain.name());
                                 });

                                 // Wallet
                                 Self::row_cell(ui, column_widths[2], row_height, |ui| {
                                    let name = self.wallet_name(ctx, row.owner);
                                    let text = RichText::new(&name)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label = Label::new(text, None)
                                       .wrap_mode(TextWrapMode::Truncate)
                                       .visuals(label_visuals);
                                    ui.add(label).on_hover_text(format!("{}\n{}", name, row.owner));
                                 });

                                 // Spender
                                 Self::row_cell(ui, column_widths[3], row_height, |ui| {
                                    let name = self.spender_label(ctx, row.chain, row.spender);

                                    let chain = ChainId::from(row.chain);
                                    let explorer = chain.block_explorer();
                                    let link =
                                       format!("{}/address/{}", explorer, row.spender.to_string());
                                    let text = RichText::new(&name)
                                       .size(theme.typography.normal)
                                       .color(theme.colors.info);
                                    ui.hyperlink_to(text, link);
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

                                 Self::amount_cell(
                                    ui,
                                    column_widths[4],
                                    row_height,
                                    &row.asset,
                                    expire,
                                    theme,
                                 );

                                 // Type
                                 Self::row_cell(ui, column_widths[5], row_height, |ui| {
                                    ui.label(
                                       RichText::new(row.kind.type_label())
                                          .size(theme.typography.normal)
                                          .color(theme.colors.text),
                                    );
                                 });

                                 // Revoke — hug the right inner edge of the card.
                                 Self::row_cell(ui, column_widths[6], row_height, |ui| {
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                       let text =
                                          RichText::new("Revoke").size(theme.typography.normal);
                                       let button = Button::new(text).visuals(button_visuals);
                                       if ui.add(button).clicked() {
                                          self.revoke(row);
                                       }
                                    });
                                 });
                              });
                           });
                        });
                     }
                  });
               });
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
            RT.spawn(async move {
               if let Err(e) = revoke_erc20_approval(chain, token, owner, spender).await {
                  tracing::error!("Failed to revoke ERC20 approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.invalidate_cache();
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
            RT.spawn(async move {
               if let Err(e) = revoke_permit2_approval(chain, owner, token, spender).await {
                  tracing::error!("Failed to revoke Permit2 approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.invalidate_cache();
                     gui.request_repaint();
                  });
               }
            });
         }
         ApprovalKind::Nft(params) => {
            let chain = params.chain;
            RT.spawn(async move {
               if let Err(e) = revoke_nft_approval(chain, params).await {
                  tracing::error!("Failed to revoke NFT approval: {:?}", e);
                  SHARED_GUI.write(|gui| {
                     gui.loading_window.reset();
                     gui.notification.reset();
                     gui.msg_window.open(format!("Revoke Failed: {}", e));
                     gui.request_repaint();
                  });
               } else {
                  SHARED_GUI.write(|gui| {
                     gui.approvals.invalidate_cache();
                     gui.request_repaint();
                  });
               }
            });
         }
      }
   }
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

   let client = ctx.get_zeus_client();
   let mut out = HashMap::new();

   for chunk in pairs.chunks(ALLOWANCE_PAIR_BATCH) {
      let chunk = chunk.to_vec();
      match client
         .request(chain, |client| {
            let chunk = chunk.clone();
            async move { batch::get_permit2_allowances(client, permit2, owner, chunk, None).await }
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

/// The calldata that revokes an NFT approval, with the params as they will read once it lands.
///
/// There is no generic "revoke": each of the three shapes is cleared a different way, and they are
/// **not** interchangeable. `setApprovalForAll(operator, false)` against a grant that was made with
/// per-token `approve` revokes nothing at all — the transaction succeeds, the row stays, and the user
/// has paid gas to believe they un-approved someone. So every shape is matched explicitly, and the
/// combinations the three shapes never emit are refused rather than guessed at.
///
/// The returned params are the *revoked* ones on purpose: they are what the manager records, and
/// recording the params as they were would leave the grant looking active after its revocation.
fn revoke_call(params: &NftApproveParams) -> Result<(Bytes, NftApproveParams), anyhow::Error> {
   let mut revoked = params.clone();

   let calldata = match (params.token_id, params.approved, params.amount) {
      // ERC-721 per-token: clear the operator to the zero address.
      (Some(id), None, None) => {
         revoked.operator = Address::ZERO;
         erc721::encode_approve(Address::ZERO, id)
      }
      // `ApprovalForAll`: unset the flag. The operator stays, which is what lets the store find and
      // drop the very row this revokes — a collection-wide entry is keyed by its operator.
      (None, Some(_), None) => {
         revoked.approved = Some(false);
         erc721::encode_set_approval_for_all(params.operator, false)
      }
      // ERC-5216: a zero allowance for this operator and this id.
      (Some(id), None, Some(_)) => {
         revoked.amount = Some(U256::ZERO);
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

   Ok((calldata, revoked))
}

/// Revoke an NFT approval on the collection that emitted it.
async fn revoke_nft_approval(chain_id: u64, params: NftApproveParams) -> Result<(), anyhow::Error> {
   let (calldata, revoked) = revoke_call(&params)?;

   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
      gui.ctx.clone()
   });
   let chain: ChainId = chain_id.into();

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
   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
      gui.request_repaint();
      gui.ctx.clone()
   });
   let chain: ChainId = chain_id.into();

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
   let ctx = SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing Permit2 revoke");
      gui.request_repaint();
      gui.ctx.clone()
   });

   let chain: ChainId = chain_id.into();
   let token_addr = token.address();

   let permit2 = address_book::permit2_contract(chain_id)?;
   let client = ctx.get_zeus_client();

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
      chain_id.into(),
      Some(msg),
      None,
      Some(owner),
   )
   .await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Wait while magic happens");
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

   /// An ERC-721 per-token grant is revoked by approving the zero address for that id.
   #[test]
   fn an_erc721_grant_is_revoked_with_approve_zero() {
      let (calldata, revoked) = revoke_call(&erc721_grant()).unwrap();

      assert_eq!(
         calldata,
         erc721::encode_approve(Address::ZERO, U256::from(ID))
      );
      assert_eq!(
         revoked.operator,
         Address::ZERO,
         "the operator is cleared"
      );
      assert_eq!(revoked.token_id, Some(U256::from(ID)));
      assert!(revoked.is_revoke());
   }

   /// `ApprovalForAll` is revoked by unsetting the flag — **not** by `approve(0, id)`, which would
   /// revoke one token of a collection-wide grant and leave the rest of it live.
   #[test]
   fn an_approval_for_all_is_revoked_with_the_flag() {
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
   }

   /// ERC-5216 is revoked with a zero allowance for that operator and id.
   #[test]
   fn an_erc5216_allowance_is_revoked_with_zero_amount() {
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
   }

   /// No shape emits a log that cannot be revoked, so an unrecognized combination is an error rather
   /// than a guessed calldata: a wrong revoke sends a real transaction that revokes something else.
   #[test]
   fn an_unrepresentable_shape_is_refused() {
      let mut params = erc721_grant();
      params.token_id = None;

      assert!(revoke_call(&params).is_err());
   }
}
