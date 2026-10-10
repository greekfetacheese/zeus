use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::{
   GUI,
   ui::{common::wallet_identity, dapps::railgun::RailgunMode},
};
use eframe::egui::{Id, Order, RichText, ScrollArea, Ui, vec2};
use egui::{Align, FontId, Layout, Margin, Shadow, Stroke};
use egui_elements::{Button, Frame as Frame2, Label, Modal, SecureTextEdit, Theme};
use egui_lucide::Lucide;
use std::sync::Arc;

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let privacy_mode = ctx.privacy_mode;
   let chain_id = ctx.chain.id();
   let icons = gui.icons.clone();
   let theme = &gui.theme;

   // The nav sits directly under the account panel: this is the whole space between them, and
   // the panel's own body height is the rest of the separation. Connecting a dApp grows the
   // panel and pushes the nav down, riding the panel's height animation.
   ui.spacing_mut().item_spacing.y = theme.spacing.md;

   gui.account_panel.show(ctx, theme, icons, ui);

   ui.vertical(|ui| {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.xs);

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
         .visuals(visuals)
         .corner_radius(0);

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
         gui.settings.open(ctx);
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

            gui.shield_ui.close();
            gui.approvals.close();
         }
      }
   });
}

pub struct ConnectedDappsUi {
   open: bool,
   pub size: (f32, f32),
}

impl ConnectedDappsUi {
   pub fn new() -> Self {
      Self {
         open: false,
         size: (450.0, 400.0),
      }
   }

   pub fn open(&mut self) {
      self.open = true;
   }
   pub fn close(&mut self) {
      self.open = false;
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let mut open = self.open;
      let chain_id = ctx.chain.id();

      let title = RichText::new("Connected Dapps").size(theme.typography.heading);
      let id = Id::new("connected_dapps_window");

      Modal::new(id, &mut open)
         .backdrop_order(Order::Middle)
         .content_order(Order::Foreground)
         .heading(title)
         .header_separator(false)
         .center_header(true)
         .closable(true)
         .max_width(self.size.0)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing.xl;
            ui.spacing_mut().button_padding = theme.button_padding;
            ui.set_max_width(self.size.0);
            ui.set_max_height(self.size.1);

            let dapps = ctx.connected_dapps();
            let dapps_are_empty = dapps.is_empty();

            let small = theme.typography.small;
            let normal = theme.typography.normal;

            ui.scope(|ui| {
               ui.vertical_centered(|ui| {
                  if dapps_are_empty {
                     ui.label(RichText::new("No connected dapps").size(normal));
                     return;
                  }
               });
            });

            if !dapps_are_empty {
               let text = RichText::new("Disconnect all").size(normal);
               let button = Button::new(text);
               if ui.add(button).clicked() {
                  ctx.disconnect_all_dapps();
               }
            }

            ScrollArea::vertical().content_margin(5).auto_shrink([false; 2]).show(ui, |ui| {
               for dapp in dapps.iter() {
                  let account = ctx.dapp_account(dapp);

                  theme.frame2.show(ui, |ui| {
                     ui.set_min_width(ui.available_width());
                     ui.spacing_mut().item_spacing.y = theme.spacing.sm;

                     ui.horizontal(|ui| {
                        // Disconnect goes first in a right-to-left pass so the origin
                        // field takes exactly the width left over. Sizing the field
                        // from the row instead of its own content is what keeps a long
                        // origin from widening the card — and with it the modal, whose
                        // centered title is measured against the intended width.
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                           let text = RichText::new("Disconnect").size(normal);
                           let button = Button::new(text);
                           if ui.add(button).clicked() {
                              ctx.disconnect_dapp(dapp);
                           }

                           let edit_margin = Margin::same(10);
                           let inner = (ui.available_width() - edit_margin.sum().x).max(24.0);

                           let mut origin = dapp.clone();
                           let edit = SecureTextEdit::singleline(&mut origin)
                              .desired_width(inner)
                              // Without this the field grows with its text.
                              .clip_text(true)
                              .margin(edit_margin)
                              .font(FontId::proportional(normal));

                           ui.add(edit).on_hover_text(RichText::new(dapp).size(small));
                        });
                     });

                     // The account this app was given. Without it there is no way
                     // to tell which account belongs to which app once the app's
                     // account is no longer the selected one.
                     if let Some(account) = account {
                        ui.add(wallet_identity(
                           ctx,
                           chain_id,
                           account,
                           theme,
                           icons.clone(),
                        ));
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
