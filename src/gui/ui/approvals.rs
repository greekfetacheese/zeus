//! UI for viewing and revoking ERC20 / Permit2 token approvals.

use crate::assets::icons::Icons;
use crate::core::{
   PermitParams, TokenApproveParams, WalletInfo, ZeusContext, send_transaction, signature,
};
use crate::gui::SHARED_GUI;
use crate::utils::{RT, truncate_address};
use egui::{Align, Frame, Grid, Layout, Margin, RichText, ScrollArea, Sense, Spinner, Ui, vec2};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use zeus_eth::{
   abi::permit::{allowance, encode_permit_single_call},
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token},
   types::ChainId,
   utils::{NumericValue, address_book},
};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::{Button, ComboBox, Label};

#[derive(Debug, Clone)]
enum ApprovalKind {
   Erc20(TokenApproveParams),
   Permit2(PermitParams),
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

pub struct ApprovalsUi {
   open: bool,
   loading: bool,
   _overlay: OverlayManager,
   selected_wallet: Option<WalletInfo>,
   selected_chain: Option<ChainId>,
   cached_rows: Vec<ApprovalRow>,
   cache_key: CacheKey,
   size: (f32, f32),
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
         size: (980.0, 620.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self) {
      if self.open {
         return;
      }

      // self.overlay.window_opened();
      self.open = true;
      self.cached_rows.clear();
      self.cache_key = CacheKey::invalid();
   }

   pub fn close(&mut self) {
      if !self.open && self.cached_rows.is_empty() {
         return;
      }

      // self.overlay.window_closed();
      self.open = false;
      self.selected_wallet = None;
      self.selected_chain = None;
      self.cached_rows = Vec::new();
      self.cache_key = CacheKey::default();
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

            let permit_info = signature::Permit2Info::new(
               ctx.clone(),
               params.chain,
               &params.token.to_erc20(),
               params.amount.wei(),
               params.owner,
               params.spender,
            )
            .await;

            if let Ok(info) = permit_info {
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
            gui.request_repaint();
         });
      });
   }

   /// Force a cache rebuild after a successful revoke.
   fn invalidate_cache(&mut self) {
      self.cached_rows.clear();
      self.cache_key = CacheKey::invalid();
   }

   fn wallet_name(&self, ctx: &mut ZeusContext, address: Address) -> String {
      ctx.get_wallet_name(address)
         .unwrap_or_else(|| truncate_address(address.to_string()))
   }

   fn spender_label(&self, ctx: &mut ZeusContext, chain: u64, spender: Address) -> String {
      ctx.get_address_name(chain, spender)
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

   /// Fixed-size grid cell with vertically centered content so icon, text,
   /// and button columns share the same baseline on striped rows.
   fn grid_cell(ui: &mut Ui, width: f32, height: f32, add_contents: impl FnOnce(&mut Ui)) {
      ui.allocate_ui_with_layout(
         vec2(width, height),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            add_contents(ui);
         },
      );
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      self.rebuild_cache();

      Frame::new().inner_margin(Margin::same(10)).show(ui, |ui| {
         ui.set_width(self.size.0);
         ui.set_height(self.size.1);
         ui.spacing_mut().item_spacing = vec2(10.0, 12.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.x = 20.0;

            let combo_visuals = theme.combo_box_visuals();
            let label_visuals = theme.label_visuals();
            let expansion = Some(6.0);

            // Wallet filter
            let wallets = ctx.get_all_wallets_info();
            let selected_wallet_name =
               self.selected_wallet.clone().map_or("All Wallets".to_string(), |wallet| {
                  wallet.name_with_source()
               });

            let text = RichText::new(selected_wallet_name).size(theme.text_sizes.normal);
            let label = Label::new(text, None)
               .visuals(label_visuals)
               .fill_width(true)
               .sense(Sense::click())
               .expand(expansion);

            ComboBox::new("approvals_wallet_filter", label)
               .visuals(combo_visuals)
               .width(200.0)
               .show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  let text = RichText::new("All Wallets").size(theme.text_sizes.normal);
                  let label = Label::new(text, None)
                     .visuals(label_visuals)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() {
                     if self.selected_wallet.is_some() {
                        self.selected_wallet = None;
                     }
                  }

                  for (_, wallet) in wallets {
                     let text =
                        RichText::new(&wallet.name_with_source()).size(theme.text_sizes.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .sense(Sense::click())
                        .fill_width(true)
                        .expand(expansion);

                     if ui.add(label).clicked() {
                        if self.selected_wallet.as_ref().map(|w| w.address) != Some(wallet.address)
                        {
                           self.selected_wallet = Some(wallet.clone());
                        }
                     }
                  }
               });

            // Chain filter
            let selected_chain_name =
               self.selected_chain.map_or("All Chains".to_string(), |chain| {
                  chain.name().to_string()
               });

            let text = RichText::new(selected_chain_name).size(theme.text_sizes.normal);
            let label = Label::new(text, None)
               .visuals(label_visuals)
               .fill_width(true)
               .sense(Sense::click())
               .expand(expansion);

            ComboBox::new("approvals_chain_filter", label)
               .visuals(combo_visuals)
               .width(200.0)
               .show_ui(ui, |ui| {
                  ui.spacing_mut().item_spacing.y = 10.0;

                  let text = RichText::new("All Chains").size(theme.text_sizes.normal);
                  let label = Label::new(text, None)
                     .visuals(label_visuals)
                     .fill_width(true)
                     .sense(Sense::click())
                     .expand(expansion);

                  if ui.add(label).clicked() {
                     if self.selected_chain.is_some() {
                        self.selected_chain = None;
                     }
                  }

                  for chain in ChainId::supported_chains() {
                     if ctx.is_chain_disabled(chain.id()) {
                        continue;
                     }

                     let text = RichText::new(chain.name()).size(theme.text_sizes.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .sense(Sense::click())
                        .fill_width(true)
                        .expand(expansion);

                     if ui.add(label).clicked() {
                        if self.selected_chain != Some(chain) {
                           self.selected_chain = Some(chain);
                        }
                     }
                  }
               });
         });

         ui.separator();

         // Rebuild after filter widgets may have changed selection
         self.rebuild_cache();

         if self.loading {
            ui.vertical_centered(|ui| {
               ui.add(Spinner::new().size(20.0).color(theme.colors.text));
            });
            return;
         }

         if self.cached_rows.is_empty() {
            ui.vertical_centered(|ui| {
               ui.add_space(40.0);
               ui.label(
                  RichText::new("No active approvals match your filters.")
                     .size(theme.text_sizes.large)
                     .color(theme.colors.text),
               );
            });
            return;
         }

         ui.vertical_centered(|ui| {
            ui.label(
               RichText::new(format!(
                  "{} active approval(s)",
                  self.cached_rows.len()
               ))
               .size(theme.text_sizes.large)
               .color(theme.colors.text),
            );
         });

         ui.add_space(10.0);

         let button_visuals = theme.button_visuals();
         let label_visuals = theme.label_visuals();
         let tint = theme.image_tint_recommended;

         ScrollArea::vertical()
            .id_salt("approvals_scroll_area")
            .auto_shrink([false; 2])
            .max_height(ui.available_height() * 0.9)
            .show(ui, |ui| {
               ui.set_width(ui.available_width());

               // Fixed cell height so icon / text / button rows share one baseline.
               // Icon is 32px; leave a little padding for the revoke button.
               let row_height = 40.0;
               let column_widths = [
                  ui.available_width() * 0.18, // Asset
                  ui.available_width() * 0.10, // Chain
                  ui.available_width() * 0.14, // Wallet
                  ui.available_width() * 0.18, // Spender
                  ui.available_width() * 0.12, // Amount
                  ui.available_width() * 0.10, // Type
                  ui.available_width() * 0.12, // Revoke
               ];

               ui.horizontal(|ui| {
                  ui.add_space((ui.available_width() - column_widths.iter().sum::<f32>()) / 2.0);

                  Grid::new("approvals_grid")
                     .spacing([20.0, 14.0])
                     .num_columns(7)
                     .min_col_width(0.0)
                     .striped(true)
                     .show(ui, |ui| {
                        // Headers use the same fixed widths as body cells so
                        // columns stay locked under their titles.
                        for (i, header) in
                           ["Asset", "Chain", "Wallet", "Spender", "Amount", "Type", ""]
                              .into_iter()
                              .enumerate()
                        {
                           Self::grid_cell(ui, column_widths[i], row_height, |ui| {
                              if !header.is_empty() {
                                 ui.label(
                                    RichText::new(header)
                                       .strong()
                                       .size(theme.text_sizes.large)
                                       .color(theme.colors.text),
                                 );
                              }
                           });
                        }
                        ui.end_row();

                        // Clone rows so revoke handlers can take owned data without
                        // holding a borrow on self.cached_rows across &mut self use.
                        let rows = self.cached_rows.clone();

                        for row in rows {
                           // Asset
                           Self::grid_cell(ui, column_widths[0], row_height, |ui| {
                              let icon = icons.currency_icon_x32(&row.token, tint);
                              ui.add(icon);
                              let text = RichText::new(row.token.symbol())
                                 .size(theme.text_sizes.normal)
                                 .color(theme.colors.text);
                              let label = Label::new(text, None)
                                 .wrap()
                                 .visuals(label_visuals)
                                 .interactive(false);
                              ui.scope(|ui| {
                                 ui.set_max_width(column_widths[0] - 40.0);
                                 ui.add(label).on_hover_text(row.token.name());
                              });
                           });

                           // Chain
                           Self::grid_cell(ui, column_widths[1], row_height, |ui| {
                              let chain: ChainId = row.chain.into();
                              ui.label(
                                 RichText::new(chain.name())
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              );
                           });

                           // Wallet
                           Self::grid_cell(ui, column_widths[2], row_height, |ui| {
                              let name = self.wallet_name(ctx, row.owner);
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              )
                              .on_hover_text(row.owner.to_string());
                           });

                           // Spender
                           Self::grid_cell(ui, column_widths[3], row_height, |ui| {
                              let name = self.spender_label(ctx, row.chain, row.spender);
                              ui.label(
                                 RichText::new(name)
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              )
                              .on_hover_text(row.spender.to_string());
                           });

                           // Amount
                           Self::grid_cell(ui, column_widths[4], row_height, |ui| {
                              ui.label(
                                 RichText::new(Self::amount_label(&row.amount))
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              );
                           });

                           // Type
                           Self::grid_cell(ui, column_widths[5], row_height, |ui| {
                              let kind = match &row.kind {
                                 ApprovalKind::Erc20(_) => "ERC-20",
                                 ApprovalKind::Permit2(_) => "Permit2",
                              };
                              ui.label(
                                 RichText::new(kind)
                                    .size(theme.text_sizes.normal)
                                    .color(theme.colors.text),
                              );
                           });

                           // Revoke
                           Self::grid_cell(ui, column_widths[6], row_height, |ui| {
                              let text = RichText::new("Revoke").size(theme.text_sizes.normal);
                              let button = Button::new(text).visuals(button_visuals);
                              if ui.add(button).clicked() {
                                 self.revoke(row);
                              }
                           });

                           ui.end_row();
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
   let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
   let chain: ChainId = chain_id.into();
   let token_addr = token.address();

   let permit2 = address_book::permit2_contract(chain_id)?;
   let client = ctx.get_zeus_client();

   SHARED_GUI.write(|gui| {
      gui.loading_window.open("Preparing Permit2 revoke");
      gui.request_repaint();
   });

   let allowance_data = client
      .request(chain_id, |client| async move {
         allowance(client, permit2, owner, token_addr, spender).await
      })
      .await?;

   let current_time = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
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

   let msg_type = crate::core::SignMsgType::new(ctx.clone(), chain_id, Some(msg), None).await?;

   SHARED_GUI.write(|gui| {
      gui.loading_window.reset();
      ctx.write(|ctx| {
         gui.sign_msg_window.open(ctx, "".to_string(), chain_id, msg_type.clone());
      });
      gui.request_repaint();
   });

   // Wait for the user to sign or cancel
   let mut signed = None;
   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;
      SHARED_GUI.read(|gui| {
         signed = gui.sign_msg_window.is_signed();
      });
      if signed.is_some() {
         SHARED_GUI.write(|gui| {
            ctx.write(|ctx| {
               gui.sign_msg_window.close(ctx);
            });
         });
         break;
      }
   }

   let signed = signed.unwrap();
   if !signed {
      SHARED_GUI.request_repaint();
      return Err(anyhow::anyhow!(
         "You cancelled the signing process"
      ));
   }

   let wallet = ctx
      .get_wallet(owner)
      .ok_or_else(|| anyhow::anyhow!("Wallet not found for approval owner"))?;
   let signature = msg_type.sign(&wallet.key).await?;

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
