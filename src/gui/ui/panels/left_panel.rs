use crate::assets::icons::Icons;
use crate::core::ZeusContext;
use crate::gui::{
   FocusRegion, GUI, Page,
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
   let icons = gui.icons.clone();

   // Taken before the theme borrow: the card the focus belongs to is drawn next, and the
   // region request is gone by the time it draws.
   if gui.take_region_focus(FocusRegion::Account) {
      gui.account_panel.focus_first_control();
   }

   let theme = &gui.theme;
   let nav_spacing = theme.spacing.xs;

   // The nav sits directly under the account panel: this is the whole space between them, and
   // the panel's own body height is the rest of the separation. Connecting a dApp grows the
   // panel and pushes the nav down, riding the panel's height animation.
   ui.spacing_mut().item_spacing.y = theme.spacing.md;

   gui.account_panel.show(ctx, theme, icons, ui);

   // The receive window cannot announce from inside its own draw — the frame holds the only
   // channel — so its sentence comes back here. This panel is drawn before the central one, and
   // the announcer the app draws after every panel says it in the same frame.
   if let Some(text) = gui.account_panel.take_notice() {
      gui.announce(text);
   }

   ui.vertical(|ui| {
      ui.spacing_mut().item_spacing = vec2(0.0, nav_spacing);

      let is_open = gui.portofolio.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Home,
         "Home",
         Some(Lucide::House),
         is_open,
         ui,
      );

      let is_open = gui.send_crypto.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Send,
         "Send",
         Some(Lucide::Send),
         is_open,
         ui,
      );

      let is_open = gui.shield_ui.is_open();
      let title = match privacy_mode {
         false => "Shield",
         true => "Unshield",
      };

      let icon = match privacy_mode {
         false => Lucide::Shield,
         true => Lucide::ShieldOff,
      };

      let response = nav_plate(
         gui,
         ctx,
         Page::Shield,
         title,
         Some(icon),
         is_open,
         ui,
      );

      // The plate draws one word, so its description says what that page does.
      let description = match privacy_mode {
         false => "Deposit into the private pool",
         true => "Withdraw from the private pool",
      };

      ui.ctx().accesskit_node_builder(response.id, |node| {
         node.set_description(description)
      });

      let is_open = gui.uniswap.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Swap,
         "Swap",
         Some(Lucide::RefreshCcwDot),
         is_open,
         ui,
      );

      let is_open = gui.across_bridge.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Bridge,
         "Bridge",
         Some(Lucide::SendToBack),
         is_open,
         ui,
      );

      let is_open = gui.wallet_ui.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Wallets,
         "Wallets",
         Some(Lucide::Wallet),
         is_open,
         ui,
      );

      let is_open = gui.tx_history.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Transactions,
         "Transactions",
         Some(Lucide::Archive),
         is_open,
         ui,
      );

      let is_open = gui.approvals.is_open();
      nav_plate(
         gui,
         ctx,
         Page::Approvals,
         "Approvals",
         Some(Lucide::KeyRound),
         is_open,
         ui,
      );

      let is_open = gui.settings.is_open();
      // Read before the plate is drawn: the plate's own click handler runs inside `nav_plate` and
      // may open Settings again, while this request belongs to the close that just happened.
      let focus_return = gui.settings.take_focus_return();
      let plate = nav_plate(
         gui,
         ctx,
         Page::Settings,
         "Settings",
         Some(Lucide::Settings),
         is_open,
         ui,
      );

      // A window the user closed must not take the focus with it: the plate that opened it takes
      // the focus back, so the nav stays where the user left it. The mark that says where the
      // keyboard is stays off it — see `GUI::nav_focus_quiet` — so nothing of the window is left
      // drawn on a plate the user did not reach for.
      if focus_return {
         gui.nav_focus_quiet = true;
         plate.request_focus();
      }

      // Reaching for the keyboard is what puts the mark back: whatever it lands on is a focus the
      // user asked for.
      if gui.nav_focus_quiet
         && ui.input(|input| {
            input.events.iter().any(|event| matches!(event, egui::Event::Key { .. }))
         })
      {
         gui.nav_focus_quiet = false;
      }

      // The plate a window hands the keyboard to must not ignore the pointer either: a focused
      // `Frame` paints its press chrome over the hover colour, so while the focus sits there the
      // entry never lights up under the cursor. The pointer asking for anything ends the
      // hand-back — the plate gives the focus up and its own hover and press chrome come back —
      // while a user who never touches the mouse keeps the keyboard where the window left it.
      if gui.nav_focus_quiet
         && (ui.rect_contains_pointer(ui.max_rect())
            || ui.input(|input| input.pointer.any_pressed()))
      {
         ui.ctx().memory_mut(|memory| memory.surrender_focus(plate.id));
         gui.nav_focus_quiet = false;
      }

      nav_plate(
         gui,
         ctx,
         Page::ConnectedDapps,
         "Connected Dapps",
         Some(Lucide::Link),
         false,
         ui,
      );

      #[cfg(feature = "dev")]
      nav_plate(gui, ctx, Page::Dev, "Dev UI", None, false, ui);
   });
}

/// Open a page, close the page bodies the nav closes when it moves, and give it the keyboard.
///
/// The nav plate and the `Alt` digit shortcuts both call this, so a page opened with a key is the
/// page a pointer click would open: same state, same reset. Settings and Connected Dapps are the
/// two that open over the page in front and close on their own, so they close nothing.
///
/// The page also takes the keyboard, through the same request `F6` sends to it: selecting the page
/// that is already in front is still a choice, and without the request the next `Tab` walks the
/// rest of the side menu while the page sits unfocused.
pub fn open_page(gui: &mut GUI, ctx: &mut ZeusContext, page: Page) {
   if !matches!(page, Page::Settings | Page::ConnectedDapps) {
      close_other_pages(gui, ctx, page);
   }

   let privacy_mode = ctx.privacy_mode;

   match page {
      Page::Home => gui.portofolio.open(),
      Page::Send => {
         gui.send_crypto.open();
         gui.send_crypto.default_currency(privacy_mode, ctx.chain.id());
         // This is shared, so reset it to avoid any issues.
         gui.recipient_selection.reset();
      }
      Page::Shield => {
         let mode = match privacy_mode {
            false => RailgunMode::Shield,
            true => RailgunMode::Unshield,
         };

         gui.shield_ui.open(mode);
         // This is shared, so reset it to avoid any issues.
         gui.recipient_selection.reset();
      }
      Page::Swap => gui.uniswap.open(),
      Page::Bridge => {
         gui.across_bridge.open();
         // This is shared, so reset it to avoid any issues.
         gui.recipient_selection.reset();
      }
      Page::Wallets => gui.wallet_ui.open(),
      Page::Transactions => gui.tx_history.open(),
      Page::Approvals => gui.approvals.open(),
      Page::Settings => gui.settings.open(ctx, &mut gui.appearance_settings),
      Page::ConnectedDapps => gui.connected_dapps.open(),
      Page::Dev => {
         #[cfg(feature = "dev")]
         gui.dev.open();
      }
   }

   // The two surfaces that open *over* the page keep the keyboard they take; every page body
   // hands it to the page, which is where a reader lands next.
   if !gui.settings.is_open() && !gui.connected_dapps.is_open() {
      gui.request_region_focus(FocusRegion::Main);
   }
}

/// Close every page body except the one `keep` is about to open.
fn close_other_pages(gui: &mut GUI, ctx: &mut ZeusContext, keep: Page) {
   if keep != Page::Home {
      gui.portofolio.close();
   }
   if keep != Page::Send {
      gui.send_crypto.close();
   }
   if keep != Page::Shield {
      gui.shield_ui.close();
   }
   if keep != Page::Swap {
      gui.uniswap.close();
   }
   if keep != Page::Bridge {
      gui.across_bridge.close();
   }
   if keep != Page::Wallets {
      gui.wallet_ui.close();
   }
   if keep != Page::Transactions {
      gui.tx_history.close(ctx);
   }
   if keep != Page::Approvals {
      gui.approvals.close();
   }
   #[cfg(feature = "dev")]
   if keep != Page::Dev {
      gui.dev.close();
   }
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
   page: Page,
   text: &str,
   icon: Option<Lucide>,
   selected: bool,
   ui: &mut Ui,
) -> Response {
   let theme = &gui.theme;
   let text_size = theme.typography.normal;
   let icon_color = theme.colors.text;

   let mut visuals = theme.frame2_visuals();
   visuals.bg = Fill::Solid(theme.frame1.fill);
   // A plate that holds the keyboard — the Settings entry after its window closes, or whichever
   // one F6 lands on — must not read as pressed. An interactive `Frame` paints its click fill
   // while it has the focus, and that fill is the window background in every theme: a band
   // darker than the panel, which looks like a press that never ended. The fill stays idle and a
   // one-point accent outline is what says where the keyboard is.
   visuals.bg_click = Fill::Solid(theme.frame1.fill);
   visuals.border_click = match gui.nav_focus_quiet {
      true => Stroke::NONE,
      false => Stroke::new(1.0, theme.colors.accent),
   };
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
      open_page(gui, ctx, page);
   }

   // A region shortcut lands on the plate of the page in front, the way Tab reaches it.
   if page == gui.current_page() && gui.take_region_focus(FocusRegion::Navigation) {
      response.request_focus();
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
                           let response = ui.add(button);

                           // Every connected app draws the same word, so the button is
                           // named after the app it disconnects.
                           response.widget_info(|| {
                              WidgetInfo::labeled(
                                 Role::Button,
                                 ui.is_enabled(),
                                 format!("Disconnect {dapp}"),
                              )
                           });

                           if response.clicked() {
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
