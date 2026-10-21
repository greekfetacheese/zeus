//! UI that allows the user to inspect and sign a message

use egui::{
   Align, FontId, Frame, Id, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui, vec2,
};
use egui_elements::{Button, Label, Modal, Theme};

use crate::assets::icons::Icons;
use crate::core::clear_signing::FormattedValue;
use crate::core::{SignMsgType, ZeusContext};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::common::{delayed_action_label, wallet_identity};
use crate::gui::ui::tx::{address, chain};

use serde_json::{Value, to_string_pretty};
use std::fmt::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use zeus_eth::{
   alloy_dyn_abi::{Eip712Types, TypedData},
   alloy_primitives::{Address, U256},
   types::ChainId,
};

pub struct SignMsgWindow {
   open: bool,
   /// A flow has claimed this window ([`Self::claim`]) and holds it until it is done with the
   /// prompt. Claimed under the same lock that opens it, so a second request cannot replace the
   /// prompt the user is reading — the phase where two flows would otherwise share one Sign click.
   pending: bool,
   dapp: String,
   chain: ChainId,
   /// Account that will produce the signature.
   account: Address,
   msg: Option<SignMsgType>,
   formatted_msg: Option<String>,
   signed: Option<bool>,
   opened_at: Option<Instant>,
   size: (f32, f32),
}

impl SignMsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         pending: false,
         dapp: String::new(),
         chain: ChainId::default(),
         account: Address::ZERO,
         msg: None,
         formatted_msg: None,
         signed: None,
         opened_at: None,
         size: (500.0, 750.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, dapp: String, chain: u64, msg: SignMsgType, account: Address) {
      if self.is_busy() {
         return;
      }

      self.dapp = dapp;
      self.chain = chain.into();
      self.account = account;
      self.open = true;
      self.msg = Some(msg);
      self.formatted_msg = None;
      self.signed = None;
      self.opened_at = Some(Instant::now());
   }

   pub fn close(&mut self) {
      self.open = false;
   }

   /// True while a signature is in flight: a flow holds the window ([`Self::pending`]) or the
   /// prompt is up.
   ///
   /// `sign_message` refuses to raise a second prompt while this holds, so two requests can never
   /// share one window — and therefore one Sign click.
   ///
   /// An answer still waiting in `signed` is deliberately *not* busy. The flow that owns the window
   /// holds the claim until it has collected the answer, so an uncollected answer can only exist
   /// once that flow is gone — and reading it as busy would let a flow that died after the user
   /// answered wedge every later request.
   pub fn is_busy(&self) -> bool {
      self.pending || self.open
   }

   /// Claim the window for one flow, from just before its prompt is put up until it is done with
   /// it. Returns false when another flow already holds it.
   ///
   /// Call under the same lock as the [`Self::is_busy`] check (`sign_message` does), or two flows
   /// can both pass the check. [`Self::release`] hands it back.
   pub fn claim(&mut self) -> bool {
      if self.pending {
         return false;
      }
      self.pending = true;
      true
   }

   pub fn release(&mut self) {
      self.pending = false;
   }

   /// Collect the answer, consuming it.
   ///
   /// Single-use on purpose: an answer that could be read twice would answer two flows from one
   /// Sign click, and the second would produce a signature the user never reviewed.
   pub fn take_signed(&mut self) -> Option<bool> {
      self.signed.take()
   }

   /// Close without signing and answer the awaiting `sign_message` with a
   /// cancel, so it errors out instead of hanging. Used when auto-lock fires.
   pub fn reject(&mut self) {
      self.close();
      self.signed = Some(false);
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

      // The heading names what is actually being signed (permit, sign-in,
      // plain message, …), so it is the only title — the body does not repeat it.
      let mut title = RichText::new(self.msg.as_ref().map_or("Sign Message", |msg| msg.title()))
         .size(theme.typography.heading);

      if self.msg.as_ref().is_some_and(|msg| msg.is_unlimited()) {
         title = title.color(theme.colors.error);
      }

      let id = Id::new("sign_msg_window");
      let frame = theme.window_frame.fill(theme.frame1.fill);

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_max_height(self.size.1);

            let frame2 = theme.frame2;

            let normal = theme.typography.normal;
            let large = theme.typography.large;

            let warning = theme.colors.warning;

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing.y = theme.spacing.md;
                  ui.spacing_mut().button_padding = theme.button_padding;

                  let msg = self.msg.clone();

                  if msg.is_none() {
                     ui.label("No message to sign");
                     return;
                  }

                  let msg = msg.unwrap();

                  frame2.show(ui, |ui| {
                     if !self.dapp.is_empty() {
                        ui.label(RichText::new(&self.dapp).size(large));

                        ui.separator();
                     }

                     // The signing account may differ from the active one (apps get
                     // their own account), so always show which account signs. This is
                     // an identity line under the origin rather than a labelled data
                     // row: it belongs with "who is asking", not with the message
                     // fields, and the address stays verifiable next to the name.
                     ui.add(wallet_identity(
                        ctx,
                        self.chain.id(),
                        self.account,
                        theme,
                        icons.clone(),
                     ));
                  });

                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width(), 45.0);

                  if msg.is_other() {
                     let p = "Unknown message, review the details below carefully.";
                     let text = RichText::new(p).size(normal).color(warning);
                     ui.label(text);
                  }

                  if msg.is_permit2_single() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           permit2_single_approval(ctx, self.chain, &msg, theme, icons.clone(), ui);
                        });
                     });
                  }

                  if msg.is_permit2612() {
                     frame.show(ui, |ui| {
                        permit2612_approval(ctx, self.chain, &msg, theme, icons.clone(), ui);
                     });
                  }

                  if msg.is_clear_signed() {
                     let frame_size = vec2(ui.available_width(), 300.0);

                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                              clear_signed_ui(ctx, self.chain, &msg, theme, icons.clone(), ui);
                           });
                        });
                     });
                  }

                  ui.add_space(20.0);

                  if self.formatted_msg.is_none() {
                     self.formatted_msg = Some(format_sign_data(&msg, self.chain));
                  }

                  // Show the msg
                  if let Some(mut formatted) = self.formatted_msg.clone() {
                     let text_edit = TextEdit::multiline(&mut formatted)
                        .font(FontId::proportional(normal))
                        .margin(Margin::same(10))
                        .desired_width(ui.available_width() * 0.95);

                     ui.label(RichText::new("Message").size(large));

                     let height = if msg.is_known() { 300.0 } else { 450.0 };
                     ScrollArea::vertical().max_height(height).content_margin(5).show(ui, |ui| {
                        ui.add(text_edit);
                     });
                  }

                  ui.add_space(20.0);
                  let ui_size = vec2(ui.available_width() * 0.9, 45.0);

                  ui.allocate_ui(ui_size, |ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.xl;
                     let button_size = vec2(ui.available_width() * 0.5, 45.0);

                     ui.horizontal(|ui| {
                        let (sign_ready, sign_label) = delayed_action_label(self.opened_at, "Sign");
                        if !sign_ready {
                           ui.ctx().request_repaint_after(Duration::from_millis(100));
                        }

                        let text = RichText::new(sign_label).size(normal);
                        let ok_btn = Button::new(text).min_size(button_size);

                        // Copy the settings out first: the widget needs an owned
                        // value, and `HoldSettings` is `Copy`.
                        let hold = appearance.button_settings(theme.colors.accent);
                        let ok_btn = match &hold {
                           Some(settings) => ok_btn.hold_settings(settings),
                           None => ok_btn,
                        };

                        // The crate's hold reports a completed hold as a click and
                        // ignores the widget's enabled state. `sign_ready` only goes
                        // `false -> true` in practice (the delayed-action countdown),
                        // so this is symmetry with the confirm window's guard and
                        // defence rather than a live bug.
                        let actionable = Self::sign_actionable(sign_ready);
                        if ui.add_enabled(actionable, ok_btn).clicked() && actionable {
                           self.signed = Some(true);
                           self.close();
                        }

                        let text = RichText::new("Cancel").size(normal);
                        let cancel_btn = Button::new(text).min_size(button_size);

                        if ui.add(cancel_btn).clicked() {
                           // Not `reset()`: rebuilding the struct would drop the owning flow's
                           // claim and could lose this answer to a request that claimed the window
                           // in the meantime, leaving both flows waiting.
                           self.signed = Some(false);
                           self.close();
                        }
                     });
                  });
               });
            });
         });
   }

   /// The sign action is available only while the button is enabled: a hold that
   /// began before the button disabled must not sign afterwards.
   fn sign_actionable(sign_ready: bool) -> bool {
      sign_ready
   }
}

fn permit2_single_approval(
   ctx: &mut ZeusContext,
   _current_chain: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_details();
   let tint = theme.image_tint_recommended;
   let icon_size = vec2(24.0, 24.0);

   // Show the chain the permit is actually signed for. Its token was fetched on that chain, so it
   // is a supported one; the wallet's own chain is only what the user happened to be connected to,
   // and presenting it as the permit's chain describes an allowance on another chain as a signature
   // on this one. Permit2's `verifyingContract` cannot stand in for this — it is the same address
   // on every chain — so the domain's `chainId` is the only thing that names a chain here.
   let permit_chain: ChainId = details.chain.into();

   let size = vec2(ui.available_width(), 30.0);

   ui.allocate_ui(size, |ui| {
      // Chain
      chain(permit_chain, theme, icons.clone(), ui);

      // Token
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approve Token").size(theme.typography.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = details.amount();
            let text = format!("{:.10} {}", amount, details.token.symbol);
            let icon = icons
               .token_icon_x32(
                  details.token.address,
                  details.token.chain_id,
                  tint,
               )
               .fit_to_exact_size(icon_size);

            let mut text = RichText::new(text).size(theme.typography.large);

            if details.is_unlimited() {
               text = text.color(theme.colors.warning);
            }

            let label = Label::new(text, Some(icon))
               .wrap()
               .visuals(theme.label_visuals())
               .interactive(false);
            ui.add(label);
         });
      });

      let is_revoke = details.amount.is_zero();

      // Approval expire

      // Only show the expiration if its an actual approval
      // Usually on revokes we pass a timestamp of 0 which will show the "Invalid timestamp"
      if !is_revoke {
         ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
               ui.label(RichText::new("Approval expire").size(theme.typography.large));
            });

            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
               let expire = details.expiration.to_relative();
               let text = RichText::new(expire).size(theme.typography.large);
               ui.label(text);
            });
         });
      }

      // Permit2 Contract
      let label = "Contract interaction";
      address(
         ctx,
         permit_chain,
         label,
         details.permit2_contract,
         theme,
         ui,
      );

      // Spender
      address(
         ctx,
         permit_chain,
         "Spender",
         details.spender,
         theme,
         ui,
      );
   });
}

fn permit2612_approval(
   ctx: &mut ZeusContext,
   _current_chain: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2612_details();
   let tint = theme.image_tint_recommended;
   let icon_size = vec2(24.0, 24.0);

   // Show the chain the permit is actually signed for. Its token was fetched on that
   // chain, so it is a supported one; the wallet's own chain is only what the user
   // happened to be connected to, and presenting it as the permit's chain describes an
   // allowance on another chain as a signature on this one.
   let permit_chain: ChainId = details.token.chain_id.into();

   chain(permit_chain, theme, icons.clone(), ui);

   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Approve Token").size(theme.typography.large));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let text = format!("{} {}", details.amount(), details.token.symbol);
         let icon = icons
            .token_icon_x32(
               details.token.address,
               details.token.chain_id,
               tint,
            )
            .fit_to_exact_size(icon_size);

         let mut text = RichText::new(text).size(theme.typography.large);

         if details.is_unlimited() {
            text = text.color(theme.colors.warning);
         }

         let label = Label::new(text, Some(icon))
            .wrap()
            .visuals(theme.label_visuals())
            .interactive(false);

         ui.add(label);
      });
   });

   if !details.amount.is_zero() {
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Deadline").size(theme.typography.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let text = RichText::new(details.deadline_label()).size(theme.typography.large);
            ui.label(text);
         });
      });
   }

   address(
      ctx,
      permit_chain,
      "Token",
      details.token.address,
      theme,
      ui,
   );
   address(
      ctx,
      permit_chain,
      "Owner",
      details.owner,
      theme,
      ui,
   );
   address(
      ctx,
      permit_chain,
      "Spender",
      details.spender,
      theme,
      ui,
   );
}

fn clear_signed_ui(
   ctx: &mut ZeusContext,
   _current_chain: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.clear_signed_details();
   let display = &details.display;
   let tint = theme.image_tint_recommended;
   let icon_size = vec2(24.0, 24.0);

   // The chain the descriptor and every value below were resolved for: the message's own domain
   // chain, not the chain the wallet is connected to. The display is built from
   // `(chainId, verifyingContract, typeHash)`, so naming the wallet's chain here would describe an
   // approval on one chain while the prompt's own fields came from another.
   let display_chain: ChainId = details.chain.into();

   if let Some(owner) = display.owner.as_ref() {
      let name = match &display.contract_name {
         Some(c) => format!("{owner} · {c}"),
         None => owner.clone(),
      };
      ui.label(RichText::new(name).size(theme.typography.normal));
   }

   if let Some(intent) = display.interpolated_intent.as_ref() {
      ui.label(RichText::new(intent).size(theme.typography.large));
   }

   for warning in &display.warnings {
      ui.label(RichText::new(warning).size(theme.typography.normal).color(theme.colors.warning));
   }

   chain(display_chain, theme, icons.clone(), ui);

   for field in &display.fields {
      match &field.value {
         FormattedValue::Address(addr) => {
            address(ctx, display_chain, &field.label, *addr, theme, ui);
         }
         FormattedValue::TokenAmount {
            amount,
            token,
            unlimited,
         } => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let amount_txt = if *unlimited {
                     "Unlimited"
                  } else {
                     amount.abbreviated()
                  };

                  let text = format!("{:.10} {}", amount_txt, token.symbol);
                  let icon = icons
                     .token_icon_x32(token.address, token.chain_id, tint)
                     .fit_to_exact_size(icon_size);

                  let mut text = RichText::new(text).size(theme.typography.large);

                  if *unlimited {
                     text = text.color(theme.colors.warning);
                  }

                  let label = Label::new(text, Some(icon))
                     .wrap()
                     .visuals(theme.label_visuals())
                     .interactive(false);
                  ui.add(label);
               });
            });
         }
         FormattedValue::Date(ts) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(ts.to_relative()).size(theme.typography.large));
               });
            });
         }
         FormattedValue::Text(text) | FormattedValue::Bytes(text) => {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  ui.label(RichText::new(&field.label).size(theme.typography.large));
               });
               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  ui.label(RichText::new(text).size(theme.typography.large));
               });
            });
         }
      }
   }
}

fn _permit2_batch_approval_ui(
   ctx: &mut ZeusContext,
   chain_id: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_batch_details();
   let tint = theme.image_tint_recommended;

   ui.label(RichText::new("Permit2 Batch Token Approval").size(theme.typography.normal));

   // Chain
   chain(chain_id, theme, icons.clone(), ui);

   ui.horizontal(|ui| {
      ui.label(RichText::new("Approve Tokens").size(theme.typography.normal));
   });

   let token_details = details
      .tokens
      .iter()
      .zip(details.amounts.iter())
      .zip(details.amounts_usd.iter());

   // Tokens
   for ((token, amount), _amount_usd) in token_details {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount_text = if amount.wei() == U256::MAX {
            "Unlimited"
         } else {
            amount.abbreviated()
         };

         let text = format!("{:.10} {}", amount_text, token.symbol);
         let icon = icons.token_icon_x32(token.address, token.chain_id, tint);
         let mut text = RichText::new(text).size(theme.typography.normal);

         if amount.wei() == U256::MAX {
            text = text.color(theme.colors.warning);
         }

         let label = Label::new(text, Some(icon)).wrap().interactive(false);
         ui.add(label);
      });
   }

   // Approval expire
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Approval expire").size(theme.typography.normal));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let expire = details.expiration.to_relative();
         let text = RichText::new(expire).size(theme.typography.normal);
         ui.label(text);
      });
   });

   // Permit2 Contract
   let label = "Contract interaction";
   address(
      ctx,
      chain_id,
      label,
      details.permit2_contract,
      theme,
      ui,
   );

   // Spender
   address(
      ctx,
      chain_id,
      "Spender",
      details.spender,
      theme,
      ui,
   );
}

fn format_sign_data(msg: &SignMsgType, _chain: ChainId) -> String {
   if msg.is_permit2_single() {
      return format_permit2_single_approval(msg);
   }

   if msg.is_permit2612() {
      return format_permit2612(msg);
   }

   if let Some(typed) = msg.typed_data() {
      return format_typed_data(&typed);
   }

   if let Some(msg_str) = msg.msg_string() {
      return msg_str;
   }

   // Fallback
   match msg.msg_value() {
      Value::String(s) => s,
      val => {
         if val.is_object() {
            to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
         } else {
            val.to_string()
         }
      }
   }
}

fn format_permit2_single_approval(msg: &SignMsgType) -> String {
   let details = msg.permit2_details();
   let mut formatted = String::new();

   writeln!(formatted, "Permit2 Token Approval").unwrap();
   writeln!(formatted, "===================").unwrap();
   writeln!(formatted).unwrap();

   // Domain
   writeln!(formatted, "Domain:").unwrap();
   writeln!(formatted, "Name: {}", "Uniswap Permit2").unwrap();
   writeln!(formatted, "Version: 2").unwrap();
   // The domain's own chain, which is what the signature is bound to and where the allowance
   // lands — not the chain the wallet is connected to.
   writeln!(formatted, "Chain: {}", details.chain).unwrap();
   writeln!(
      formatted,
      "Verifying Contract: {}",
      details.permit2_contract.to_string()
   )
   .unwrap();
   writeln!(formatted).unwrap();

   // Message details
   writeln!(formatted, "Message:").unwrap();
   writeln!(formatted, "Token: {}", details.token.address).unwrap();
   writeln!(
      formatted,
      "Amount: {}",
      details.amount.wei().to_string()
   )
   .unwrap();
   writeln!(
      formatted,
      "Expiration: {}",
      details.expiration.timestamp().to_string()
   )
   .unwrap();
   writeln!(
      formatted,
      "Spender: {}",
      details.spender.to_string()
   )
   .unwrap();

   formatted
}

fn format_permit2612(msg: &SignMsgType) -> String {
   let details = msg.permit2612_details();
   let mut formatted = String::new();

   writeln!(formatted, "{}", details.title()).unwrap();
   writeln!(formatted, "===================").unwrap();
   writeln!(formatted).unwrap();

   writeln!(
      formatted,
      "Token: {}",
      details.token.address.to_string()
   )
   .unwrap();
   writeln!(
      formatted,
      "Amount: {}",
      details.amount.wei().to_string()
   )
   .unwrap();
   if !details.amount.is_zero() {
      writeln!(
         formatted,
         "Deadline: {}",
         details.deadline.to_string()
      )
      .unwrap();
   }
   writeln!(formatted, "Owner: {}", details.owner.to_string()).unwrap();
   writeln!(
      formatted,
      "Spender: {}",
      details.spender.to_string()
   )
   .unwrap();

   formatted
}

/// Formats a generic EIP-712 TypedData structure in a readable way.
/// This is a best-effort formatter for unknown typed data messages.
/// It structures the output with sections for Domain, Types, and Message.
fn format_typed_data(typed_data: &TypedData) -> String {
   let mut formatted = String::new();

   // Title based on primary type
   writeln!(
      formatted,
      "Signing Typed Data: {}",
      typed_data.primary_type
   )
   .unwrap();
   writeln!(formatted, "=========================").unwrap();
   writeln!(formatted).unwrap();

   // Domain section
   writeln!(formatted, "Domain:").unwrap();
   if let Some(name) = &typed_data.domain.name {
      writeln!(formatted, " Name: {}", name).unwrap();
   }
   if let Some(version) = &typed_data.domain.version {
      writeln!(formatted, " Version: {}", version).unwrap();
   }
   if let Some(chain_id) = &typed_data.domain.chain_id {
      writeln!(formatted, " Chain ID: {}", chain_id).unwrap();
   }
   if let Some(verifying_contract) = &typed_data.domain.verifying_contract {
      writeln!(
         formatted,
         " Verifying Contract: {}",
         verifying_contract
      )
      .unwrap();
   }
   if let Some(salt) = &typed_data.domain.salt {
      writeln!(formatted, " Salt: {}", salt).unwrap();
   }
   writeln!(formatted).unwrap();

   // Types section (convert Resolver to Eip712Types to access the map)
   let types: Eip712Types = (&typed_data.resolver).into();
   writeln!(formatted, "Types:").unwrap();
   for (type_name, props) in types.iter() {
      writeln!(formatted, " {}", type_name).unwrap();
      for prop in props {
         writeln!(
            formatted,
            "  - {}: {}",
            prop.name(),
            prop.type_name()
         )
         .unwrap();
      }
      writeln!(formatted).unwrap();
   }
   writeln!(formatted).unwrap();

   // Message section
   writeln!(formatted, "Message:").unwrap();
   match to_string_pretty(&typed_data.message) {
      Ok(pretty_message) => {
         // Indent the pretty JSON for readability
         for line in pretty_message.lines() {
            writeln!(formatted, " {}", line).unwrap();
         }
      }
      Err(e) => {
         tracing::error!("Failed to pretty-print message: {}", e);
         writeln!(formatted, " {}", typed_data.message.to_string()).unwrap();
      }
   }

   formatted
}

#[cfg(test)]
mod tests {
   use super::*;

   /// Auto-lock cancels a pending signature: the window closes and the awaiting
   /// `sign_message` is answered "not signed" so it returns a cancel error.
   #[test]
   fn reject_answers_cancelled_and_closes() {
      let mut window = SignMsgWindow::new();
      window.open = true;

      window.reject();

      assert!(!window.is_open());
      assert_eq!(window.take_signed(), Some(false));
   }

   /// A completed hold must not sign a button that has since disabled: the crate
   /// reports the hold as a click without checking whether the widget is enabled.
   #[test]
   fn sign_is_only_actionable_while_the_button_is_enabled() {
      assert!(!SignMsgWindow::sign_actionable(false));
      assert!(SignMsgWindow::sign_actionable(true));
   }

   /// One Sign click answers exactly one flow: the answer is collected once, so a second awaiting
   /// `sign_message` cannot read it and sign a payload the user never reviewed.
   ///
   /// This is the whole point of the guard — the awaiters sign their *own* message, not the one the
   /// window is showing, so an answer that could be read twice hands a dapp a signature for a
   /// different message than the user approved.
   #[test]
   fn a_signature_stays_busy_until_its_answer_is_collected() {
      let mut window = SignMsgWindow::new();

      // Nothing in flight: a flow may claim the window.
      assert!(!window.is_busy());
      assert!(window.claim());
      assert!(window.is_busy());

      // The claim is exclusive for as long as the flow holds it.
      assert!(!window.claim());

      // The prompt is up.
      window.open = true;
      assert!(window.is_busy());

      // Answered, and the owning flow still holds the claim until it has collected it: still busy,
      // and the answer is collected once and only once.
      window.signed = Some(true);
      window.close();
      assert!(window.is_busy());
      assert_eq!(window.take_signed(), Some(true));
      assert_eq!(window.take_signed(), None);

      // The owning flow releases the window when it is done with it.
      window.release();
      assert!(!window.is_busy());
   }
}
