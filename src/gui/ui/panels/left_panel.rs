use eframe::egui::{Color32, Ui};

use crate::gui::{
   GUI,
   ui::{button, rich_text},
};
use egui_theme::utils;

pub fn show(ui: &mut Ui, gui: &mut GUI) {
   ui.vertical_centered(|ui| {
      ui.add_space(20.0);
      ui.spacing_mut().item_spacing.y = 30.0;
      ui.visuals_mut().widgets.hovered.expansion = 15.0;
      ui.visuals_mut().widgets.active.expansion = 15.0;

      utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
      utils::bg_color_on_hover(ui, gui.theme.colors.widget_bg_color_idle);
      utils::bg_color_on_click(ui, gui.theme.colors.widget_bg_color_click);
      utils::no_border_on_click(ui);

      let home = button(rich_text("Home").size(21.0));
      if ui.add(home).clicked() {
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.portofolio.open = true;
      }

      let wallets = button(rich_text("Wallets").size(21.0));
      if ui.add(wallets).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = true;
      }

      let swap = button(rich_text("Swap").size(21.0));
      if ui.add(swap).clicked() {
         gui.portofolio.open = false;
         gui.send_crypto.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.swap_ui.open = true;
      }

      let send = button(rich_text("Send").size(21.0));
      if ui.add(send).clicked() {
         gui.swap_ui.open = false;
         gui.portofolio.open = false;
         gui.settings.open = false;
         gui.wallet_ui.open = false;
         gui.send_crypto.open = true;
      }

      let settings = button(rich_text("Settings").size(21.0));
      if ui.add(settings).clicked() {
         gui.portofolio.open = false;
         gui.swap_ui.open = false;
         gui.send_crypto.open = false;
         gui.wallet_ui.open = false;
         gui.settings.open = true;
      }

      if ui
         .add(button(rich_text("Theme Editor").size(20.0)))
         .clicked()
      {
         gui.editor.open = true;
      }

      /*
      if ui.add(button(rich_text("Data Insp").size(20.0))).clicked() {
          gui.data_inspection = true;
      }
       */

      // show_data_insp(gui, ui);
   });
}

/* 
#[allow(dead_code)]
fn show_data_insp(gui: &mut GUI, ui: &mut Ui) {
   let mut open = gui.data_inspection;

   Window::new("Data Inspection")
      .open(&mut open)
      .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
      .scroll([false, true])
      .frame(Frame::window(ui.style()))
      .show(ui.ctx(), |ui| {
         ui.set_width(400.0);
         ui.set_height(400.0);
         ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 10.0;

            let ctx = gui.ctx.clone();
            let v2_pools = ctx.read(|ctx| ctx.pool_manager.v2_pools()).into_values();
            let v3_pools = ctx.read(|ctx| ctx.pool_manager.v3_pools()).into_values();

            ui.label(rich_text("V2 Pools"));

            for pool in v2_pools {
               let (price0, price1) = if pool.base_token().address == pool.token0.address {
                  (pool.base_usd, pool.quote_usd)
               } else {
                  (pool.quote_usd, pool.base_usd)
               };

               let chain = ChainId::new(pool.chain_id).unwrap();
               ui.label(rich_text(format!(
                  "Pair: {}-{}",
                  pool.token0.symbol, pool.token1.symbol
               )));
               ui.label(rich_text(format!(
                  "{} ${} - {} ${}",
                  pool.token0.symbol, price0, pool.token1.symbol, price1
               )));
               ui.label(rich_text(format!(
                  "Pool Address: {}",
                  pool.address.to_string()
               )));
               ui.label(rich_text(format!("DEX: {}", pool.dex.to_str())));
               ui.label(rich_text(format!("Chain: {}", chain.name())));
            }

            ui.label(rich_text("V3 Pools"));
            for pool in v3_pools {
               let price0 = ctx.get_token_price(&pool.token0).float();
               let price1 = ctx.get_token_price(&pool.token1).float();
               let chain = ChainId::new(pool.chain_id).unwrap();
               ui.label(rich_text(format!(
                  "Pair: {}-{}",
                  pool.token0.symbol, pool.token1.symbol
               )));
               ui.label(rich_text(format!(
                  "{} ${} - {} ${}",
                  pool.token0.symbol, price0, pool.token1.symbol, price1
               )));
               ui.label(rich_text(format!(
                  "Pool Address: {}",
                  pool.address.to_string()
               )));
               ui.label(rich_text(format!("DEX: {}", pool.dex.to_str())));
               ui.label(rich_text(format!("Chain: {}", chain.name())));
            }
         });
      });

   gui.data_inspection = open;
}
*/