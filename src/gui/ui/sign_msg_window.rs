use egui::{
   Align, Align2, Button, FontId, Frame, Layout, Margin, Order, RichText, ScrollArea, TextEdit, Ui,
   Window, vec2,
};
use zeus_theme::Theme;
use egui_widgets::Label;

use super::{address, chain, contract_interact};
use crate::assets::icons::Icons;
use crate::core::{
   ZeusCtx,
   utils::{format_expiry, sign::SignMsgType},
};

use regex::Regex;
use serde_json::{Value, to_string_pretty};
use std::fmt::Write;
use std::sync::Arc;
use zeus_eth::{
   alloy_dyn_abi::{Eip712Types, TypedData},
   alloy_primitives::U256,
   types::ChainId,
};

pub struct SignMsgWindow {
   open: bool,
   dapp: String,
   chain: ChainId,
   msg: Option<SignMsgType>,
   formatted_msg: Option<String>,
   signed: Option<bool>,
   size: (f32, f32),
}

impl SignMsgWindow {
   pub fn new() -> Self {
      Self {
         open: false,
         dapp: String::new(),
         chain: ChainId::default(),
         msg: None,
         formatted_msg: None,
         signed: None,
         size: (500.0, 550.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn open(&mut self, ctx: ZeusCtx, dapp: String, chain: u64, msg: SignMsgType) {
      ctx.set_sign_msg_window_open(true);
      self.dapp = dapp;
      self.chain = chain.into();
      self.open = true;
      self.msg = Some(msg);
      self.formatted_msg = None;
      self.signed = None;
   }

   pub fn reset(&mut self, ctx: ZeusCtx) {
      ctx.set_sign_msg_window_open(false);
      *self = Self::new();
   }

   pub fn close(&mut self, ctx: ZeusCtx) {
      ctx.set_sign_msg_window_open(false);
      self.open = false;
   }

   pub fn is_signed(&self) -> Option<bool> {
      self.signed
   }

   pub fn show(&mut self, ctx: ZeusCtx, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      Window::new("Sign Message")
         .title_bar(false)
         .resizable(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .collapsible(false)
         .frame(Frame::window(ui.style()))
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing.y = 15.0;
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let msg = self.msg.clone();

                  if msg.is_none() {
                     ui.label("No message to sign");
                     return;
                  }

                  let msg = msg.unwrap();

                  ui.label(RichText::new(&self.dapp).size(theme.text_sizes.large));

                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width(), 45.0);

                  ui.label(RichText::new(msg.title()).size(theme.text_sizes.large));

                  if msg.is_permit2_single() {
                     ui.allocate_ui(frame_size, |ui| {
                        frame.show(ui, |ui| {
                           permit2_single_approval(
                              ctx.clone(),
                              self.chain,
                              &msg,
                              theme,
                              icons.clone(),
                              ui,
                           );
                        });
                     });
                  }

                  ui.add_space(30.0);

                  if self.formatted_msg.is_none() {
                     self.formatted_msg = Some(format_sign_data(&msg, self.chain));
                  }

                  // Show the msg
                  if let Some(mut formatted) = self.formatted_msg.clone() {
                     let text_edit = TextEdit::multiline(&mut formatted)
                        .font(FontId::proportional(theme.text_sizes.large))
                        .margin(Margin::same(10))
                        .desired_width(ui.available_width() * 0.95);

                     ui.label(RichText::new("Message").size(theme.text_sizes.large));

                     let height = if msg.is_known() { 150.0 } else { 300.0 };
                     ScrollArea::vertical().max_height(height).show(ui, |ui| {
                        ui.add(text_edit);
                     });
                  }

                  ui.add_space(20.0);
                  let ui_size = vec2(ui.available_width() * 0.9, 45.0);

                  ui.allocate_ui(ui_size, |ui| {
                     ui.spacing_mut().item_spacing.x = 20.0;
                     let button_size = vec2(ui.available_width() * 0.5, 45.0);

                     ui.horizontal(|ui| {
                        let ok_btn =
                           Button::new(RichText::new("Sign").size(theme.text_sizes.normal))
                              .min_size(button_size);
                        if ui.add(ok_btn).clicked() {
                           self.signed = Some(true);
                           self.close(ctx.clone());
                        }

                        let cancel_btn =
                           Button::new(RichText::new("Cancel").size(theme.text_sizes.normal))
                              .min_size(button_size);
                        if ui.add(cancel_btn).clicked() {
                           self.reset(ctx);
                           self.signed = Some(false);
                        }
                     });
                  });
               });
            });
         });
   }
}

fn permit2_single_approval(
   ctx: ZeusCtx,
   chain_id: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_details();
   let tint = theme.image_tint_recommended;

   let size = vec2(ui.available_width(), 30.0);

   ui.allocate_ui(size, |ui| {
      // Chain
      chain(chain_id, theme, icons.clone(), ui);

      // Token
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approve Token").size(theme.text_sizes.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let amount = details.amount();
            let text = format!("{} {}", amount, details.token.symbol);
            let icon = icons.token_icon_x32(details.token.address, details.token.chain_id, tint);
            let label = Label::new(
               RichText::new(text).size(theme.text_sizes.large),
               Some(icon),
            )
            .wrap();
            ui.add(label);
         });
      });

      // Approval expire
      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.label(RichText::new("Approval expire").size(theme.text_sizes.large));
         });

         ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let expire = format_expiry(details.expiration);
            let text = RichText::new(expire).size(theme.text_sizes.large);
            ui.label(text);
         });
      });

      // Permit2 Contract
      contract_interact(
         ctx.clone(),
         chain_id,
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
   });
}

fn _permit2_batch_approval_ui(
   ctx: ZeusCtx,
   chain_id: ChainId,
   msg: &SignMsgType,
   theme: &Theme,
   icons: Arc<Icons>,
   ui: &mut Ui,
) {
   let details = msg.permit2_batch_details();
   let tint = theme.image_tint_recommended;

   ui.label(RichText::new("Permit2 Batch Token Approval").size(theme.text_sizes.normal));

   // Chain
   chain(chain_id, theme, icons.clone(), ui);

   ui.horizontal(|ui| {
      ui.label(RichText::new("Approve Tokens").size(theme.text_sizes.normal));
   });

   let token_details = details
      .tokens
      .iter()
      .zip(details.amounts.iter())
      .zip(details.amounts_usd.iter());

   // Tokens
   for ((token, amount), _amount_usd) in token_details {
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let amount = if amount.wei() == U256::MAX {
            "Unlimited".to_string()
         } else {
            amount.format_abbreviated()
         };

         let text = format!("{} {}", amount, token.symbol);
         let icon = icons.token_icon_x32(token.address, token.chain_id, tint);
         let label = Label::new(
            RichText::new(text).size(theme.text_sizes.normal),
            Some(icon),
         )
         .wrap();
         ui.add(label);
      });
   }

   // Approval expire
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new("Approval expire").size(theme.text_sizes.normal));
      });

      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         let expire = format_expiry(details.expiration);
         let text = RichText::new(expire).size(theme.text_sizes.normal);
         ui.label(text);
      });
   });

   // Permit2 Contract
   contract_interact(
      ctx.clone(),
      chain_id,
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

   // Protocol/Dapp
   // TODO:
}

fn format_sign_data(msg: &SignMsgType, _chain: ChainId) -> String {
   let typed_data = msg.typed_data();

   if msg.is_permit2_single() {
      format_permit2_single_approval(msg)
   } else if typed_data.is_some() {
      let typed = typed_data.as_ref().unwrap();
      format_typed_data(typed)
   } else {
      // Try JSON first
      let mut raw = msg.msg_value().to_string();
      // Strip outer quotes
      if raw.starts_with('"') && raw.ends_with('"') {
         raw = raw[1..raw.len() - 1].to_string();
      }

      match serde_json::from_str::<Value>(&raw) {
         Ok(value) if value.is_object() => match to_string_pretty(&value) {
            Ok(pretty) => pretty,
            Err(e) => {
               tracing::error!(
                  "Pretty JSON conversion failed, returning raw {}",
                  e
               );
               raw
            }
         },
         _ => {
            // Try as escaped plain string (personal_sign)
            let unescaped = unescape_ethereum_message(&raw);
            if unescaped.starts_with("\u{19}Ethereum Signed Message:\n") {
               format_plain_message(unescaped)
            } else if raw.starts_with("0x") {
               // Hex: line-break chunks
               let mut hex_formatted = String::new();
               for chunk in raw.as_bytes().chunks(66) {
                  writeln!(
                     hex_formatted,
                     "{}",
                     std::str::from_utf8(chunk).unwrap_or(&raw)
                  )
                  .unwrap();
               }
               hex_formatted
            } else {
               // Plain text: wrap
               textwrap::fill(&raw, 80)
            }
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
   writeln!(formatted, "  Name: {}", "Uniswap Permit2").unwrap();
   writeln!(formatted, "  Version: 2").unwrap();
   writeln!(
      formatted,
      "  Chain: {} (Ethereum)",
      details.token.chain_id
   )
   .unwrap();
   writeln!(
      formatted,
      "  Verifying Contract: {}",
      "Uniswap Protocol: Permit2".to_string()
   )
   .unwrap();
   writeln!(formatted).unwrap();

   // Message details
   writeln!(formatted, "Message:").unwrap();
   writeln!(
      formatted,
      "  Token: {} ({})",
      details.token.symbol, details.token.address
   )
   .unwrap();
   writeln!(
      formatted,
      "  Amount: {}",
      details.amount.wei().to_string()
   )
   .unwrap();
   writeln!(
      formatted,
      "  Expiration: {}",
      format_expiry(details.expiration)
   )
   .unwrap();
   writeln!(
      formatted,
      "  Spender: {}",
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

/// Basic unescaper for Ethereum personal_sign payloads.
/// Handles common escapes: \n, \r, \t, \uXXXX (unicode).
/// Assumes input is a string with literal escape sequences (e.g., "\\n", "\\u0019").
fn unescape_ethereum_message(raw: &str) -> String {
   // This function remains unchanged
   let mut result = String::new();
   let mut i = 0;
   while i < raw.len() {
      if raw.as_bytes()[i] == b'\\' {
         i += 1;
         if i >= raw.len() {
            result.push('\\');
            break;
         }
         match raw.as_bytes()[i] as char {
            'n' => {
               result.push('\n');
               i += 1;
            }
            'r' => {
               result.push('\r');
               i += 1;
            }
            't' => {
               result.push('\t');
               i += 1;
            }
            'u' => {
               // Parse \uXXXX
               i += 1;
               if i + 4 <= raw.len() {
                  let hex_str = &raw[i..i + 4];
                  if let Ok(code) = u32::from_str_radix(hex_str, 16) {
                     if let Some(ch) = char::from_u32(code) {
                        result.push(ch);
                     } else {
                        // Invalid unicode: push as-is
                        result.push_str("\\u");
                        result.push_str(hex_str);
                     }
                  } else {
                     result.push_str("\\u");
                     result.push_str(hex_str);
                  }
                  i += 4;
                  continue;
               } else {
                  result.push_str("\\u");
               }
            }
            c => {
               result.push('\\');
               result.push(c);
               i += 1;
            }
         }
      } else {
         result.push(raw.as_bytes()[i] as char);
         i += 1;
      }
   }
   result
}

/// Formats plain messages (e.g., personal_sign) for readability.
/// Detects Ethereum prefix and structures it with sections.
fn format_plain_message(unescaped: String) -> String {
   let mut formatted = String::new();
   let lines: Vec<&str> = unescaped.split('\n').collect();
   // Detect and handle prefix
   if let Some(first_line) = lines.first() {
      if first_line.starts_with("\u{19}Ethereum Signed Message:") {
         // NEW: Properly parse and skip the length to extract the clean message
         let prefix = "\u{19}Ethereum Signed Message:\n";
         let prefix_len = prefix.len();
         let remaining = &unescaped[prefix_len..];
         // Parse length (sequence of digits)
         let mut len_str = String::new();
         let mut bytes_skipped = 0;
         for ch in remaining.chars() {
            if ch.is_ascii_digit() {
               len_str.push(ch);
               bytes_skipped += ch.len_utf8();
            } else {
               break;
            }
         }
         let msg_len: usize = match len_str.parse() {
            Ok(l) => l,
            Err(_) => {
               tracing::error!("Failed to parse message length");
               return textwrap::fill(&unescaped, 80);
            }
         };
         let msg_start = prefix_len + bytes_skipped;
         if unescaped.len() - msg_start != msg_len {
            tracing::error!("Message length mismatch");
            return textwrap::fill(&unescaped, 80);
         }
         let message = &unescaped[msg_start..];
         // Now process the clean message
         let msg_lines: Vec<&str> = message.split('\n').collect();
         // First line: App + action
         if let Some(app_line) = msg_lines.first() {
            writeln!(formatted, "{}", app_line.trim()).unwrap();
            writeln!(formatted).unwrap();
         }
         // Params: URI, Version, etc. – treat as key: value
         let param_re = Regex::new(r"(URI|Version|Chain ID|Nonce|Issued At): (.*)").unwrap();
         let mut in_params = false;
         for line in msg_lines.iter().skip(1) {
            if param_re.is_match(line) {
               in_params = true;
               let caps = param_re.captures(line).unwrap();
               let key = caps.get(1).unwrap().as_str();
               let value = caps.get(2).unwrap().as_str();
               writeln!(formatted, "{}: {}", key, value).unwrap();
            } else if line.trim().is_empty() {
               if in_params {
                  break; // End of params
               }
            } else {
               // Fallback: just add line
               writeln!(formatted, "{}", line.trim()).unwrap();
            }
         }
         return formatted;
      }
   }
   // Fallback: Simple multiline with wrapping
   textwrap::fill(&unescaped, 80)
}
