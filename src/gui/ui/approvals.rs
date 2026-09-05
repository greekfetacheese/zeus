//! UI for viewing and revoking ERC20 / Permit2 token approvals.

use crate::assets::icons::Icons;
use crate::core::{
   PermitParams, TokenApproveParams, WalletInfo, ZeusContext, send_transaction, signature,
};
use crate::gui::{SHARED_GUI, ui::show_with_fade};
use crate::utils::{RT, TimeStamp, truncate_address};
use egui::{
   Align, Frame, Layout, Margin, RichText, ScrollArea, Sense, Spinner, TextWrapMode, Ui, UiBuilder,
   vec2,
};
use egui_elements::{Button, ComboBox, Label, OverlayManager, Theme};
use elegance::{Badge, BadgeTone};
use std::collections::HashMap;
use std::sync::Arc;
use zeus_eth::{
   abi::permit::{allowance, encode_permit_single_call},
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   types::ChainId,
   utils::{NumericValue, address_book},
};

const ZEUS_TIP: &str = "Zeus only shows approvals that have been been made in-app.\n
It cannot track approvals made from other wallets.";

const DEFAULT_ROWS_PER_PAGE: usize = 10;

#[derive(Debug, Clone)]
enum ApprovalKind {
   Erc20(TokenApproveParams),
   Permit2(PermitParams),
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
}

#[derive(Debug, Clone)]
struct ApprovalRow {
   chain: u64,
   owner: Address,
   token: Currency,
   spender: Address,
   amount: NumericValue,
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

/// `(chain, owner, token, spender)`.
type PermitInfoMap = HashMap<(u64, Address, Address, Address), signature::Permit2Info>;

pub struct ApprovalsUi {
   open: bool,
   loading: bool,
   _overlay: OverlayManager,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   cached_rows: Vec<ApprovalRow>,
   cache_key: CacheKey,
   cached_permit_info: PermitInfoMap,
   current_page: usize,
   rows_per_page: usize,
}

impl ApprovalsUi {
   pub fn new(_overlay: OverlayManager) -> Self {
      Self {
         open: false,
         loading: false,
         _overlay,
         selected_wallet: None,
         selected_chain: None,
         cached_rows: Vec::new(),
         cache_key: CacheKey::default(),
         cached_permit_info: HashMap::new(),
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
      self.cached_permit_info = HashMap::new();
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
      let mut permit_info_cache = self.cached_permit_info.clone();

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
               token: Currency::from(params.token.clone()),
               spender: params.spender,
               amount: params.amount.clone(),
               kind: ApprovalKind::Erc20(params),
            });
         }

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

            // We actually need to call the permit info here to check if the amount
            // has been already spent

            let key = (
               params.chain,
               params.owner,
               params.token.address(),
               params.spender,
            );

            // Check cache first, if empty fetch from rpc
            let info_opt = permit_info_cache.get(&key).cloned();

            // TODO: Update the StateView contract so we can do batch calls here

            let permit_info = if let Some(info) = info_opt {
               Some(info)
            } else {
               let info_res = signature::Permit2Info::new(
                  ctx.clone(),
                  params.chain,
                  &params.token.to_erc20(),
                  params.amount.wei(),
                  params.owner,
                  params.spender,
               )
               .await;

               if let Ok(info) = info_res {
                  permit_info_cache.insert(key, info.clone());
                  Some(info)
               } else {
                  None
               }
            };

            if let Some(info) = permit_info {
               // If it doesnt need a new signature it means the permit is still valid
               if !info.needs_new_signature {
                  rows.push(ApprovalRow {
                     chain: params.chain,
                     owner: params.owner,
                     token: params.token.clone(),
                     spender: params.spender,
                     amount: params.amount.clone(),
                     kind: ApprovalKind::Permit2(params),
                  });
               }
            } else {
               rows.push(ApprovalRow {
                  chain: params.chain,
                  owner: params.owner,
                  token: params.token.clone(),
                  spender: params.spender,
                  amount: params.amount.clone(),
                  kind: ApprovalKind::Permit2(params),
               });
            }
         }

         // Token symbol, then spender — stable enough for browsing.
         rows.sort_by(|a, b| {
            a.token
               .symbol()
               .cmp(&b.token.symbol())
               .then(a.spender.cmp(&b.spender))
               .then(a.chain.cmp(&b.chain))
         });

         SHARED_GUI.write(|gui| {
            if gui.approvals.cache_key == key {
               gui.approvals.cached_rows = rows;
            }
            gui.approvals.loading = false;
            gui.approvals.cached_permit_info = permit_info_cache;
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

   fn amount_label(amount: &NumericValue) -> String {
      // ERC20 unlimited is U256::MAX; Permit2 amounts are uint160.
      let wei = amount.wei();
      let u160_max = (U256::from(1u8) << 160) - U256::from(1u8);
      if wei == U256::MAX || wei >= u160_max {
         "Unlimited".to_string()
      } else {
         amount.abbreviated()
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

   fn amount_cell(
      ui: &mut Ui,
      width: f32,
      height: f32,
      amount: &NumericValue,
      expire: Option<String>,
      theme: &Theme,
   ) {
      Self::row_cell(ui, width, height, |ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.xs;

         ui.label(
            RichText::new(Self::amount_label(amount))
               .size(theme.typography.normal)
               .color(theme.colors.text),
         );

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
               let wallets = ctx.get_all_wallets_info();
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

                     for (_, wallet) in wallets {
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
                  let row_height = 48.0;
                  let col_spacing = 20.0;
                  let n_cols = 7.0;
                  let row_frame = theme.frame2.outer_margin(Margin::ZERO);
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
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     for row in rows {
                        ui.allocate_ui(vec2(row_width, row_height + inner_y), |ui| {
                           row_frame.show(ui, |ui| {
                              ui.set_width(inner_width);
                              ui.spacing_mut().item_spacing.x = col_spacing;

                              ui.horizontal(|ui| {
                                 // Asset
                                 Self::row_cell(ui, column_widths[0], row_height, |ui| {
                                    let icon = icons.currency_icon_x32(&row.token, tint);
                                    ui.add(icon);
                                    let text = RichText::new(row.token.symbol())
                                       .size(theme.typography.normal)
                                       .color(theme.colors.text);
                                    let label =
                                       Label::new(text, None).wrap().visuals(label_visuals);
                                    ui.scope(|ui| {
                                       ui.set_max_width(column_widths[0] - 40.0);
                                       ui.add(label).on_hover_text(row.token.name());
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
                                    &row.amount,
                                    expire,
                                    theme,
                                 );

                                 // Type
                                 Self::row_cell(ui, column_widths[5], row_height, |ui| {
                                    let kind = match &row.kind {
                                       ApprovalKind::Erc20(_) => "ERC-20",
                                       ApprovalKind::Permit2(_) => "Permit2",
                                    };
                                    ui.label(
                                       RichText::new(kind)
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
      }
   }
}

async fn revoke_erc20_approval(
   chain_id: u64,
   token: ERC20Token,
   from: Address,
   spender: Address,
) -> Result<(), anyhow::Error> {
   let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
   let chain: ChainId = chain_id.into();

   let calldata = token.encode_approve(spender, U256::ZERO);
   let value = U256::ZERO;
   let dapp = "".to_string();
   let mev_protect = false;
   let auth_list = vec![];
   let interact_to = token.address;

   let (_, _) = send_transaction(
      ctx,
      dapp,
      None,
      chain,
      mev_protect,
      from,
      interact_to,
      calldata,
      value,
      auth_list,
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

   let (_, _) = send_transaction(
      ctx,
      "".to_string(),
      None,
      chain,
      false,
      owner,
      permit2,
      calldata,
      U256::ZERO,
      vec![],
   )
   .await?;

   Ok(())
}
