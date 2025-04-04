use crate::assets::icons::Icons;
use crate::core::ZeusCtx;
use crate::gui::GUI;
use eframe::egui::{Align2, Button, Color32, Frame, RichText, ScrollArea, Ui, Window};
use egui_theme::{Theme, utils};
use std::sync::Arc;
use zeus_eth::{amm::UniswapV2Pool, types::ChainId};

#[cfg(feature = "dev")]
use std::time::Duration;
#[cfg(feature = "dev")]
use crate::core::utils::RT;
#[cfg(feature = "dev")]
use crate::gui::SHARED_GUI;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   let ctx = gui.ctx.clone();
   ui.vertical_centered(|ui| {
      ui.add_space(20.0);
      ui.spacing_mut().item_spacing.y = 30.0;
      ui.visuals_mut().widgets.hovered.expansion = 15.0;
      ui.visuals_mut().widgets.active.expansion = 15.0;

      utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
      utils::bg_color_on_hover(ui, gui.theme.colors.widget_bg_color);
      utils::bg_color_on_click(ui, gui.theme.colors.widget_bg_color_click);
      utils::no_border_on_click(ui);

      let home = Button::new(RichText::new("Home").size(21.0));
      if ui.add(home).clicked() {
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.portofolio.open = true;
      }

      let send = Button::new(RichText::new("Send").size(21.0));
      if ui.add(send).clicked() {
         gui.swap_ui.open = false;
         gui.portofolio.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.send_crypto.open = true;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();

         let chain = gui.send_crypto.chain_select.chain.id();
         let fee = ctx
            .get_priority_fee(chain)
            .unwrap_or_default()
            .formatted()
            .clone();
         gui.send_crypto.priority_fee = fee;
      }

      let bridge = Button::new(RichText::new("Bridge").size(21.0));
      if ui.add(bridge).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.across_bridge.open = true;

         let chain = gui.send_crypto.chain_select.chain.id();
         let fee = ctx
            .get_priority_fee(chain)
            .unwrap_or_default()
            .formatted()
            .clone();
         gui.across_bridge.priority_fee = fee;
      }

      let wallets = Button::new(RichText::new("Wallets").size(21.0));
      if ui.add(wallets).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.wallet_ui.open = true;
      }

      let tx_history = Button::new(RichText::new("Transactions").size(21.0));
      if ui.add(tx_history).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.across_bridge.open = false;
         gui.tx_history.open = true;
      }

      #[cfg(feature = "dev")]
      {
         let swap = Button::new(RichText::new("Swap").size(21.0));
         if ui.add(swap).clicked() {
            gui.portofolio.open = false;
            gui.send_crypto.open = false;
            gui.settings.open = false;
            gui.wallet_ui.open = false;
            gui.tx_history.open = false;
            gui.across_bridge.open = false;
            gui.swap_ui.open = true;
         }
      }

      let settings = Button::new(RichText::new("Settings").size(21.0));
      if ui.add(settings).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.wallet_ui.open = false;
         gui.tx_history.open = false;
         gui.across_bridge.open = false;
         gui.settings.open = true;
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(RichText::new("Theme Editor").size(20.0)))
         .clicked()
      {
         gui.editor.open = true;
      }

      #[cfg(feature = "dev")]
      if ui
         .add(Button::new(RichText::new("Inspect Pool Data").size(20.0)))
         .clicked()
      {
         gui.data_inspection = true;
      }

      #[cfg(feature = "dev")]
      show_data_insp(gui, ui);

     #[cfg(feature = "dev")]
      {
         let test_locks_button = ui.add(Button::new(RichText::new("Test Locks").size(20.0)));
         if test_locks_button.clicked() {
            gui.testing_window.open = true;
            RT.spawn(async move {
               test_locks(ctx).await;
            });
         }
      }
   });
}

#[cfg(feature = "dev")]
async fn test_locks(ctx: ZeusCtx) {
   for i in 0..1001 {
      // do nothing just aquire the lock
      ctx.write(|_ctx| {});
      SHARED_GUI.write(|gui| {
         gui.testing_window.msg1 = format!("Aquared Lock On GUI and Context: {}", i);
      });
   }
   tokio::time::sleep(Duration::from_secs(1)).await;

   for i in 0..1001 {
      ctx.write(|_ctx| {});
      SHARED_GUI.write(|gui| {
         gui.testing_window.msg2 = format!("Aquared Lock On GUI and Context: {}", i);
      });
   }
   tokio::time::sleep(Duration::from_secs(1)).await;

   for i in 0..1001 {
      ctx.write(|_ctx| {});
      SHARED_GUI.write(|gui| {
         gui.testing_window.msg3 = format!("Aquared Lock On GUI and Context: {}", i);
      });
   }

   loop {
      tokio::time::sleep(Duration::from_millis(50)).await;
      let panic = SHARED_GUI.read(|gui| gui.testing_window.panic);
      if panic {
         panic!("Panic Button Pressed!");
      }
   }
}

#[allow(dead_code)]
fn show_data_insp(gui: &mut GUI, ui: &mut Ui) {
   let mut open = gui.data_inspection;
   let theme = &gui.theme;
   let icons = gui.icons.clone();

   Window::new("Data Inspection")
      .open(&mut open)
      .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
      .frame(Frame::window(ui.style()))
      .show(ui.ctx(), |ui| {
         ui.set_width(600.0);
         ui.set_height(600.0);
         ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 20.0;

            let ctx = gui.ctx.clone();
            let v2_pools = ctx.read(|ctx| ctx.pool_manager.v2_pools()).into_values();
            let _v3_pools = ctx.read(|ctx| ctx.pool_manager.v3_pools()).into_values();

            ScrollArea::vertical().show(ui, |ui| {
               ui.label(RichText::new(format!("V2 Pools {}", v2_pools.len())).size(theme.text_sizes.large));
               for pool in v2_pools {
                  v2_pool_info(ctx.clone(), theme, icons.clone(), &pool, ui);
               }
            });
         });
      });

   gui.data_inspection = open;
}

#[allow(dead_code)]
fn v2_pool_info(ctx: ZeusCtx, theme: &Theme, _icons: Arc<Icons>, pool: &UniswapV2Pool, ui: &mut Ui) {
   let frame = theme.frame1;

   frame.show(ui, |ui| {
      ui.set_width(300.0);
      ui.set_height(150.0);
      ui.spacing_mut().item_spacing.y = 10.0;
      ui.spacing_mut().item_spacing.x = 5.0;

      let chain = ChainId::new(pool.chain_id).unwrap();

      ui.vertical(|ui| {
         ui.horizontal(|ui| {
            ui.label(RichText::new("Token0:").size(theme.text_sizes.normal));
            ui.label(RichText::new(&pool.token0.symbol).size(theme.text_sizes.normal));
         });

         ui.horizontal(|ui| {
            ui.label(RichText::new("Token1:").size(theme.text_sizes.normal));
            ui.label(RichText::new(&pool.token1.symbol).size(theme.text_sizes.normal));
         });

         ui.horizontal(|ui| {
            ui.label(RichText::new("Chain:").size(theme.text_sizes.normal));
            ui.label(RichText::new(chain.name()).size(theme.text_sizes.normal));
         });

         ui.horizontal(|ui| {
            ui.label(RichText::new("Dex:").size(theme.text_sizes.normal));
            ui.label(RichText::new(pool.dex.to_str()).size(theme.text_sizes.normal));
         });

         ui.horizontal(|ui| {
            let exp_link = chain.block_explorer();
            let link = format!("{}/address/{}", exp_link, pool.address);
            ui.label(RichText::new("Address:").size(theme.text_sizes.normal));
            ui.add(egui::Hyperlink::from_label_and_url(
               RichText::new(&pool.address.to_string()).size(theme.text_sizes.small),
               link,
            ));
         });

         let base = pool.base_token();
         let quote = pool.quote_token();
         let base_usd = ctx.get_token_price(base);

         if let Some(base_usd) = base_usd {
            let quote_usd = pool.quote_price(base_usd.f64()).unwrap_or_default();

            ui.horizontal(|ui| {
               ui.label(
                  RichText::new(format!("{} ${}", base.symbol, base_usd.formatted())).size(theme.text_sizes.normal),
               );
            });

            ui.horizontal(|ui| {
               ui.label(RichText::new(format!("{} ${}", quote.symbol, quote_usd)).size(theme.text_sizes.normal));
            });
         } else {
            ui.label(
               RichText::new("Base Token Price not found")
                  .size(theme.text_sizes.small)
                  .color(Color32::RED),
            );
         }
      });
   });
}
