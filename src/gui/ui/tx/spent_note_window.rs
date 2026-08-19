use std::sync::Arc;

use egui::{Align, Align2, Frame, Layout, Margin, Order, RichText, Stroke, Ui, Window, vec2};
use zeus_eth::{alloy_primitives::U256, currency::Currency, types::ChainId, utils::NumericValue};
use zeus_theme::{OverlayManager, Theme};
use zeus_widgets::Button;

use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::ui::tx::chain;
use crate::utils::TimeStamp;
use zeus_eth::alloy_primitives::Address;
use zeus_railgun::{PrivateHistoryEntry, PrivateHistoryKind};

/// One spent-note row for privacy-mode transaction history.
#[derive(Clone)]
pub struct SpentHistoryRow {
   pub wallet: Address,
   pub chain: u64,
   pub action: String,
   pub spent_block: u64,
   pub spent_timestamp: u64,
   pub entry: PrivateHistoryEntry,
}

/// Details for a spent Railgun note (privacy-mode history).
pub struct SpentNoteWindow {
   open: bool,
   overlay: OverlayManager,
   row: Option<SpentHistoryRow>,
   size: (f32, f32),
}

impl SpentNoteWindow {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         row: None,
         size: (550.0, 460.0),
      }
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn close(&mut self) {
      if self.open {
         self.overlay.window_closed();
      }
      self.open = false;
      self.row = None;
   }

   pub fn open(&mut self, row: SpentHistoryRow) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.row = Some(row);
      self.open = true;
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let title = RichText::new("Spent Note").size(theme.text_sizes.heading);
      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      Window::new(title)
         .resizable(false)
         .collapsible(false)
         .order(Order::Middle)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.set_max_width(self.size.0);
            ui.set_height(self.size.1);

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let button_visuals = theme.button_visuals();

                  let Some(row) = self.row.as_ref() else {
                     ui.label(RichText::new("Spent note not found").size(theme.text_sizes.large));
                     let size = vec2(ui.available_width() * 0.8, 45.0);
                     let text = RichText::new("Close").size(theme.text_sizes.normal);
                     let close_button = Button::new(text).min_size(size).visuals(button_visuals);
                     if ui.add(close_button).clicked() {
                        self.close();
                     }
                     return;
                  };

                  let chain_id: ChainId = row.chain.into();
                  let frame = theme.frame2;
                  let frame_size = vec2(ui.available_width() * 0.9, 45.0);

                  ui.label(
                     RichText::new(match row.entry.kind {
                        PrivateHistoryKind::Merge => "Merged Notes",
                        PrivateHistoryKind::Send => "Private Transfer",
                     })
                     .size(theme.text_sizes.very_large)
                     .strong(),
                  );

                  ui.allocate_ui(frame_size, |ui| {
                     frame.show(ui, |ui| {
                        chain(chain_id, theme, icons.clone(), ui);

                        kv_row(
                           "Wallet",
                           &wallet_label(ctx, row.wallet),
                           theme,
                           ui,
                        );

                        kv_row("Amount", &amount_label(ctx, row), theme, ui);

                        if row.entry.change_amount > 0 {
                           kv_row(
                              "Change returned",
                              &format_amount(
                                 ctx,
                                 row.chain,
                                 row.entry.asset,
                                 row.entry.change_amount,
                              ),
                              theme,
                              ui,
                           );
                        }

                        kv_row(
                           "Notes spent",
                           &row.entry.input_count.to_string(),
                           theme,
                           ui,
                        );

                        if !row.entry.memo.is_empty() {
                           kv_row("Memo", &row.entry.memo, theme, ui);
                        }

                        kv_row("Age", &spent_age(ctx, row), theme, ui);

                        kv_row(
                           "Spent block",
                           &row.spent_block.to_string(),
                           theme,
                           ui,
                        );
                     });
                  });

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

fn wallet_label(ctx: &mut ZeusContext, address: zeus_eth::alloy_primitives::Address) -> String {
   ctx.get_wallet_name(address).unwrap_or_else(|| address.to_string())
}

fn amount_label(ctx: &mut ZeusContext, row: &SpentHistoryRow) -> String {
   format_amount(ctx, row.chain, row.entry.asset, row.entry.amount)
}

fn format_amount(
   ctx: &mut ZeusContext,
   chain: u64,
   asset: zeus_railgun::caip::AssetId,
   amount: u128,
) -> String {
   let (symbol, decimals) = match asset.erc20_address() {
      Some(addr) => ctx
         .currency_db
         .get_erc20_token(chain, addr)
         .map(|t| (t.symbol.to_string(), t.decimals))
         .unwrap_or_else(|| (addr.to_string(), 18)),
      None => (asset.to_string(), 18),
   };
   let amount = NumericValue::format_wei(U256::from(amount), decimals);
   let currency = asset
      .erc20_address()
      .and_then(|addr| ctx.currency_db.get_erc20_token(chain, addr).map(Currency::from));
   if let Some(currency) = currency {
      let usd = ctx.get_currency_value_for_amount(amount.f64(), &currency);
      format!(
         "{} {} ~ ${}",
         amount.abbreviated(),
         symbol,
         usd.abbreviated()
      )
   } else {
      format!("{} {}", amount.abbreviated(), symbol)
   }
}

pub(crate) fn spent_age(_ctx: &ZeusContext, row: &SpentHistoryRow) -> String {
   if row.spent_timestamp > 0 {
      return TimeStamp::Seconds(row.spent_timestamp).to_relative();
   }

   format!("block {}", row.spent_block)
}

fn kv_row(label: &str, value: &str, theme: &Theme, ui: &mut Ui) {
   ui.horizontal(|ui| {
      ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
         ui.label(RichText::new(label).size(theme.text_sizes.large));
      });
      ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
         ui.label(RichText::new(value).size(theme.text_sizes.large).color(theme.colors.text));
      });
   });
}
