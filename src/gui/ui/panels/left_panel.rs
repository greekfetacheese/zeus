use crate::core::ZeusContext;
use crate::gui::{GUI, ui::dapps::railgun::RailgunMode};
use eframe::egui::{Align2, Order, RichText, ScrollArea, Ui, vec2};
use egui::{FontId, Margin, Shadow, Stroke};
use egui_elements::{
   Button, Frame as Frame2, Label, OverlayManager, SecureTextEdit, Theme, widgets::Window,
};
use egui_lucide::Lucide;

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let privacy_mode = ctx.privacy_mode;
   let chain_id = ctx.chain.id();
   let icons = gui.icons.clone();
   let theme = &gui.theme;

   gui.header.show(ctx, theme, icons, ui);

   ui.add_space(10.0);

   ui.vertical(|ui| {
      ui.spacing_mut().item_spacing = vec2(0.0, 4.0);

      let text_size = gui.theme.typography.normal;
      let icon_color = theme.colors.text;
      let mut visuals = theme.frame2_visuals();
      visuals.bg = theme.frame1.fill;
      visuals.border = Stroke::NONE;
      visuals.shadow = Shadow::NONE;

      let is_open = gui.portofolio.is_open();

      let frame = Frame2::from_egui(theme.frame2)
         .interactive(true)
         .fill_width(true)
         .square_corners()
         .visuals(visuals);

      let icon = Lucide::House.size(20.0).color(icon_color).image();
      let home = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(RichText::new("Home").size(text_size), Some(icon))
               .interactive(false)
               .image_on_left(),
         );
      });

      if home.response.clicked() {
         gui.portofolio.open();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.send_crypto.is_open();

      let icon = Lucide::Send.size(20.0).color(icon_color).image();
      let send = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(RichText::new("Send").size(text_size), Some(icon))
               .interactive(false)
               .image_on_left(),
         );
      });

      if send.response.clicked() {
         gui.send_crypto.open();
         gui.send_crypto.default_currency(privacy_mode, chain_id);
         gui.uniswap.close();
         gui.portofolio.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.shield_ui.is_open();
      let title = match privacy_mode {
         false => "Shield",
         true => "Unshield",
      };

      let mode = match privacy_mode {
         false => RailgunMode::Shield,
         true => RailgunMode::Unshield,
      };

      let icon = match privacy_mode {
         false => Lucide::Shield.size(20.0).color(icon_color).image(),
         true => Lucide::ShieldOff.size(20.0).color(icon_color).image(),
      };

      let shield = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(RichText::new(title).size(text_size), Some(icon))
               .interactive(false)
               .image_on_left(),
         );
      });

      if shield.response.clicked() {
         gui.shield_ui.open(mode);
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.approvals.close();
      }

      let is_open = gui.uniswap.is_open();

      let icon = Lucide::RefreshCcwDot.size(20.0).color(icon_color).image();
      let swap = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(RichText::new("Swap").size(text_size), Some(icon))
               .interactive(false)
               .image_on_left(),
         );
      });

      if swap.response.clicked() {
         gui.uniswap.open();
         gui.portofolio.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.across_bridge.is_open();

      let icon = Lucide::SendToBack.size(20.0).color(icon_color).image();
      let bridge = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Bridge").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if bridge.response.clicked() {
         gui.across_bridge.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         // This is shared, so reset it to avoid any issues
         gui.recipient_selection.reset();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.wallet_ui.is_open();

      let icon = Lucide::Wallet.size(20.0).color(icon_color).image();
      let wallets = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Wallets").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if wallets.response.clicked() {
         gui.wallet_ui.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.tx_history.is_open();

      let icon = Lucide::Archive.size(20.0).color(icon_color).image();
      let tx_history = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Transactions").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if tx_history.response.clicked() {
         gui.tx_history.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let is_open = gui.approvals.is_open();

      let icon = Lucide::KeyRound.size(20.0).color(icon_color).image();
      let approvals = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Approvals").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if approvals.response.clicked() {
         gui.approvals.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.settings.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
      }

      let is_open = gui.settings.is_open();

      let icon = Lucide::Settings.size(20.0).color(icon_color).image();
      let settings = frame.selected(is_open).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Settings").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if settings.response.clicked() {
         gui.settings.open();
         gui.portofolio.close();
         gui.uniswap.close();
         gui.send_crypto.close();
         gui.wallet_ui.close();
         gui.tx_history.close(ctx);
         gui.across_bridge.close();
         gui.dev.close();
         gui.shield_ui.close();
         gui.approvals.close();
      }

      let icon = Lucide::Link.size(20.0).color(icon_color).image();
      let connected_dapps = frame.selected(false).show(ui, |ui| {
         ui.add(
            Label::new(
               RichText::new("Connected Dapps").size(text_size),
               Some(icon),
            )
            .interactive(false)
            .image_on_left(),
         );
      });

      if connected_dapps.response.clicked() {
         gui.connected_dapps.open();
      }

      #[cfg(feature = "dev")]
      {
         let text = RichText::new("Theme Editor").size(text_size);
         let theme_editor = frame.selected(false).show(ui, |ui| {
            ui.add(Label::new(text, None).interactive(false));
         });

         if theme_editor.response.clicked() {
            gui.editor.open = true;
         }

         let text = RichText::new("FPS Metrics").size(text_size);
         let fps_metrics = frame.selected(false).show(ui, |ui| {
            ui.add(Label::new(text, None).interactive(false));
         });

         if fps_metrics.response.clicked() {
            gui.fps_metrics.open = true;
         }

         {
            let text = RichText::new("Dev UI").size(text_size);
            let dev = frame.selected(false).show(ui, |ui| {
               ui.add(Label::new(text, None).interactive(false));
            });
            if dev.response.clicked() {
               gui.dev.open();
               gui.portofolio.close();
               gui.uniswap.close();
               gui.send_crypto.close();
               gui.wallet_ui.close();
               gui.tx_history.close(ctx);
               gui.across_bridge.close();
               gui.settings.close();
               gui.shield_ui.close();
               gui.approvals.close();
            }
         }
      }
   });
}

pub struct ConnectedDappsUi {
   open: bool,
   overlay: OverlayManager,
   pub size: (f32, f32),
}

impl ConnectedDappsUi {
   pub fn new(overlay: OverlayManager) -> Self {
      Self {
         open: false,
         overlay,
         size: (300.0, 400.0),
      }
   }

   pub fn open(&mut self) {
      if !self.open {
         self.overlay.window_opened();
      }
      self.open = true;
   }
   pub fn close(&mut self) {
      self.overlay.window_closed();
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();
      let window_frame = theme.window_frame;
      let title_frame = window_frame.stroke(Stroke::NONE);

      let title = RichText::new("Connected Dapps").size(theme.typography.heading);
      Window::new(title)
         .open(&mut open)
         .collapsible(false)
         .resizable(false)
         .order(Order::Middle)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_frame(title_frame)
         .frame(window_frame)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = 20.0;
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.set_width(self.size.0);
            ui.set_height(self.size.1);

            let mut dapps = ctx.connected_dapps();
            let dapps_are_empty = dapps.is_empty();

            ui.scope(|ui| {
               ui.vertical_centered(|ui| {
                  if dapps_are_empty {
                     ui.label(RichText::new("No connected dapps").size(theme.typography.normal));
                     return;
                  }
               });
            });

            if !dapps_are_empty {
               let text = RichText::new("Disconnect all").size(theme.typography.normal);
               let button = Button::new(text).visuals(button_visuals);
               if ui.add(button).clicked() {
                  ctx.disconnect_all_dapps();
               }
            }

            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
               for dapp in dapps.iter_mut() {
                  ui.horizontal(|ui| {
                     let edit = SecureTextEdit::singleline(dapp)
                        .visuals(text_edit_visuals)
                        .min_size(vec2(ui.available_width() * 0.10, 25.0))
                        .margin(Margin::same(10))
                        .font(FontId::proportional(theme.typography.normal));
                     ui.add(edit);

                     let text = RichText::new("Disconnect").size(theme.typography.normal);
                     let button =
                        Button::new(text).visuals(button_visuals).min_size(vec2(50.0, 25.0));
                     if ui.add(button).clicked() {
                        ctx.disconnect_dapp(&dapp);
                     }
                  });
               }
            });
         });

      if !open {
         self.close();
      }
   }
}
