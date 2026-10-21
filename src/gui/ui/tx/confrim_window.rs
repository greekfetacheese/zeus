use egui::{Align, CollapsingHeader, Id, Layout, Order, RichText, ScrollArea, Ui, vec2};
use egui_elements::{Button, Modal, Theme};

use super::{
   AVAIL_WIDTH_MARGIN, address, chain_chip, clear_display_ui, events::*, show_analysis_buttons,
   show_calldata_modal, show_state_changes_block, show_tx_diffs_modal, show_tx_options_modal,
   simulated_block, tx_cost, value,
};
use crate::assets::icons::Icons;
use crate::core::clear_signing::{self, ClearDisplay};
use crate::core::{DecodedEvent, TransactionAnalysis, ZeusContext, ZeusCtx};
use crate::gui::SHARED_GUI;
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::common::{delayed_action_label, wallet_identity};
use crate::utils::{RT, estimate_tx_cost};
use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::NativeCurrency,
   types::ChainId,
   utils::NumericValue,
};

use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct TxConfirmationWindow {
   open: bool,
   /// A flow has claimed this window ([`Self::claim`]) and holds it until it is done
   /// with the prompt. Claimed under the same lock that checks [`Self::is_busy`], so
   /// a second flow cannot claim it while the first is still fetching the analysis
   /// for its prompt — the phase where no prompt can be shown yet.
   pending: bool,
   /// True if the tx is coming from Zeus
   source_is_zeus: bool,
   decoded_events: DecodedEvents,
   /// True to confirm, false to reject
   confirmed_or_rejected: Option<bool>,
   /// Bumped on every `open` and `close`. Late clear-signing tasks must not apply to a
   /// newer prompt, or re-open one that has been closed.
   open_generation: u64,
   dapp: String,
   chain: ChainId,
   native_currency: NativeCurrency,
   /// Tx to be confirmed and sent to the network
   tx: Option<TransactionAnalysis>,
   tx_main_event: Option<DecodedEvent>,
   /// Adjust priority fee
   priority_fee: String,
   mev_protect: bool,
   /// True if the tx is sponsored by another account
   sponsored: bool,
   /// The name the user picked for the recipient, with the address it was picked for: the main
   /// event's recipient row repeats it instead of a reverse-lookup name. `None` for a flow with
   /// no picked name.
   recipient_name: Option<(Address, String)>,
   gas_used: u64,
   /// Adjust gas limit
   gas_limit: u64,
   adjusted_gas_limit: String,
   tx_cost: NumericValue,
   tx_cost_usd: NumericValue,
   show_calldata: bool,
   show_diffs: bool,
   /// Priority fee / gas limit / MEV status
   show_tx_options: bool,
   clear_display: Option<ClearDisplay>,
   /// When the prompt became visible. `None` skips the Sign/Confirm delay (in-app Zeus txs).
   opened_at: Option<Instant>,
   size: (f32, f32),
}

impl TxConfirmationWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         pending: false,
         source_is_zeus: false,
         decoded_events: DecodedEvents::new(),
         confirmed_or_rejected: None,
         open_generation: 0,
         dapp: String::new(),
         chain: ChainId::default(),
         native_currency: NativeCurrency::default(),
         tx: None,
         tx_main_event: None,
         priority_fee: String::new(),
         mev_protect: false,
         sponsored: false,
         recipient_name: None,
         gas_used: 0,
         gas_limit: 0,
         adjusted_gas_limit: String::new(),
         tx_cost: NumericValue::default(),
         tx_cost_usd: NumericValue::default(),
         show_calldata: false,
         show_diffs: false,
         show_tx_options: false,
         clear_display: None,
         opened_at: None,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// True while a confirmation is in flight: a flow holds the window
   /// ([`Self::pending`]) or the prompt is up.
   ///
   /// `confirm_tx` refuses to raise a second prompt while this holds, so two sends
   /// can never share one window — and therefore one Confirm click.
   ///
   /// An answer still waiting in `confirmed_or_rejected` is deliberately *not* busy: the flow that
   /// owns the window holds the claim until it has collected the answer, so an uncollected answer
   /// can only exist once that flow is gone — and reading it as busy would let a flow that died
   /// after the user answered wedge every later send.
   pub fn is_busy(&self) -> bool {
      self.pending || self.open
   }

   /// Claim the window for one flow, from just before its prompt is built until it
   /// is done with it. Returns false when another flow already holds it.
   ///
   /// Call under the same lock as the [`Self::is_busy`] check (`confirm_tx` does),
   /// or two flows can both pass the check. [`Self::release`] hands it back.
   pub(crate) fn claim(&mut self) -> bool {
      if self.pending {
         return false;
      }
      self.pending = true;
      true
   }

   pub fn release(&mut self) {
      self.pending = false;
   }

   pub fn reset(&mut self) {
      self.close();
      *self = Self::new();
   }

   pub fn close(&mut self) {
      self.open = false;
      // Invalidate an in-flight preparation (`open_inner`): its write-back is guarded on
      // this generation, so a close — auto-lock's `reject()` included — drops it instead
      // of letting it re-open the window as a prompt with no flow behind it after unlock.
      self.open_generation = self.open_generation.wrapping_add(1);
   }

   /// Open this [TxConfirmationWindow]
   pub fn open(
      &mut self,
      ctx: ZeusCtx,
      source_is_zeus: bool,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
      recipient_name: Option<(Address, String)>,
   ) {
      self.open_inner(
         ctx,
         source_is_zeus,
         dapp,
         chain,
         tx,
         priority_fee,
         mev_protect,
         sponsored,
         recipient_name,
         None,
      );
   }

   /// Open with a pre-built ERC-7730 display (dev dummy; skips registry lookup).
   pub fn open_with_clear_display(
      &mut self,
      ctx: ZeusCtx,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
      display: ClearDisplay,
   ) {
      self.open_inner(
         ctx,
         false,
         dapp,
         chain,
         tx,
         priority_fee,
         mev_protect,
         sponsored,
         None,
         Some(display),
      );
   }

   fn open_inner(
      &mut self,
      ctx: ZeusCtx,
      source_is_zeus: bool,
      dapp: String,
      chain: ChainId,
      tx: TransactionAnalysis,
      priority_fee: String,
      mev_protect: bool,
      sponsored: bool,
      recipient_name: Option<(Address, String)>,
      prebuilt_display: Option<ClearDisplay>,
   ) {
      if self.is_busy() {
         return;
      }

      self.sponsored = sponsored;
      self.source_is_zeus = source_is_zeus;
      self.recipient_name = recipient_name;
      // `send_transaction` polls this immediately. Confirm/Reject only `close()`
      // the window, so the previous answer stays in this field and would be
      // treated as the new prompt (auto-confirm + spinner while this task
      // is still fetching a clear-signing descriptor).
      self.confirmed_or_rejected = None;
      self.open_generation = self.open_generation.wrapping_add(1);
      let generation = self.open_generation;

      if sponsored {
         self.tx_cost = NumericValue::default();
         self.tx_cost_usd = NumericValue::default();
      }

      RT.spawn(async move {
         let native = NativeCurrency::from(chain.id());
         let main_event = tx.infer_main_event(ctx.clone(), chain.id());
         let gas_used = tx.gas_used;
         let gas_limit = gas_used * 15 / 10;

         let clear_display = if prebuilt_display.is_some() {
            prebuilt_display
         } else if tx.contract_interact && tx.call_data.len() >= 4 {
            clear_signing::try_clear_sign_calldata(
               ctx.clone(),
               chain.id(),
               tx.sender,
               tx.interact_to,
               tx.value,
               &tx.call_data,
            )
            .await
         } else {
            None
         };

         SHARED_GUI.write(|gui| {
            if gui.tx_confirmation_window.open_generation != generation {
               return;
            }

            gui.tx_confirmation_window.dapp = dapp;
            gui.tx_confirmation_window.priority_fee = priority_fee;
            gui.tx_confirmation_window.mev_protect = mev_protect;
            gui.tx_confirmation_window.gas_used = gas_used;
            gui.tx_confirmation_window.gas_limit = gas_limit;
            gui.tx_confirmation_window.adjusted_gas_limit = gas_limit.to_string();
            gui.tx_confirmation_window.chain = chain;
            gui.tx_confirmation_window.native_currency = native;
            gui.tx_confirmation_window.tx = Some(tx);
            gui.tx_confirmation_window.tx_main_event = Some(main_event);
            gui.tx_confirmation_window.clear_display = clear_display;
            gui.tx_confirmation_window.open = true;
            gui.tx_confirmation_window.confirmed_or_rejected = None;
            gui.tx_confirmation_window.opened_at = if source_is_zeus {
               None
            } else {
               Some(Instant::now())
            };

            ctx.write(|ctx| {
               gui.tx_confirmation_window.calculate_tx_cost(ctx, gas_used);
            });
            gui.request_repaint();
         });
      });
   }

   /// Collect the answer, consuming it.
   ///
   /// Single-use on purpose: an answer that could be read twice would answer two
   /// flows from one Confirm click, and the second would broadcast a transaction
   /// the user never reviewed.
   pub fn take_confirmed_or_rejected(&mut self) -> Option<bool> {
      self.confirmed_or_rejected.take()
   }

   /// Close without confirming and record a rejection, so the awaiting
   /// `confirm_tx` errors out instead of hanging. Used when auto-lock fires.
   pub fn reject(&mut self) {
      self.confirmed_or_rejected = Some(false);
      self.close();
   }

   pub fn get_priority_fee(&self) -> NumericValue {
      NumericValue::parse_to_gwei(&self.priority_fee)
   }

   /// The gas limit the transaction is sent with.
   ///
   /// The window's "Gas Limit" field is an override: whatever the user typed wins, and the simulated
   /// limit is only the fallback for a field that is empty or unparseable. It can only ever add
   /// headroom — a value below what the simulation used is floored, since a limit under the
   /// requirement can only produce an out-of-gas revert.
   pub fn get_gas_limit(&self) -> u64 {
      self
         .adjusted_gas_limit
         .trim()
         .parse::<u64>()
         .unwrap_or(self.gas_limit)
         .max(self.gas_used)
   }

   pub fn get_clear_display(&self) -> Option<ClearDisplay> {
      self.clear_display.clone()
   }

   /// Calculate the cost of the transaction
   fn calculate_tx_cost(&mut self, ctx: &mut ZeusContext, gas_used: u64) {
      if self.sponsored {
         return;
      }

      let chain = self.chain;
      let fee = NumericValue::parse_to_gwei(&self.priority_fee);
      let fee = if fee.is_zero() && chain.supports_type_2_tx() {
         NumericValue::parse_to_gwei("1")
      } else {
         fee
      };

      let (cost_in_wei, cost_in_usd) = estimate_tx_cost(ctx, chain.id(), gas_used, fee.wei());
      self.tx_cost = cost_in_wei;
      self.tx_cost_usd = cost_in_usd;
   }

   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      appearance: &AppearanceSettings,
      ui: &mut Ui,
   ) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let id = Id::new("tx_confirmation_window");
      let frame = theme.window_frame.fill(theme.frame1.fill);
      let heading = self.heading(theme);

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(heading)
         .header_separator(false)
         .center_header(true)
         .closable(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            // A maximum, not a fixed height: `set_height` also sets the *minimum*, and
            // that is what left a band of empty space under the buttons of a short
            // window. The content decides the height now.
            ui.set_max_height(self.size.1);

            let button_visuals = theme.button_visuals();

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.tx.is_none() {
                  ui.label(
                     RichText::new("Transaction Analysis not found, this is a bug")
                        .size(theme.typography.large),
                  );
                  return;
               }

               let analysis = self.tx.as_ref().unwrap();
               let main_event = self.tx_main_event.as_ref().unwrap();

               let frame = theme.frame2;
               let frame_size = vec2(ui.available_width() * AVAIL_WIDTH_MARGIN, 45.0);

               // Decoded events window
               self.decoded_events.show(
                  ctx,
                  self.chain,
                  theme,
                  icons.clone(),
                  analysis,
                  frame_size,
                  frame,
                  self.size,
                  ui,
               );

               let calldata = analysis.call_data.to_string();
               // Read before `clear_display` borrows its field: this asks the whole window.
               let descriptor_leads = self.descriptor_leads();
               let clear_display = self.clear_display.as_ref();

               // Calldata window
               show_calldata_modal(
                  &mut self.show_calldata,
                  theme,
                  ctx,
                  self.chain,
                  icons.clone(),
                  clear_display,
                  calldata,
                  ui,
               );

               show_tx_diffs_modal(
                  &mut self.show_diffs,
                  theme,
                  ctx,
                  self.chain,
                  icons.clone(),
                  &analysis.balance_diff,
                  &analysis.approval_diff,
                  ui,
               );

               // Priority fee, gas limit and the MEV status live behind this modal: the
               // fields are set once in a while, and the height they used to hold is
               // worth more to what the transaction does.
               let fee_changed = show_tx_options_modal(
                  &mut self.show_tx_options,
                  theme,
                  self.chain,
                  &mut self.priority_fee,
                  &mut self.adjusted_gas_limit,
                  self.gas_used,
                  self.mev_protect,
                  (!main_event.is_other() && main_event.is_mev_vulnerable())
                     || main_event.is_other_or_eoa_delegate(),
                  ui,
               );

               // The wallet that signs this transaction, on the chain it goes out on
               ui.allocate_ui(frame_size, |ui| {
                  frame.show(ui, |ui| {
                     if !self.dapp.is_empty() {
                        ui.label(RichText::new(&self.dapp).size(theme.typography.large));
                        ui.separator();
                     }

                     // A dapp gets its own account, so the signing wallet is not
                     // necessarily the one selected in the sidebar: name it next to the
                     // origin that asked for it.
                     ui.horizontal(|ui| {
                        ui.add(wallet_identity(
                           ctx,
                           self.chain.id(),
                           analysis.sender,
                           theme,
                           icons.clone(),
                        ));

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                           chain_chip(self.chain, theme, icons.clone(), ui);
                        });
                     });
                  });
               });

               let completely_unknown = main_event.is_other() && self.clear_display.is_none();
               let has_more_than_one_diff =
                  analysis.balance_diff.len() > 1 || analysis.approval_diff.len() > 1;
               let show_extra_warning =
                  main_event.is_other_or_eoa_delegate() && has_more_than_one_diff;

               let msg = "Review the transaction details before proceeding";

               if show_extra_warning || completely_unknown {
                  ui.label(
                     RichText::new(msg).size(theme.typography.large).color(theme.colors.warning),
                  );
               }

               // A bare asset movement gives way to the descriptor, which says what the call was
               // for; the state change rows below still say what moved.
               // Main event details
               if !main_event.is_other() && !descriptor_leads {
                  // The name the user picked for the recipient; the event renderer matches it
                  // against the row's own address before showing it.
                  let recipient_name =
                     self.recipient_name.as_ref().map(|(address, name)| (*address, name.as_str()));

                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        show_event(
                           ctx,
                           self.chain,
                           theme,
                           icons.clone(),
                           main_event,
                           recipient_name,
                           ui,
                        );
                     });
                  });
               }

               // Clear display UI
               if main_event.is_other() || descriptor_leads {
                  let frame_size = vec2(ui.available_width() * AVAIL_WIDTH_MARGIN, 300.0);

                  if let Some(display) = self.clear_display.clone() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           ScrollArea::vertical()
                              .id_salt("clear_diplay_ui")
                              .max_height(300.0)
                              .show(ui, |ui| {
                                 clear_display_ui(
                                    ctx,
                                    self.chain,
                                    &display,
                                    theme,
                                    icons.clone(),
                                    ui,
                                 );
                              });
                        });
                     });
                  }
               }

               // Show the state changes, if empty it early returns
               // so it doesn't affect the UI in any way.
               show_state_changes_block(
                  ctx,
                  self.chain,
                  theme,
                  icons.clone(),
                  &analysis.balance_diff,
                  &analysis.approval_diff,
                  ui,
               );

               // Transaction cost, then the raw details behind a toggle.
               //
               // The cost stays outside the toggle: it is what the balance check above
               // is measured against, so it cannot be hidden by default. Chain and
               // sender are not repeated here — the identity line names both.
               ui.allocate_ui(frame_size, |ui| {
                  frame.show(ui, |ui| {
                     tx_cost(
                        self.chain,
                        &self.tx_cost,
                        &self.tx_cost_usd,
                        theme,
                        ui,
                     );

                     CollapsingHeader::new(
                        RichText::new("Transaction details").size(theme.typography.large),
                     )
                     .id_salt(("tx_details", self.open_generation))
                     .show(ui, |ui| {
                        // Contract interaction
                        if analysis.contract_interact {
                           let label = "Contract interaction";
                           address(
                              ctx,
                              self.chain,
                              label,
                              analysis.interact_to,
                              theme,
                              ui,
                           );
                        }

                        // Value to be sent
                        value(ctx, self.chain, analysis.value_sent(), theme, ui);

                        // The block the numbers above were read at
                        if let Some(block_number) = analysis.block_number {
                           simulated_block(self.chain, block_number, theme, ui);
                        }
                     });
                  });
               });

               /*
               // Show ETH received
               if !analysis.eth_received().is_zero()
                  && !analysis.is_unwrap_weth()
                  && !analysis.is_swap()
               {
                  let text = "You will receive";
                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        eth_received(
                           self.chain.id(),
                           analysis.eth_received(),
                           analysis.eth_received_usd(ctx),
                           theme,
                           icons.clone(),
                           text,
                           ui,
                        );
                     });
                  });
               }
               */

               // Decoded Events / Calldata / State / Tx Options
               let buttons_size = ui.available_width() * AVAIL_WIDTH_MARGIN;
               let buttons = show_analysis_buttons(analysis, theme, buttons_size, true, ui);

               if buttons.events {
                  self.decoded_events.open();
               }

               if buttons.calldata {
                  self.show_calldata = true;
               }

               if buttons.balance_and_approvals {
                  self.show_diffs = true;
               }

               if buttons.tx_options {
                  self.show_tx_options = true;
               }

               // Read before the cost is recalculated: `analysis` borrows `self.tx`, and
               // `calculate_tx_cost` needs the whole `self`.
               let sender = analysis.sender;
               let value_sent = analysis.value_sent().wei();

               if fee_changed {
                  self.calculate_tx_cost(ctx, self.gas_used);
               }

               let sufficient_balance = self.sufficient_balance(ctx, value_sent, sender);

               // The action row is a different decision from the row of inspectors above
               // it, so it gets a gap rather than just `item_spacing`.
               ui.add_space(theme.spacing.lg);

               // Buttons
               let size = vec2(ui.available_width() * AVAIL_WIDTH_MARGIN, 45.0);
               ui.allocate_ui(size, |ui| {
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.xl;

                     // leave room for the gap between the two buttons,
                     // otherwise the row overflows to the right
                     let button_size = vec2(
                        (ui.available_width() - theme.spacing.xl) * 0.5,
                        45.0,
                     );

                     // A wallet that cannot cover value + cost says so on the button it
                     // disables, rather than in a row of its own above the row.
                     let ready_label = self.confirm_label(sufficient_balance);
                     let (confirm_ready, confirm_label) =
                        delayed_action_label(self.opened_at, &ready_label);
                     if !confirm_ready {
                        ui.ctx().request_repaint_after(Duration::from_millis(100));
                     }
                     let text = RichText::new(confirm_label).size(theme.typography.large);
                     let confirm = Button::new(text).min_size(button_size).visuals(button_visuals);

                     // Copy the settings out first: the widget needs an owned
                     // value, and `HoldSettings` is `Copy`.
                     let hold = appearance.button_settings(theme.colors.accent);
                     let confirm = match &hold {
                        Some(settings) => confirm.hold_settings(settings),
                        None => confirm,
                     };

                     // The crate's hold reports a completed hold as a click and
                     // ignores the widget's enabled state, so readiness is required
                     // again at the click site: a hold that began while the wallet
                     // could pay must not confirm after the balance drops mid-hold.
                     let actionable = Self::confirm_actionable(sufficient_balance, confirm_ready);
                     if ui.add_enabled(actionable, confirm).clicked() && actionable {
                        self.confirmed_or_rejected = Some(true);
                        self.close();
                     }

                     let text = RichText::new("Reject").size(theme.typography.large);
                     let reject = Button::new(text).min_size(button_size).visuals(button_visuals);

                     if ui.add(reject).clicked() {
                        self.confirmed_or_rejected = Some(false);
                        self.close();
                     }
                  });
               });
            });
         });
   }

   /// True when a resolved ERC-7730 display should lead the window instead of the decoded main
   /// event.
   ///
   /// A decoded event outranks the descriptor everywhere except one shape: a bare asset movement.
   /// `Transfer` / `NftTransfer` say what the transaction moved, not what it was for — and when the
   /// call's own target is not the asset contract, that movement is an effect of the call. Aave's
   /// `supply()` pulls the token with an internal `transferFrom`, which is the only log Zeus decodes
   /// (the aToken mint is a `Transfer` from the zero address, which the decoder refuses), so the
   /// window headlined "ERC20 Transfer" over the descriptor that named the call "Supply". A wallet
   /// `transfer()` / `transferFrom()` on the token itself keeps its card: there the target *is* the
   /// asset, so the movement is the intent.
   fn descriptor_leads(&self) -> bool {
      let (Some(analysis), Some(main_event), Some(_)) = (
         self.tx.as_ref(),
         self.tx_main_event.as_ref(),
         self.clear_display.as_ref(),
      ) else {
         return false;
      };

      // Only bare movements are eligible. A decoded swap, bridge, approval, position op or Railgun
      // op names an intent of its own — one Zeus composes with amounts and prices — and stays the
      // headline.
      let asset = match main_event {
         DecodedEvent::Transfer(params) if params.currency.is_erc20() => {
            Some(params.currency.address())
         }
         DecodedEvent::NftTransfer(params) => Some(params.collection),
         _ => None,
      };

      asset.is_some_and(|asset| asset != analysis.interact_to)
   }

   /// The window's title: what the transaction does.
   ///
   /// A decoded main event names itself; an undecodable calldata falls back to the
   /// ERC-7730 heading, and a resolved descriptor also leads when the main event is only an
   /// asset movement ([`Self::descriptor_leads`]). An unlimited approval is called out in the
   /// error colour — the same treatment the signing window gives its own title.
   fn heading(&self, theme: &Theme) -> RichText {
      let (Some(_), Some(main_event)) = (self.tx.as_ref(), self.tx_main_event.as_ref()) else {
         return RichText::new("Confirm Transaction").size(theme.typography.heading);
      };

      let action_name = if main_event.is_other() || self.descriptor_leads() {
         self
            .clear_display
            .as_ref()
            .map(|d| d.heading.clone())
            .unwrap_or_else(|| main_event.name())
      } else {
         main_event.name()
      };

      let mut text = RichText::new(action_name).size(theme.typography.heading);

      if !self.source_is_zeus {
         let unlimited = (main_event.is_permit() && main_event.permit_params().is_unlimited())
            || (main_event.is_token_approval()
               && main_event.token_approval_params().is_unlimited());

         if unlimited {
            text = text.color(theme.colors.error);
         }
      }

      if main_event.is_other() && self.clear_display.is_none() {
         text = text.color(theme.colors.warning);
      }

      text
   }

   /// What the confirm button says: the action, or the reason it cannot be pressed.
   fn confirm_label(&self, sufficient_balance: bool) -> String {
      if sufficient_balance {
         "Confirm".to_string()
      } else {
         format!("Insufficient {}", self.native_currency.symbol)
      }
   }

   /// The confirm action is available only while the button is enabled: a hold that
   /// began before the button disabled must not confirm afterwards.
   fn confirm_actionable(sufficient_balance: bool, confirm_ready: bool) -> bool {
      sufficient_balance && confirm_ready
   }

   fn sufficient_balance(&self, ctx: &mut ZeusContext, eth_spent: U256, sender: Address) -> bool {
      let balance = ctx.get_eth_balance(self.chain.id(), sender);
      let total_cost = eth_spent + self.tx_cost.wei();
      balance.wei() >= total_cost
   }
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Auto-lock cancels a pending confirmation: the window closes, the awaiting
   /// `confirm_tx` is answered "rejected" so it errors out, and any in-flight
   /// preparation is invalidated so it cannot re-open the window after unlock.
   #[test]
   fn reject_answers_rejected_and_closes() {
      let mut window = TxConfirmationWindow::new();
      window.open = true;

      let generation = window.open_generation;
      window.reject();

      assert!(!window.is_open());
      assert_eq!(window.take_confirmed_or_rejected(), Some(false));
      assert_ne!(
         window.open_generation, generation,
         "an in-flight preparation must not survive the lock-time reject"
      );
   }

   /// Every close path invalidates an in-flight preparation, not just the auto-lock
   /// reject: that is why the bump lives in `close()` rather than in `reject()`.
   #[test]
   fn a_plain_close_invalidates_an_in_flight_preparation() {
      let mut window = TxConfirmationWindow::new();

      let generation = window.open_generation;
      window.close();

      assert_ne!(window.open_generation, generation);
   }

   /// One Confirm click answers exactly one flow: the answer is collected once, so
   /// a second awaiting `confirm_tx` cannot read it and broadcast again.
   #[test]
   fn a_confirmation_stays_busy_until_its_answer_is_collected() {
      let mut window = TxConfirmationWindow::new();

      // Nothing in flight: a flow may claim the window.
      assert!(!window.is_busy());
      assert!(window.claim());
      assert!(window.is_busy());

      // The claim is exclusive for as long as the flow holds it — including while
      // the prompt's analysis is still being fetched and no prompt is up yet.
      assert!(!window.claim());

      // The prompt is up.
      window.open = true;
      assert!(window.is_busy());

      // Answered, but the awaiting flow has not collected it yet: still busy, and
      // the answer is collected once and only once.
      window.confirmed_or_rejected = Some(true);
      window.close();
      assert!(window.is_busy());
      assert_eq!(window.take_confirmed_or_rejected(), Some(true));
      assert_eq!(window.take_confirmed_or_rejected(), None);

      // The owning flow releases the window when it is done with it.
      window.release();
      assert!(!window.is_busy());
   }

   /// A wallet that cannot cover value + cost says so on the confirm button, which is
   /// the control it disables — the window spends no row repeating it.
   #[test]
   fn the_confirm_button_names_the_missing_funds() {
      let mut window = TxConfirmationWindow::new();
      window.native_currency = NativeCurrency::from(1);

      assert_eq!(window.confirm_label(true), "Confirm");
      assert_eq!(window.confirm_label(false), "Insufficient ETH");
   }

   /// A completed hold must not confirm a button that has since disabled: the crate
   /// reports the hold as a click without checking whether the widget is enabled, so
   /// the click site re-requires readiness.
   #[test]
   fn confirm_is_only_actionable_while_the_button_is_enabled() {
      assert!(!TxConfirmationWindow::confirm_actionable(
         false, false
      ));
      assert!(!TxConfirmationWindow::confirm_actionable(
         false, true
      ));
      assert!(!TxConfirmationWindow::confirm_actionable(
         true, false
      ));
      assert!(TxConfirmationWindow::confirm_actionable(
         true, true
      ));
   }

   /// The "Gas Limit" field is an override, not decoration. It was written and edited but never read
   /// back, so a typed value was discarded and the transaction went out with the simulated limit —
   /// which on a chain where a new storage slot costs ~98,000 gas cannot be raised from the UI at all.
   #[test]
   fn the_typed_gas_limit_overrides_the_simulated_one() {
      let mut window = TxConfirmationWindow::new();
      window.gas_used = 45_000;
      window.gas_limit = 67_500;

      window.adjusted_gas_limit = "600000".to_string();
      assert_eq!(
         window.get_gas_limit(),
         600_000,
         "the typed value wins"
      );

      window.adjusted_gas_limit = " 600000 ".to_string();
      assert_eq!(
         window.get_gas_limit(),
         600_000,
         "surrounding space is tolerated"
      );

      window.adjusted_gas_limit = String::new();
      assert_eq!(
         window.get_gas_limit(),
         67_500,
         "an empty field falls back to the simulated limit"
      );

      window.adjusted_gas_limit = "not a number".to_string();
      assert_eq!(
         window.get_gas_limit(),
         67_500,
         "an unparseable field falls back too"
      );

      window.adjusted_gas_limit = "1000".to_string();
      assert_eq!(
         window.get_gas_limit(),
         45_000,
         "never below what the simulation used"
      );
   }

   /// The picked name is prompt state: a fresh window has none, `open` stores it with the address
   /// it was picked for, and the next open replaces it — a later flow must not inherit the
   /// previous recipient's name.
   #[test]
   fn open_stores_the_picked_name_and_the_next_open_clears_it() {
      let handle = ZeusCtx::new();
      let mut window = TxConfirmationWindow::new();

      assert_eq!(window.recipient_name, None);

      let picked = Address::repeat_byte(0xaa);
      window.open(
         handle.clone(),
         true,
         String::new(),
         ChainId::default(),
         TransactionAnalysis::dummy_transfer(),
         "1".to_string(),
         false,
         false,
         Some((picked, "alice.eth".to_string())),
      );

      assert_eq!(
         window.recipient_name.as_ref().map(|(address, name)| (*address, name.as_str())),
         Some((picked, "alice.eth"))
      );

      // The spawn behind `open` writes back to the global window, not this one, so the local
      // `open` flag never flips; close anyway so the second open is never refused as busy.
      window.close();

      window.open(
         handle,
         true,
         String::new(),
         ChainId::default(),
         TransactionAnalysis::dummy_transfer(),
         "1".to_string(),
         false,
         false,
         None,
      );

      assert_eq!(window.recipient_name, None);
   }
}

#[cfg(test)]
mod height_budget {
   use super::*;
   use crate::core::clear_signing::ClearDisplay;
   use crate::tests::unlock_ctx;
   use egui::{CentralPanel, Context, RawInput, Rect, pos2};
   use egui_elements::ThemeKind;

   /// The confirmation window's height budget.
   ///
   /// Height is the constraint this window keeps tripping over: every row it carries
   /// is a row the confirmation itself cannot use. Each sample therefore names the
   /// height it renders at today plus ~25 px — about one row — so that re-adding a
   /// row fails here instead of being found in a screenshot. Measured with the same
   /// instrument before the wallet identity / transaction options work these samples
   /// were 594–730 px tall; they are 446–826 px now.
   ///
   /// Ignored by default: it renders the real window and needs this machine's `data/`.
   /// Run it alone: `cargo test -p zeus -- --ignored height_budget --nocapture`.
   #[test]
   #[ignore = "renders the real window: needs this machine's data/ for unlock_ctx"]
   fn the_confirmation_window_fits_its_height_budget() {
      let handle = unlock_ctx();

      let cases = [
         (
            "eoa delegate",
            TransactionAnalysis::dummy_eoa_delegate(),
            "",
            false,
            505.0,
         ),
         (
            "unknown",
            TransactionAnalysis::unknown_tx_1(),
            "app.uniswap.org",
            false,
            470.0,
         ),
         (
            // The tallest combination the window can reach: an undecodable
            // transaction with a clear-signing display and more state changes than
            // the inline block shows at once.
            "unknown with diffs",
            TransactionAnalysis::dummy_with_diffs(),
            "",
            true,
            855.0,
         ),
         (
            // Decoded main event plus both kinds of change: the tallest a dapp
            // transaction can render.
            "swap with diffs",
            decoded_swap_with_diffs(),
            "app.uniswap.org",
            false,
            825.0,
         ),
         (
            // One capped section per kind of change: a long balance list must cost no
            // more than a short one.
            "long balance list",
            analysis_with_many_changes(),
            "app.uniswap.org",
            false,
            745.0,
         ),
         (
            "clear signed",
            TransactionAnalysis::dummy_clear_signed(),
            "app.aave.com",
            true,
            605.0,
         ),
         (
            "shield",
            TransactionAnalysis::dummy_shield(),
            "",
            false,
            510.0,
         ),
         (
            "unshield",
            TransactionAnalysis::dummy_unshield(),
            "",
            false,
            550.0,
         ),
      ];

      for (name, analysis, dapp, clear_display, budget) in cases {
         let mut window = window_for(&handle, analysis, dapp, clear_display);
         let height = modal_height(&mut window, &handle);

         println!("{name}: {height:.0} px (budget {budget:.0})");

         assert!(
            height <= budget,
            "{name} renders {height:.0} px tall, past its {budget:.0} px budget"
         );
      }
   }

   /// Render the window headlessly and return the settled height of the modal card.
   ///
   /// Read on several frames: a centred modal header re-asserts the width it measured
   /// on the previous one, so a single frame shows a size that is still moving.
   fn modal_height(window: &mut TxConfirmationWindow, handle: &ZeusCtx) -> f32 {
      let egui_ctx = Context::default();
      let mut theme = Theme::new(ThemeKind::TokyoNight);
      theme.install(&egui_ctx);
      egui_extras::install_image_loaders(&egui_ctx);
      let icons = Arc::new(Icons::new(&egui_ctx).expect("icons"));
      let appearance = AppearanceSettings::new();

      // How `egui_elements::Modal` registers its card: the Area id it salts with the
      // id passed to `Modal::new`.
      let card = Id::new("elegance_modal_window").with(Id::new(Id::new("tx_confirmation_window")));

      let mut height = 0.0;

      for _ in 0..4 {
         let handle = handle.clone();

         let mut out = egui_ctx.run_ui(
            RawInput {
               screen_rect: Some(Rect::from_min_size(
                  pos2(0.0, 0.0),
                  vec2(1290.0, 950.0),
               )),
               ..Default::default()
            },
            |egui_ctx| {
               CentralPanel::default().show(egui_ctx, |ui| {
                  handle.write(|c| window.show(c, &theme, icons.clone(), &appearance, ui));
               });
            },
         );
         out.textures_delta.clear();

         height = egui_ctx
            .memory(|m| m.area_rect(card))
            .map(|rect| rect.height())
            .unwrap_or(height);
      }

      height
   }

   /// The `with diffs` fixture with a long balance list: ten rows behind one capped
   /// section, so a transaction with a dozen movements costs no more than a short one.
   fn analysis_with_many_changes() -> TransactionAnalysis {
      let mut analysis = TransactionAnalysis::dummy_with_diffs();
      let tokens = analysis.balance_diff.tokens.clone();

      for i in 0..10 {
         analysis.balance_diff.tokens.push(tokens[i % tokens.len()].clone());
      }

      analysis
   }

   /// A decoded main event *and* state changes: what a dapp transaction looks like now
   /// that the inline block is not gated on the main event being undecodable.
   fn decoded_swap_with_diffs() -> TransactionAnalysis {
      let mut analysis = TransactionAnalysis::dummy_swap();
      let with_diffs = TransactionAnalysis::dummy_with_diffs();
      analysis.set_diffs(with_diffs.balance_diff, with_diffs.approval_diff);
      analysis
   }

   /// The window as the confirmation path leaves it, minus the parts of `open_inner`
   /// that cannot run in a test (it spawns through `RT` and writes back through
   /// `SHARED_GUI`).
   fn window_for(
      handle: &ZeusCtx,
      analysis: TransactionAnalysis,
      dapp: &str,
      clear_display: bool,
   ) -> TxConfirmationWindow {
      let chain = handle.chain();
      let gas_used = analysis.gas_used;
      let gas_limit = gas_used * 15 / 10;

      let mut window = TxConfirmationWindow::new();
      window.chain = chain;
      window.native_currency = NativeCurrency::from(chain.id());
      window.dapp = dapp.to_string();
      window.gas_used = gas_used;
      window.gas_limit = gas_limit;
      window.adjusted_gas_limit = gas_limit.to_string();
      window.priority_fee = "1".to_string();
      window.tx_main_event = Some(analysis.infer_main_event(handle.clone(), chain.id()));
      window.tx = Some(analysis);
      window.clear_display = clear_display.then(ClearDisplay::dummy_calldata);
      window.open = true;

      handle.write(|c| window.calculate_tx_cost(c, gas_used));

      window
   }
}

#[cfg(test)]
mod descriptor_precedence {
   use super::*;
   use crate::core::clear_signing::ClearDisplay;
   use crate::core::{DecodedEvent, TransactionAnalysis};
   use crate::tests::test_ctx;
   use egui_elements::ThemeKind;
   use zeus_eth::currency::ERC20Token;
   use zeus_eth::utils::address_book::universal_router_v2;

   /// The reported shape: `supply()` decodes to exactly one ERC-20 `Transfer` — the `transferFrom`
   /// the Pool made on the user's behalf.
   fn aave_supply() -> TransactionAnalysis {
      let mut analysis = TransactionAnalysis::dummy_clear_signed();
      analysis.decoded_events = vec![DecodedEvent::dummy_erc20_transfer()];
      analysis
   }

   fn window(analysis: TransactionAnalysis, display: Option<ClearDisplay>) -> TxConfirmationWindow {
      let mut window = TxConfirmationWindow::new();
      window.tx_main_event =
         Some(analysis.infer_main_event(test_ctx(analysis.chain), analysis.chain));
      window.tx = Some(analysis);
      window.clear_display = display;
      window
   }

   fn heading_of(window: &TxConfirmationWindow) -> String {
      let theme = Theme::new(ThemeKind::TokyoNight);
      window.heading(&theme).text().to_string()
   }

   #[test]
   fn a_pulled_token_transfer_yields_to_the_descriptor() {
      let window = window(
         aave_supply(),
         Some(ClearDisplay::dummy_calldata()),
      );

      assert_eq!(
         window.tx_main_event.as_ref().unwrap().name(),
         "ERC20 Transfer",
         "the fixture must still rank as the bare movement it reports"
      );
      assert!(window.descriptor_leads());
      assert_eq!(heading_of(&window), "Supply");
   }

   #[test]
   fn a_direct_token_call_keeps_its_own_card() {
      let mut analysis = aave_supply();
      analysis.interact_to = ERC20Token::weth().address;
      let window = window(analysis, Some(ClearDisplay::dummy_calldata()));

      assert!(!window.descriptor_leads());
      assert_eq!(heading_of(&window), "ERC20 Transfer");
   }

   #[test]
   fn a_decoded_protocol_intent_still_leads() {
      let mut analysis = TransactionAnalysis::dummy_swap();
      analysis.interact_to = universal_router_v2(1).unwrap();
      let window = window(analysis, Some(ClearDisplay::dummy_calldata()));
      let main_event_name = window.tx_main_event.as_ref().unwrap().name();

      assert!(!window.descriptor_leads());
      assert_eq!(heading_of(&window), main_event_name);
   }

   #[test]
   fn no_descriptor_changes_nothing() {
      let window = window(aave_supply(), None);

      assert!(!window.descriptor_leads());
      assert_eq!(heading_of(&window), "ERC20 Transfer");
   }
}
