use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::{
   GUI,
   ui::{
      common::{address::name_address, wallet_identity},
      dapps::railgun::RailgunMode,
   },
};
use eframe::egui::{Id, Order, RichText, ScrollArea, Ui, vec2};
use egui::{Align, FontId, Layout, Margin, Response, Role, Shadow, Stroke, WidgetInfo};
use egui_elements::{Button, Fill, Frame as Frame2, Label, Modal, SecureTextEdit, Theme};
use egui_lucide::Lucide;
use std::sync::Arc;

pub fn show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let privacy_mode = ctx.privacy_mode;
   let chain_id = ctx.chain.id();
   let icons = gui.icons.clone();
   let theme = &gui.theme;
   let nav_spacing = theme.spacing.xs;

   // The nav sits directly under the account panel: this is the whole space between them, and
   // the panel's own body height is the rest of the separation. Connecting a dApp grows the
   // panel and pushes the nav down, riding the panel's height animation.
   ui.spacing_mut().item_spacing.y = theme.spacing.md;

   gui.account_panel.show(ctx, theme, icons, ui);

   ui.vertical(|ui| {
      ui.spacing_mut().item_spacing = vec2(0.0, nav_spacing);

      let is_open = gui.portofolio.is_open();
      nav_plate(
         gui,
         ctx,
         "Home",
         Some(Lucide::House),
         is_open,
         |gui, ctx| {
            gui.portofolio.open();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.send_crypto.is_open();
      nav_plate(
         gui,
         ctx,
         "Send",
         Some(Lucide::Send),
         is_open,
         |gui, ctx| {
            gui.send_crypto.open();
            gui.send_crypto.default_currency(privacy_mode, chain_id);
            gui.uniswap.close();
            gui.portofolio.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            // This is shared, so reset it to avoid any issues
            gui.recipient_selection.reset();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

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
         false => Lucide::Shield,
         true => Lucide::ShieldOff,
      };

      nav_plate(
         gui,
         ctx,
         title,
         Some(icon),
         is_open,
         |gui, ctx| {
            gui.shield_ui.open(mode);
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            // This is shared, so reset it to avoid any issues
            gui.recipient_selection.reset();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.uniswap.is_open();
      nav_plate(
         gui,
         ctx,
         "Swap",
         Some(Lucide::RefreshCcwDot),
         is_open,
         |gui, ctx| {
            gui.uniswap.open();
            gui.portofolio.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.across_bridge.is_open();
      nav_plate(
         gui,
         ctx,
         "Bridge",
         Some(Lucide::SendToBack),
         is_open,
         |gui, ctx| {
            gui.across_bridge.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            // This is shared, so reset it to avoid any issues
            gui.recipient_selection.reset();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.wallet_ui.is_open();
      nav_plate(
         gui,
         ctx,
         "Wallets",
         Some(Lucide::Wallet),
         is_open,
         |gui, ctx| {
            gui.wallet_ui.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.tx_history.is_open();
      nav_plate(
         gui,
         ctx,
         "Transactions",
         Some(Lucide::Archive),
         is_open,
         |gui, _ctx| {
            gui.tx_history.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );

      let is_open = gui.approvals.is_open();
      nav_plate(
         gui,
         ctx,
         "Approvals",
         Some(Lucide::KeyRound),
         is_open,
         |gui, ctx| {
            gui.approvals.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();

            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();
            #[cfg(feature = "dev")]
            gui.dev.close();
            gui.shield_ui.close();
         },
         ui,
      );

      let is_open = gui.settings.is_open();
      // Read before the plate is drawn: the plate's own click handler runs inside `nav_plate` and
      // may open Settings again, while this request belongs to the close that just happened.
      let focus_return = gui.settings.take_focus_return();
      let plate = nav_plate(
         gui,
         ctx,
         "Settings",
         Some(Lucide::Settings),
         is_open,
         |gui, ctx| {
            gui.settings.open(ctx, &mut gui.appearance_settings);
         },
         ui,
      );

      // A window the user closed must not take the focus with it: the plate that opened it takes
      // the focus back, so the nav stays where the user left it.
      if focus_return {
         plate.request_focus();
      }

      nav_plate(
         gui,
         ctx,
         "Connected Dapps",
         Some(Lucide::Link),
         false,
         |gui, _ctx| {
            gui.connected_dapps.open();
         },
         ui,
      );

      #[cfg(feature = "dev")]
      nav_plate(
         gui,
         ctx,
         "Dev UI",
         None,
         false,
         |gui, ctx| {
            gui.dev.open();
            gui.portofolio.close();
            gui.uniswap.close();
            gui.send_crypto.close();
            gui.wallet_ui.close();
            gui.tx_history.close(ctx);
            gui.across_bridge.close();

            gui.shield_ui.close();
            gui.approvals.close();
         },
         ui,
      );
   });
}

/// One nav entry: a plate that carries the entry's role, name and selected state itself.
///
/// The icon and the word inside are drawn by an `egui_elements::Label`, which is a node of its
/// own; left as-is a reader would hear the entry twice ("Home button, Home"). The label is hidden
/// from the accessibility tree after it is added, so the plate is the only node of the entry.
///
/// Returns the plate's response, so a caller can move the focus onto it.
fn nav_plate(
   gui: &mut GUI,
   ctx: &mut ZeusContext,
   text: &str,
   icon: Option<Lucide>,
   selected: bool,
   on_click: impl FnOnce(&mut GUI, &mut ZeusContext),
   ui: &mut Ui,
) -> Response {
   let theme = &gui.theme;
   let text_size = theme.typography.normal;
   let icon_color = theme.colors.text;

   let mut visuals = theme.frame2_visuals();
   visuals.bg = Fill::Solid(theme.frame1.fill);
   visuals.border = Stroke::NONE;
   visuals.shadow = Shadow::NONE;

   let frame = Frame2::from_egui(theme.frame2)
      .interactive(true)
      .fill_width(true)
      .visuals(visuals)
      .corner_radius(0);

   // `image_on_left` also moves the text right by the label's icon spacing, so it is only
   // applied when there is an icon: the Dev UI entry has none and must stay where it was.
   let has_icon = icon.is_some();
   let icon = icon.map(|icon| icon.size(20.0).color(icon_color).image());
   let label = Label::new(RichText::new(text).size(text_size), icon).interactive(false);
   let label = if has_icon {
      label.image_on_left()
   } else {
      label
   };

   // The frame registers itself with an empty sense before its contents, which withdraws focus
   // from the id it is about to take. A plate reached with Tab would therefore lose focus on the
   // next frame and Space/Enter would never activate it. `show` is exactly `begin` + the
   // contents + `end`, so doing it by hand lets focus be put back before the final registration.
   let focused_before = ui.ctx().memory(|memory| memory.focused());

   let mut plate = frame.selected(selected).begin(ui);
   let id = ui.next_auto_id();

   if focused_before == Some(id) {
      ui.ctx().memory_mut(|memory| memory.request_focus(id));
   }

   let content = plate.content_ui.add(label);
   let response = plate.end(ui);

   response.widget_info(|| WidgetInfo::selected(Role::Button, ui.is_enabled(), selected, text));

   ui.ctx().accesskit_node_builder(content.id, |node| node.set_hidden());

   if response.clicked() {
      on_click(gui, ctx);
   }

   response
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
      let id = Id::unique("connected_dapps_window");

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
                        let (identity, spoken) =
                           wallet_identity(ctx, chain_id, account, theme, icons.clone());

                        let identity = ui.add(identity);
                        name_address(ui, &identity, &spoken);
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
