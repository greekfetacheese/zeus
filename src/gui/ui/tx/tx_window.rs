use egui::{Align2, Order, RichText, ScrollArea, Spinner, Stroke, Ui, vec2};
use egui_elements::{Button, OverlayManager, Theme, widgets::Window};

use super::{
   address, chain, clear_display_ui, eth_received, events::*, show_calldata_modal, tx_cost,
   tx_hash, value,
};
use crate::assets::icons::Icons;
use crate::core::clear_signing;
use crate::core::{TransactionRich, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::utils::RT;
use zeus_eth::types::ChainId;

use std::sync::Arc;

/// A window to show details for a transaction that has been sent to the network
pub struct TxWindow {
   open: bool,
   loading: bool,
   overlay: OverlayManager,
   decoded_events: DecodedEvents,
   tx: Option<TransactionRich>,
   show_calldata: bool,
   size: (f32, f32),
}

impl TxWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         loading: false,
         overlay: overlay.clone(),
         decoded_events: DecodedEvents::new(overlay),
         tx: None,
         show_calldata: false,
         size: (550.0, 400.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
      self.tx = None;
      self.show_calldata = false;
   }

   /// Show this [TxWindow]
   pub fn open(&mut self, tx: Option<TransactionRich>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.show_calldata = false;
      self.tx = tx;
      self.open = true;
      self.maybe_fill_clear_display();
   }

   fn maybe_fill_clear_display(&mut self) {
      let Some(tx) = self.tx.as_ref() else {
         return;
      };

      if !tx.main_event.is_other() || tx.clear_display.is_some() {
         return;
      }

      if !tx.analysis.contract_interact || tx.analysis.call_data.len() < 4 {
         return;
      }

      self.loading = true;

      let hash = tx.hash;
      let chain = tx.chain;
      let from = tx.sender();
      let to = tx.interact_to();
      let value = tx.value();
      let calldata = tx.call_data();

      RT.spawn(async move {
         let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
         let display =
            clear_signing::try_clear_sign_calldata(ctx, chain, from, to, value, &calldata).await;

         let Some(display) = display else {
            SHARED_GUI.write(|gui| {
               gui.tx_window.loading = false;
            });
            return;
         };

         SHARED_GUI.write(|gui| {
            if let Some(open_tx) = gui.tx_window.tx.as_mut() {
               if open_tx.hash == hash && open_tx.clear_display.is_none() {
                  open_tx.clear_display = Some(display);
               }
            }
            gui.tx_window.loading = false;
            gui.request_repaint();
         });
      });
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Transaction Details").size(theme.typography.heading);
      let window_frame = theme.window_frame.fill(theme.frame1.fill);
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Middle)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
               ui.spacing_mut().button_padding = theme.button_padding;

               if self.loading {
                  ui.add(Spinner::new().size(20.0).color(theme.colors.text));
                  return;
               }

               let button_visuals = theme.button_visuals();

               ui.add_space(20.0);

               if self.tx.is_none() {
                  ui.label(RichText::new("Transaction not found").size(theme.typography.large));
                  let size = vec2(ui.available_width() * 0.8, 45.0);

                  let text = RichText::new("Close").size(theme.typography.normal);
                  let close_button = Button::new(text).min_size(size).visuals(button_visuals);

                  if ui.add(close_button).clicked() {
                     self.close();
                  }
                  return;
               }

               let tx = self.tx.as_ref().unwrap();
               let chain_id: ChainId = tx.chain.into();

               let frame = theme.frame2;
               let frame_size = vec2(ui.available_width() * 0.95, 45.0);

               self.decoded_events.show(
                  ctx,
                  chain_id,
                  theme,
                  icons.clone(),
                  &tx.analysis,
                  frame_size,
                  frame,
                  self.size,
                  ui,
               );

               let calldata = tx.analysis.call_data.to_string();
               let clear_display = tx.clear_display.clone();
               show_calldata_modal(
                  &mut self.show_calldata,
                  theme,
                  ctx,
                  chain_id,
                  icons.clone(),
                  clear_display.as_ref(),
                  calldata,
                  ui,
               );

               let frame_size = vec2(ui.available_width() * 0.9, 45.0);
               let tx = self.tx.as_ref().unwrap();
               let main_event = &tx.main_event;

               if !main_event.is_other() && tx.success {
                  let frame_size = vec2(ui.available_width() * 0.9, 300.0);

                  ui.label(
                     RichText::new(tx.summary_name()).size(theme.typography.very_large).strong(),
                  );
                  ui.allocate_ui(frame_size, |ui| {
                     ScrollArea::vertical()
                        .content_margin(5)
                        .id_salt("clear_diplay_ui")
                        .max_height(300.0)
                        .show(ui, |ui| {
                           frame.show(ui, |ui| {
                              show_event(
                                 ctx,
                                 chain_id,
                                 theme,
                                 icons.clone(),
                                 main_event,
                                 ui,
                              );
                           });
                        });
                  });
               }

               if main_event.is_other() && tx.success {
                  ui.label(
                     RichText::new(tx.summary_name()).size(theme.typography.very_large).strong(),
                  );

                  if let Some(display) = tx.clear_display.clone() {
                     let display_size = vec2(ui.available_width() * 0.95, 300.0);
                     ui.allocate_ui(display_size, |ui| {
                        frame.show(ui, |ui| {
                           ScrollArea::vertical()
                              .id_salt("tx_window_clear_display")
                              .max_height(300.0)
                              .show(ui, |ui| {
                                 clear_display_ui(
                                    ctx,
                                    chain_id,
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

               if !tx.success {
                  let text = "Transaction failed";
                  ui.label(
                     RichText::new(text).size(theme.typography.large).color(theme.colors.error),
                  );
               }

               ui.allocate_ui(frame_size, |ui| {
                  frame.show(ui, |ui| {
                     chain(chain_id, theme, icons.clone(), ui);

                     if tx.contract_interact {
                        let label = "Contract interaction";
                        address(ctx, chain_id, label, tx.interact_to(), theme, ui);
                     }

                     value(ctx, chain_id, tx.value_sent.clone(), theme, ui);

                     tx_cost(chain_id, &tx.tx_cost, &tx.tx_cost_usd, theme, ui);

                     tx_hash(tx.chain.into(), &tx.hash, theme, ui);
                  });
               });

               // Show ETH received
               if !tx.eth_received.is_zero()
                  && !tx.analysis.is_unwrap_weth()
                  && !tx.analysis.is_swap()
               {
                  let text = "Received";
                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        eth_received(
                           tx.chain,
                           tx.eth_received.clone(),
                           tx.eth_received_usd.clone(),
                           theme,
                           icons.clone(),
                           text,
                           ui,
                        );
                     });
                  });
               }

               let ui_size = vec2(ui.available_width() * 0.6, 45.0);
               ui.allocate_ui(ui_size, |ui| {
                  ui.horizontal(|ui| {
                     ui.spacing_mut().item_spacing.x = theme.spacing.sm;
                     let button_size = vec2(150.0, 30.0);

                     let text = RichText::new("Decoded events").size(theme.typography.large);
                     let button =
                        Button::new(text).visuals(theme.button_visuals()).min_size(button_size);
                     if ui.add(button).clicked() {
                        self.decoded_events.open();
                     }

                     let text = RichText::new("Calldata").size(theme.typography.large);
                     let button = Button::new(text).visuals(button_visuals).min_size(button_size);
                     if ui.add(button).clicked() {
                        self.show_calldata = true;
                     }
                  });
               });

               ui.add_space(30.0);

               let size = vec2(ui.available_width() * 0.8, 45.0);
               let text = RichText::new("Close").size(theme.typography.normal);
               let close_button = Button::new(text).min_size(size).visuals(button_visuals);

               if ui.add(close_button).clicked() {
                  self.close();
               }
            });
         });
   }
}
