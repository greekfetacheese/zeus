use egui::{Align2, Frame, Margin, Order, RichText, Ui, Window, vec2};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::Button;

use super::{chain, contract_interact, eth_received, events::*, tx_cost, tx_hash, value};
use crate::assets::icons::Icons;
use crate::core::{TransactionRich, ZeusContext};
use zeus_eth::types::ChainId;

use std::sync::Arc;

/// A window to show details for a transaction that has been sent to the network
pub struct TxWindow {
   open: bool,
   overlay: OverlayManager,
   decoded_events: DecodedEvents,
   tx: Option<TransactionRich>,
   size: (f32, f32),
}

impl TxWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay: overlay.clone(),
         decoded_events: DecodedEvents::new(overlay),
         tx: None,
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
   }

   /// Show this [TxWindow]
   pub fn open(&mut self, tx: Option<TransactionRich>) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.tx = tx;
      self.open = true;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Transaction Details").size(theme.text_sizes.heading);
      let window_frame = theme.frame1;

      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Foreground)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let button_visuals = theme.button_visuals();

                  ui.add_space(20.0);

                  if self.tx.is_none() {
                     ui.label(RichText::new("Transaction not found").size(theme.text_sizes.large));
                     let size = vec2(ui.available_width() * 0.8, 45.0);

                     let text = RichText::new("Close").size(theme.text_sizes.normal);
                     let close_button = Button::new(text).min_size(size).visuals(button_visuals);

                     if ui.add(close_button).clicked() {
                        self.close();
                     }
                     return;
                  }

                  let tx = self.tx.as_ref().unwrap();
                  let main_event = &tx.main_event;
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

                  let frame_size = vec2(ui.available_width() * 0.9, 45.0);

                  if !main_event.is_other() && tx.success {
                     ui.label(
                        RichText::new(main_event.name()).size(theme.text_sizes.very_large).strong(),
                     );
                     ui.allocate_ui(frame_size, |ui| {
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
                  }

                  // Tx Action is unknown
                  if main_event.is_other() && tx.success {
                     let text =
                        RichText::new("Show all decoded events").size(theme.text_sizes.large);
                     let button = Button::new(text).visuals(theme.button_visuals());
                     let clicked = ui.add(button).clicked();
                     if clicked {
                        self.decoded_events.open();
                     }
                  }

                  if !tx.success {
                     let text = "Transaction failed";
                     ui.label(
                        RichText::new(text).size(theme.text_sizes.large).color(theme.colors.error),
                     );
                  }

                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        chain(chain_id, theme, icons.clone(), ui);

                        if tx.contract_interact {
                           contract_interact(ctx, chain_id, tx.interact_to(), theme, ui);
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

                  ui.add_space(30.0);

                  let size = vec2(ui.available_width() * 0.8, 45.0);
                  let text = RichText::new("Close").size(theme.text_sizes.normal);
                  let close_button = Button::new(text).min_size(size).visuals(button_visuals);

                  if ui.add(close_button).clicked() {
                     self.close();
                  }
               });
            });
         });
   }
}
