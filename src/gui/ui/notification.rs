//! Notification window that shows up when a tx is beign processed and when its done

use crate::assets::Icons;
use crate::core::{TransactionRich, ZeusContext};
use crate::gui::SHARED_GUI;
use crate::gui::ui::common::address::{
   SpokenAddress, drawn_and_spoken, name_address, transfer_for_speech,
};
use crate::gui::ui::common::{chain_label, currency_for_speech, name_text, token_for_speech};
use crate::gui::ui::token_selection::nft_collection_name;
use crate::utils::TimeStamp;
use egui::{Align2, Order, ProgressBar, Response, RichText, TextWrapMode, Ui, Window, vec2};
use egui_elements::{Button, Label, MultiLabel, Spinner, Theme};
use egui_lucide::Lucide;

use std::sync::Arc;
use std::time::Duration;

use zeus_eth::{
   alloy_primitives::{Address, U256},
   currency::{Currency, ERC20Token, NativeCurrency},
   utils::NumericValue,
};
use zeus_railgun::{abi::railgun::TokenType, caip::AssetId};

/// A known name for an address, or the truncated address when nothing knows it, with the words a
/// screen reader gets for the same address.
fn address_text(
   ctx: &ZeusContext,
   chain: u64,
   address: Address,
   theme: &Theme,
) -> (Label, SpokenAddress) {
   let name = ctx.get_address_name(chain, address);
   let (drawn, spoken) = drawn_and_spoken(name.as_deref(), &address.to_string());

   let label = Label::new(
      RichText::new(drawn).size(theme.typography.normal),
      None,
   )
   .wrap_mode(TextWrapMode::Extend)
   .interactive(false);

   (label, spoken)
}

pub use crate::core::tx::notification::NotificationType;

/// A notification that appears at the top of the screen.
pub struct Notification {
   open: bool,
   with_progress_bar: bool,
   // UNIX timestamp in milliseconds of when the notification must be started
   start_on: u64,
   // UNIX timestamp in milliseconds of when the progress bar is full
   finish_on: u64,
   title: String,
   notification: NotificationType,
   tx: Option<TransactionRich>,
   size: (f32, f32),
   // the one sentence this window says, queued on the frame it first draws
   notice: Option<String>,
   // whether it has already said that sentence, so a transition is never spoken twice
   announced: bool,
   // whether a shortcut asked for the window's first control and it has not taken it yet
   focus_pending: bool,
}

impl Notification {
   pub fn new() -> Self {
      Self {
         open: false,
         with_progress_bar: true,
         start_on: 0,
         finish_on: 0,
         title: String::new(),
         notification: NotificationType::Other(String::new()),
         tx: None,
         size: (500.0, 100.0),
         notice: None,
         announced: false,
         focus_pending: false,
      }
   }

   /// Open this [Notification] with a progress bar that fills once the event is done.
   ///
   /// Used to show a finished event. The window holds for [`HOLD_MS`] after the bar is full — long
   /// enough to hear the sentence and reach Details — and then closes itself, unless the keyboard is
   /// on one of its controls or a dialog is drawn over it.
   pub fn open_with_progress_bar(
      &mut self,
      start_on: u64,
      finish_on: u64,
      title: String,
      notification: NotificationType,
      tx: Option<TransactionRich>,
   ) {
      self.open = true;
      self.with_progress_bar = true;
      self.start_on = start_on;
      self.finish_on = finish_on;
      self.title = title;
      self.notification = notification;
      self.tx = tx;
      self.notice = None;
      self.announced = false;
   }

   /// Open this [Notification] with a spinner, it does not automatically close
   ///
   /// Can be used as an alternative to a loading window
   pub fn open_with_spinner(&mut self, title: String, notification: NotificationType) {
      self.open = true;
      self.with_progress_bar = false;
      self.title = title;
      self.notification = notification;
      self.notice = None;
      self.announced = false;
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   /// The one sentence this window should say, drained by the panel that draws it.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Ask the window's first control to take the focus when it next draws.
   ///
   /// A closed window has nothing to focus, so the request is dropped instead of firing when a
   /// later notification happens to open.
   pub fn request_focus(&mut self) {
      if self.open {
         self.focus_pending = true;
      }
   }

   pub fn show(&mut self, ctx: &ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      // The window speaks for itself, once, on the first frame it is on screen: drawn every frame it
      // would repeat the same sentence, and a window that was never opened says nothing at all.
      if !self.announced {
         self.announced = true;
         self.notice = Some(transition(&self.title, self.with_progress_bar));
      }

      let frame = theme.frame1;

      let avail_width = ui.available_width();

      let (x, y) = if avail_width > 1650.0 {
         (150.0, -400.0)
      } else {
         (110.0, -370.0)
      };

      // egui takes this window's accessible name from its title (egui-0.37.0/src/containers/
      // window.rs:682) and an undecorated window draws no title bar at all, so a human name here
      // costs no pixels. The id egui derives from it is never shown.
      Window::new("Notification")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .order(Order::Background)
         .anchor(Align2::CENTER_CENTER, vec2(x, y))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.sm);
            ui.spacing_mut().button_padding = theme.button_padding;
            ui.set_max_width(self.size.0);
            ui.set_max_height(self.size.1);

            if self.with_progress_bar {
               self.show_with_progress_bar(ctx, theme, icons.clone(), ui);
            } else {
               self.show_with_spinner(ctx, theme, icons, ui);
            }
         });
   }

   fn show_notification(&self, ctx: &ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      match &self.notification {
         NotificationType::Bridge(_) => {
            self.show_bridge_nofitication(theme, icons.clone(), ui);
         }
         NotificationType::Swap(_) => {
            self.show_swap_notification(theme, icons.clone(), ui);
         }
         NotificationType::Transfer(_) => {
            self.show_transfer_notification(ctx, theme, icons, ui);
         }
         NotificationType::NftTransfer(_) => {
            self.show_nft_transfer_notification(ctx, theme, icons, ui);
         }
         NotificationType::NftApproval(_) => {
            self.show_nft_approval_notification(ctx, theme, icons, ui);
         }
         NotificationType::TokenApproval(_) => {
            self.show_token_approval_notification(theme, icons, ui);
         }
         NotificationType::WrapETH(_) => {
            self.show_wrap_eth_notification(theme, icons.clone(), ui);
         }
         NotificationType::UnwrapWETH(_) => {
            self.show_unwrap_weth_notification(theme, icons.clone(), ui);
         }
         NotificationType::Shield(_) => {
            self.show_shield_notification(ctx, theme, icons.clone(), ui);
         }
         NotificationType::Unshield(_) => {
            self.show_unshield_notification(ctx, theme, icons.clone(), ui);
         }
         NotificationType::PrivateTransfer(_) => {
            self.show_private_transfer_notification(ctx, theme, icons.clone(), ui);
         }

         NotificationType::Other(_) => {
            // Do nothing, just show the title
         }
      }
   }

   fn show_with_progress_bar(
      &mut self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let bar_width = self.size.0 / 2.0;
      let bar_color = theme.colors.text;
      // Details is the useful action when there is a transaction behind the window; Dismiss is
      // the only one otherwise.
      let focus = std::mem::take(&mut self.focus_pending);

      let now = TimeStamp::now_as_millis().unwrap_or_default().timestamp();
      let start = self.start_on;
      let finish = self.finish_on;
      let elapsed = now.saturating_sub(start);
      let total = finish.saturating_sub(start);
      let expiry = finish.saturating_add(HOLD_MS);
      let overdue = now >= expiry;

      // The window can only take itself away on a frame, and a full bar stops asking for one.
      if !overdue {
         ui.ctx()
            .request_repaint_after(Duration::from_millis(expiry.saturating_sub(now)));
      }

      // A full bar stays full: it used to reset the whole window, which deleted the only "it was
      // sent" copy while a reader could still be speaking it.
      let progress: f32 = if total == 0 {
         1.0
      } else {
         (elapsed as f64 / total as f64).min(1.0) as f32
      };

      // Whether one of the controls below holds the keyboard: a window must not go from under it.
      let mut held = false;

      ui.vertical_centered(|ui| {
         let icon = Lucide::Check.size(20.0).color(theme.colors.text).image();
         let text = RichText::new(&self.title).size(theme.typography.large);
         let label = Label::new(text, Some(icon)).interactive(false);
         ui.add(label);

         self.show_notification(ctx, theme, icons, ui);

         ui.horizontal(|ui| {
            let text = RichText::new("Transaction Details").size(theme.typography.normal);
            let button = Button::new(text).visuals(theme.button_visuals());

            let details = ui.add_enabled(self.tx.is_some(), button);

            if focus && self.tx.is_some() {
               details.request_focus();
            }

            if details.clicked() {
               let tx = self.tx.take();
               // The dialog is drawn over this notification and hands the focus back here when
               // it closes, so the button that opened it is remembered.
               let opener = details.id;
               crate::runtime::rt().spawn_blocking(move || {
                  SHARED_GUI.write(|gui| {
                     gui.tx_window.open(tx, Some(opener));
                  });
               });
            }

            // The window sets its horizontal item spacing to zero, so the pair is spaced by hand.
            ui.add_space(theme.spacing.sm);

            let dismiss = dismiss_button(theme, ui, focus && self.tx.is_none());
            if dismiss.clicked() {
               self.reset();
            }

            held = details.has_focus() || dismiss.has_focus();
         });

         ui.add(
            ProgressBar::new(progress)
               .animate(true)
               .fill(bar_color)
               .desired_width(bar_width)
               .desired_height(8.0),
         );
      });

      // Take the window away once its hold is over, the way it used to when the bar filled — but
      // never from under the keyboard, and never while a dialog is drawn over it: that dialog hands
      // the focus back here when it closes, and a window that had gone has nothing to hand it to.
      if self.open && overdue && !held {
         let layer = ui.layer_id();

         if ui.ctx().memory(|memory| memory.allows_interaction(layer)) {
            self.reset();
         }
      }
   }

   fn show_with_spinner(
      &mut self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      ui.vertical_centered(|ui| {
         ui.label(RichText::new(&self.title).size(theme.typography.large));
         self.show_notification(ctx, theme, icons, ui);

         ui.add(Spinner::new().size(20.0));

         // Alone on screen this variant had no way out at all: the same Dismiss the mined window has.
         let dismiss = dismiss_button(theme, ui, std::mem::take(&mut self.focus_pending));

         if dismiss.clicked() {
            self.reset();
         }
      });
   }

   fn show_swap_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.swap_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         let symbol_in = params.input_currency.symbol();
         let amount_in = format_amount(&params.amount_in);

         let symbol_out = params.output_currency.symbol();
         let amount_out = format_amount(&params.received);

         let text_in = format!("{} {}", amount_in, symbol_in);
         let text_in = RichText::new(text_in).size(theme.typography.large);
         let icon_in = icons
            .currency_icon_x32(&params.input_currency, tint)
            .fit_to_exact_size(icon_size);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();

         let text_out = format!("{} {}", amount_out, symbol_out);
         let text_out = RichText::new(text_out).size(theme.typography.large);
         let icon_out = icons
            .currency_icon_x32(&params.output_currency, tint)
            .fit_to_exact_size(icon_size);

         let label_in = Label::new(text_in, Some(icon_in))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0).interactive(false);
         let label_out = Label::new(text_out, Some(icon_out))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let multi_label = MultiLabel::new(vec![label_in, label_arrow, label_out]);

         // The line draws two tickers; a reader hears the tokens behind them.
         let response = ui.add(multi_label);
         name_text(
            ui,
            &response,
            format!(
               "{} {} to {} {}",
               amount_in,
               currency_for_speech(&params.input_currency).name,
               amount_out,
               currency_for_speech(&params.output_currency).name
            ),
         );
      });
   }

   fn show_bridge_nofitication(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.bridge_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         // These ids come from a store and are not re-validated, so they are named with the
         // fallible helper rather than converted with the panicking `ChainId::from`. The icons are
         // looked up by raw id, which already falls back on its own.
         let from_chain = chain_label(params.origin_chain);
         let symbol_in = params.input_currency.symbol();
         let amount_in = format_amount(&params.amount);

         let to_chain = chain_label(params.destination_chain);
         let symbol_out = params.output_currency.symbol();
         let amount_out = format_amount(&params.received);

         let text_in = format!("{} {}", amount_in, symbol_in);
         let text_in = RichText::new(text_in).size(theme.typography.large);
         let icon_in = icons
            .currency_icon_x32(&params.input_currency, tint)
            .fit_to_exact_size(icon_size);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();

         let text_out = format!("{} {}", amount_out, symbol_out);
         let text_out = RichText::new(text_out).size(theme.typography.large);
         let icon_out = icons
            .currency_icon_x32(&params.output_currency, tint)
            .fit_to_exact_size(icon_size);

         let label_in = Label::new(text_in, Some(icon_in))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0).interactive(false);
         let label_out = Label::new(text_out, Some(icon_out))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let multi_label = MultiLabel::new(vec![label_in, label_arrow, label_out]);
         let response = ui.add(multi_label);
         name_text(
            ui,
            &response,
            format!(
               "{} {} on {} to {} {} on {}",
               amount_in,
               currency_for_speech(&params.input_currency).name,
               from_chain,
               amount_out,
               currency_for_speech(&params.output_currency).name,
               to_chain
            ),
         );

         let chain_in = RichText::new(&from_chain).size(theme.typography.large);
         let chain_in_icon = icons.chain_icon(params.origin_chain, tint);
         let label1 = Label::new(chain_in, Some(chain_in_icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0).interactive(false);

         let chain_out = RichText::new(&to_chain).size(theme.typography.large);
         let chain_out_icon = icons.chain_icon(params.destination_chain, tint);
         let label2 = Label::new(chain_out, Some(chain_out_icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let multi_label = MultiLabel::new(vec![label1, label_arrow, label2]);
         ui.add(multi_label);
      });
   }

   fn show_wrap_eth_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.wrap_eth_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         let native: Currency = NativeCurrency::from(params.chain).into();
         let weth: Currency = ERC20Token::wrapped_native_token(params.chain).into();
         let eth_wrapped = format_amount(&params.eth_wrapped);
         let weth_received = eth_wrapped.clone();

         let text = format!("{} {}", eth_wrapped, native.symbol());
         let text_amount = RichText::new(text).size(theme.typography.large);
         let icon = icons.currency_icon_x32(&native, tint).fit_to_exact_size(icon_size);
         let label1 = Label::new(text_amount, Some(icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let arrow_icon = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow_icon)).spacing(0.0).interactive(false);

         let text = format!("{} {}", weth_received, weth.symbol());
         let text_amount = RichText::new(text).size(theme.typography.large);
         let icon = icons.currency_icon_x32(&weth, tint).fit_to_exact_size(icon_size);

         let label2 = Label::new(text_amount, Some(icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let multi_label = MultiLabel::new(vec![label1, arrow_label, label2]);
         let response = ui.add(multi_label);
         name_text(
            ui,
            &response,
            format!(
               "{} {} to {} {}",
               eth_wrapped,
               currency_for_speech(&native).name,
               weth_received,
               currency_for_speech(&weth).name
            ),
         );
      });
   }

   fn show_unwrap_weth_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.unwrap_weth_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         let weth: Currency = ERC20Token::wrapped_native_token(params.chain).into();
         let native: Currency = NativeCurrency::from(params.chain).into();
         let weth_unwrapped = format_amount(&params.weth_unwrapped);
         let eth_received = format_amount(&params.eth_received);

         let text = format!("{} {}", weth_unwrapped, weth.symbol());
         let text_amount = RichText::new(text).size(theme.typography.large);
         let icon = icons.currency_icon_x32(&weth, tint).fit_to_exact_size(icon_size);
         let label1 = Label::new(text_amount, Some(icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);

         let arrow_icon = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow_icon)).spacing(0.0).interactive(false);

         let text = format!("{} {}", eth_received, native.symbol());
         let text_amount = RichText::new(text).size(theme.typography.large);
         let icon = icons.currency_icon_x32(&native, tint).fit_to_exact_size(icon_size);

         let label2 = Label::new(text_amount, Some(icon))
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
         let multi_label = MultiLabel::new(vec![label1, arrow_label, label2]);
         let response = ui.add(multi_label);
         name_text(
            ui,
            &response,
            format!(
               "{} {} to {} {}",
               weth_unwrapped,
               currency_for_speech(&weth).name,
               eth_received,
               currency_for_speech(&native).name
            ),
         );
      });
   }

   fn show_transfer_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.transfer_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         let chain = params.currency.chain_id();
         let currency = &params.currency;
         let amount = if let Some(amount) = &params.real_amount_sent {
            format_amount(amount)
         } else {
            format_amount(&params.amount)
         };

         let text = format!("{} {}", amount, currency.symbol());
         let text = RichText::new(text).size(theme.typography.large);
         let icon = icons.currency_icon_x32(currency, tint).fit_to_exact_size(icon_size);

         let label =
            Label::new(text, Some(icon)).wrap_mode(TextWrapMode::Extend).interactive(false);
         let response = ui.add(label);

         // The line draws the ticker; a reader hears the token behind it.
         name_text(
            ui,
            &response,
            format!(
               "{} {}",
               amount,
               currency_for_speech(currency).name
            ),
         );

         let (from_label, from_spoken) = address_text(ctx, chain, params.sender, theme);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow)).spacing(0.0).interactive(false);

         let (to_label, to_spoken) = address_text(ctx, chain, params.recipient, theme);

         let multi_label = MultiLabel::new(vec![from_label, arrow_label, to_label]);
         let response = ui.add(multi_label);

         // The two addresses share one painted row and neither part has a node of its own, so the
         // pair is named as the one line it is drawn as.
         let spoken = transfer_for_speech(&from_spoken, &to_spoken);
         name_address(ui, &response, &spoken);
      });
   }

   /// The body of an NFT transfer, mint, burn or approval: the token, then the same `from → to` line
   /// the fungible transfer notification shows.
   fn show_nft_transfer_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.nft_transfer_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      let collection = nft_collection_name(
         ctx.nft_db.get_collection(params.chain, params.collection).as_ref(),
         params.collection,
      );

      let subject = match params.token_id {
         Some(token_id) => format!("{} #{}", collection, token_id),
         None => collection,
      };

      // No `1 ×` prefix for an ERC-721: the id *is* the thing, and "1 ×" next to it is noise.
      let units = if params.amount > U256::from(1) {
         format!("{} × ", params.amount)
      } else {
         String::new()
      };

      let text = format!("{}{}", units, subject);

      // A transfer always names a token; the fallback id only covers a stored event whose id never
      // made it into the art cache.
      let icon = icons
         .nft_icon_x64(
            params.chain,
            params.collection,
            params.token_id.unwrap_or(U256::ZERO),
            tint,
         )
         .fit_to_exact_size(icon_size);

      ui.vertical_centered(|ui| {
         let label = Label::new(
            RichText::new(text).size(theme.typography.normal),
            Some(icon),
         )
         .wrap_mode(TextWrapMode::Extend)
         .interactive(false);
         ui.add(label);

         let (from_label, from_spoken) = address_text(ctx, params.chain, params.from, theme);
         let (to_label, to_spoken) = address_text(ctx, params.chain, params.to, theme);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow)).spacing(0.0).interactive(false);

         let response = ui.add(MultiLabel::new(vec![
            from_label,
            arrow_label,
            to_label,
         ]));

         let spoken = transfer_for_speech(&from_spoken, &to_spoken);
         name_address(ui, &response, &spoken);
      });
   }

   fn show_nft_approval_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.nft_approval_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      let collection = nft_collection_name(
         ctx.nft_db.get_collection(params.chain, params.collection).as_ref(),
         params.collection,
      );

      // A collection-wide approval has no token id, so the collection is the whole subject; the second
      // line then says what was granted and to whom.
      let subject = match params.token_id {
         Some(token_id) => format!("{} #{}", collection, token_id),
         None => collection,
      };

      let icon = match params.token_id {
         Some(token_id) => icons.nft_icon_x64(params.chain, params.collection, token_id, tint),
         None => icons.nft_collection_icon_x64(params.chain, params.collection, tint),
      }
      .fit_to_exact_size(icon_size);

      ui.vertical_centered(|ui| {
         let label = Label::new(
            RichText::new(subject).size(theme.typography.normal),
            Some(icon),
         )
         .wrap_mode(TextWrapMode::Extend)
         .interactive(false);
         ui.add(label);

         // The same one-line reading the confirmation window gives, minus the collection it already
         // named above: the action, then the operator it grants (or revokes).
         let action = if params.is_revoke() {
            "Revoke"
         } else {
            "Approve"
         };
         let action_label = Label::new(
            RichText::new(action).size(theme.typography.normal),
            None,
         );
         let (operator, operator_spoken) = address_text(ctx, params.chain, params.operator, theme);

         let arrow = Lucide::ArrowRight.size(20.0).color(theme.colors.text).image();
         let arrow_label = Label::new("", Some(arrow)).spacing(0.0).interactive(false);

         let response = ui.add(MultiLabel::new(vec![
            action_label,
            arrow_label,
            operator,
         ]));

         // Only the operator is an address; the action beside it is a word the same row draws and
         // has no node of its own, so the row is named for what a listener can check.
         name_address(ui, &response, &operator_spoken);
      });
   }

   fn show_token_approval_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.token_approval_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         let is_unlimited = params.amount.wei() == U256::MAX;
         let amount = if is_unlimited {
            "Unlimited".to_string()
         } else {
            format_amount(&params.amount)
         };

         let amount_usd = if is_unlimited {
            None
         } else {
            params.amount_usd.as_ref()
         };

         let icon = icons
            .currency_icon_x32(&Currency::from(params.token.clone()), tint)
            .fit_to_exact_size(icon_size);
         let text = if let Some(amount_usd) = amount_usd {
            RichText::new(format!(
               "{} {} ~ ${}",
               amount,
               params.token.symbol,
               format_amount(amount_usd)
            ))
            .size(theme.typography.normal)
         } else {
            RichText::new(format!("{} {}", amount, params.token.symbol))
               .size(theme.typography.normal)
         };

         let label =
            Label::new(text, Some(icon)).wrap_mode(TextWrapMode::Extend).interactive(false);
         let response = ui.add(label);

         // The line draws the ticker; a reader hears the token behind it.
         let name = token_for_speech(&params.token.name, &params.token.symbol).name;
         let spoken = match amount_usd {
            Some(amount_usd) => {
               format!(
                  "{} {} ~ ${}",
                  amount,
                  name,
                  format_amount(amount_usd)
               )
            }
            None => format!("{amount} {name}"),
         };
         name_text(ui, &response, spoken);
      });
   }

   fn show_shield_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.shield_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         if let (Some(token), Some(amount)) = (&params.erc20, &params.amount) {
            let icon = icons
               .token_icon_x32(token.address, token.chain_id, tint)
               .fit_to_exact_size(icon_size);
            let text = if let Some(amount_usd) = &params.amount_usd {
               RichText::new(format!(
                  "{} {} ~ ${}",
                  format_amount(amount),
                  token.symbol,
                  format_amount(amount_usd)
               ))
               .size(theme.typography.normal)
            } else {
               RichText::new(format!(
                  "{} {}",
                  format_amount(amount),
                  token.symbol
               ))
               .size(theme.typography.normal)
            };

            let label = Label::new(text, Some(icon))
               .image_on_left()
               .wrap_mode(TextWrapMode::Extend)
               .interactive(false);
            ui.add(label);
         } else if let AssetId::Erc721(collection, token_id)
         | AssetId::Erc1155(collection, token_id) = &params.asset
         {
            // An NFT shield is reported the same way the fungible one is: the collection's name and art,
            // with the id, instead of leaving the notification with only its title. An ERC-1155 moves a
            // count, so it is shown the way the private transfer shows it.
            let name = nft_collection_name(
               ctx.nft_db.get_collection(params.chain, *collection).as_ref(),
               *collection,
            );

            let icon = icons
               .nft_icon_x64(params.chain, *collection, *token_id, tint)
               .fit_to_exact_size(icon_size);

            let mut text = format!("{} #{}", name, token_id);
            if matches!(params.asset, AssetId::Erc1155(..))
               && let Some(amount) = params.amount.as_ref()
            {
               text.push_str(&format!(" × {}", amount.abbreviated()));
            }

            let label = Label::new(
               RichText::new(text).size(theme.typography.normal),
               Some(icon),
            )
            .image_on_left()
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
            ui.add(label);
         }
      });
   }

   fn show_unshield_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.unshield_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         if let (Some(token), Some(amount)) = (&params.erc20, &params.amount) {
            let icon = icons
               .token_icon_x32(token.address, token.chain_id, tint)
               .fit_to_exact_size(icon_size);
            let text = if let Some(amount_usd) = &params.amount_usd {
               RichText::new(format!(
                  "{} {} ~ ${}",
                  format_amount(amount),
                  token.symbol,
                  format_amount(amount_usd)
               ))
               .size(theme.typography.normal)
            } else {
               RichText::new(format!(
                  "{} {}",
                  format_amount(amount),
                  token.symbol
               ))
               .size(theme.typography.normal)
            };

            let label = Label::new(text, Some(icon))
               .image_on_left()
               .wrap_mode(TextWrapMode::Extend)
               .interactive(false);
            ui.add(label);
         } else if matches!(
            params.token_data.tokenType,
            TokenType::ERC721 | TokenType::ERC1155
         ) {
            // Same as the shield: name the NFT, don't leave the notification with only its title. An
            // ERC-1155 moves a count, so it is shown the way the private transfer shows it.
            let name = nft_collection_name(
               ctx.nft_db.get_collection(params.chain, params.token_data.tokenAddress).as_ref(),
               params.token_data.tokenAddress,
            );

            let icon = icons
               .nft_icon_x64(
                  params.chain,
                  params.token_data.tokenAddress,
                  params.token_data.tokenSubID,
                  tint,
               )
               .fit_to_exact_size(icon_size);

            let mut text = format!("{} #{}", name, params.token_data.tokenSubID);
            if params.token_data.tokenType == TokenType::ERC1155
               && let Some(amount) = params.amount.as_ref()
            {
               text.push_str(&format!(" × {}", amount.abbreviated()));
            }

            let label = Label::new(
               RichText::new(text).size(theme.typography.normal),
               Some(icon),
            )
            .image_on_left()
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
            ui.add(label);
         }
      });
   }

   fn show_private_transfer_notification(
      &self,
      ctx: &ZeusContext,
      theme: &Theme,
      icons: Arc<Icons>,
      ui: &mut Ui,
   ) {
      let params = self.notification.private_transfer_params();
      let tint = theme.image_tint_recommended;
      let icon_size = vec2(24.0, 24.0);

      ui.vertical_centered(|ui| {
         if let (Some(token), Some(amount)) = (params.erc20.as_ref(), params.amount.as_ref()) {
            let icon = icons
               .token_icon_x32(token.address, token.chain_id, tint)
               .fit_to_exact_size(icon_size);
            let text = if let Some(amount_usd) = params.amount_usd.as_ref() {
               RichText::new(format!(
                  "{} {} ~ ${}",
                  format_amount(amount),
                  token.symbol,
                  format_amount(amount_usd)
               ))
               .size(theme.typography.normal)
            } else {
               RichText::new(format!(
                  "{} {}",
                  format_amount(amount),
                  token.symbol
               ))
               .size(theme.typography.normal)
            };

            let label = Label::new(text, Some(icon))
               .image_on_left()
               .wrap_mode(TextWrapMode::Extend)
               .interactive(false);
            ui.add(label);
         } else if let AssetId::Erc721(collection, token_id)
         | AssetId::Erc1155(collection, token_id) = &params.asset
         {
            // The same row shape the confirmation uses: the collection names it and its art stands in for
            // a token icon — plus a count, but only for the standard that has one.
            let name = nft_collection_name(
               ctx.nft_db.get_collection(params.chain, *collection).as_ref(),
               *collection,
            );

            let icon = icons
               .nft_icon_x64(params.chain, *collection, *token_id, tint)
               .fit_to_exact_size(icon_size);

            let mut text = format!("{} #{}", name, token_id);
            if matches!(params.asset, AssetId::Erc1155(..))
               && let Some(amount) = params.amount.as_ref()
            {
               text.push_str(&format!(" × {}", amount.abbreviated()));
            }

            let label = Label::new(
               RichText::new(text).size(theme.typography.normal),
               Some(icon),
            )
            .image_on_left()
            .wrap_mode(TextWrapMode::Extend)
            .interactive(false);
            ui.add(label);
         }
      });
   }
}

/// How long the window goes on after its bar is full. The bar is the confirm wait; the hold is the
/// time a reader gets to finish the sentence in before the window takes itself away.
const HOLD_MS: u64 = 5_000;

/// The title zeus-core gives a pending transaction whose action could not be decoded. It already
/// reads as a sentence about the transaction, so it is not put in front of a second one.
const UNKNOWN_ACTION_TITLE: &str = "Transaction in progress";

/// The one sentence a notification is worth: what finished, or what is running.
fn transition(title: &str, with_progress_bar: bool) -> String {
   if with_progress_bar {
      format!("{title} confirmed.")
   } else if title == UNKNOWN_ACTION_TITLE {
      "Transaction pending.".to_string()
   } else {
      format!("{title} pending.")
   }
}

/// The close control both variants draw. Its node is named by the drawn word, so a reader hears a
/// button called "Dismiss".
fn dismiss_button(theme: &Theme, ui: &mut Ui, focus: bool) -> Response {
   let text = RichText::new("Dismiss").size(theme.typography.normal);
   let button = Button::new(text).visuals(theme.button_visuals());

   let response = ui.add(button);
   if focus {
      response.request_focus();
   }

   response
}

/// Cap displayed amounts at 10 characters (same as AmountField / portfolio).
fn format_amount(amount: &NumericValue) -> String {
   format!("{:.10}", amount.abbreviated())
}

#[cfg(test)]
mod tests {
   use super::*;
   use crate::core::ZeusCtx;
   use egui::accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId};
   use egui::{Context, Event, RawInput, Rect, pos2, vec2};
   use egui_elements::ThemeKind;

   const TITLE: &str = "Transfer";

   /// What a drawn window needs: an AccessKit-enabled context, the installed theme, the icon store,
   /// and a handle with no wallet behind it.
   struct Harness {
      ctx: Context,
      handle: ZeusCtx,
      theme: Theme,
      icons: Arc<Icons>,
   }

   impl Harness {
      fn new() -> Self {
         let ctx = Context::default();
         let mut theme = Theme::new(ThemeKind::TokyoNight);
         theme.install(&ctx);
         egui_extras::install_image_loaders(&ctx);

         let icons = Arc::new(Icons::new(&ctx).expect("icons"));
         let handle = ZeusCtx::new(crate::runtime::test_handle());

         Self {
            ctx,
            handle,
            theme,
            icons,
         }
      }

      /// Run one AccessKit-enabled frame of the window and return that pass's nodes together with
      /// the sentence the window queued for the screen reader during it.
      fn draw_frame(
         &self,
         notification: &mut Notification,
         events: Vec<Event>,
      ) -> (Vec<(NodeId, Node)>, Option<String>) {
         self.draw_frame_with(notification, events, |_| {})
      }

      /// One frame of the window with `before` drawn first, which is how a dialog is put over it.
      fn draw_frame_with(
         &self,
         notification: &mut Notification,
         events: Vec<Event>,
         mut before: impl FnMut(&mut Ui),
      ) -> (Vec<(NodeId, Node)>, Option<String>) {
         self.ctx.enable_accesskit();

         let mut said = None;

         let mut output = self.ctx.run_ui(
            RawInput {
               screen_rect: Some(Rect::from_min_size(
                  pos2(0.0, 0.0),
                  vec2(1290.0, 950.0),
               )),
               events,
               ..Default::default()
            },
            |ui| {
               before(ui);

               self.handle.write(|zeus| {
                  notification.show(zeus, &self.theme, self.icons.clone(), ui);

                  said = notification.take_notice();
               });
            },
         );

         // A pass with nothing to draw produces no nodes, which is what a closed window is.
         let nodes = output
            .platform_output
            .accesskit_update
            .take()
            .map(|update| update.nodes.into_iter().collect::<Vec<_>>())
            .unwrap_or_default();

         output.drop_without_applying_deltas();

         (nodes, said)
      }

      /// Draw the window until its tree has settled and return the nodes of the last pass: a
      /// context's first frame produces no nodes while the window is still being sized.
      fn settled(&self, notification: &mut Notification) -> Vec<(NodeId, Node)> {
         let mut nodes = Vec::new();

         for _ in 0..3 {
            (nodes, _) = self.draw_frame(notification, Vec::new());
         }

         nodes
      }

      /// The name of the control the keyboard is on after this frame.
      fn focused(&self, nodes: &[(NodeId, Node)]) -> Option<String> {
         let id = self.ctx.memory(|memory| memory.focused())?;

         nodes
            .iter()
            .find(|(node_id, _)| *node_id == id.accesskit_id())
            .and_then(|(_, node)| node.label())
            .map(str::to_string)
      }
   }

   fn now_ms() -> u64 {
      TimeStamp::now_as_millis().unwrap_or_default().timestamp()
   }

   /// A mined notification: the progress-bar variant a confirmed transaction shows.
   fn mined(notification: &mut Notification, start_on: u64, finish_on: u64) {
      notification.open_with_progress_bar(
         start_on,
         finish_on,
         TITLE.to_string(),
         NotificationType::Other(String::new()),
         None,
      );
   }

   /// Every name and description in the tree, for a failure message.
   fn spoken(nodes: &[(NodeId, Node)]) -> Vec<String> {
      nodes
         .iter()
         .flat_map(|(_, node)| [node.label(), node.value(), node.description()])
         .flatten()
         .map(str::to_string)
         .collect()
   }

   /// The id of the node a reader finds by this name.
   fn node_named(nodes: &[(NodeId, Node)], name: &str) -> NodeId {
      nodes
         .iter()
         .find(|(_, node)| node.label() == Some(name) || node.value() == Some(name))
         .map(|(id, _)| *id)
         .unwrap_or_else(|| panic!("no node is named {name:?}: {:?}", spoken(nodes)))
   }

   /// A bar that has just filled is still on screen: the hold after it is what a reader finishes the
   /// sentence in, and what leaves them time to reach Details.
   #[test]
   fn a_filled_bar_holds_the_window_for_a_moment() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(
         &mut notification,
         now.saturating_sub(20_000),
         now.saturating_sub(HOLD_MS / 2),
      );

      let nodes = harness.settled(&mut notification);

      assert!(
         notification.open,
         "the hold after the bar fills must keep the window up"
      );
      node_named(&nodes, "Dismiss");
      node_named(&nodes, "Transaction Details");
   }

   /// Then it takes itself away, the way it did when the bar filled: a window that Tab cannot reach
   /// must not stay over the page for ever.
   #[test]
   fn a_filled_bar_takes_the_window_away_once_the_hold_is_over() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(
         &mut notification,
         now.saturating_sub(30_000),
         now.saturating_sub(HOLD_MS + 1_000),
      );

      let nodes = harness.settled(&mut notification);

      assert!(
         !notification.open,
         "a window nobody dismissed must not stay up for ever"
      );
      assert!(
         !spoken(&nodes).iter().any(|text| text == "Dismiss"),
         "a closed window draws nothing: {:?}",
         spoken(&nodes)
      );
   }

   /// The shortcut that asks for this window (Alt+N, and the F6 status region) has to land on one of
   /// its controls: a reader cannot Tab to it, because it is drawn before the page.
   #[test]
   fn the_notification_shortcut_lands_on_its_first_control() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(&mut notification, now, now.saturating_add(60_000));

      harness.settled(&mut notification);

      notification.request_focus();

      let nodes = harness.settled(&mut notification);

      assert_eq!(
         harness.focused(&nodes).as_deref(),
         Some("Dismiss"),
         "the shortcut must land on a control of the window"
      );
   }

   /// A window that goes while the keyboard is on it takes the reader's place away with it.
   #[test]
   fn a_window_that_holds_the_focus_is_not_taken_away() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(&mut notification, now, now.saturating_add(60_000));

      harness.settled(&mut notification);

      notification.request_focus();

      let nodes = harness.settled(&mut notification);
      assert_eq!(
         harness.focused(&nodes).as_deref(),
         Some("Dismiss"),
         "the shortcut must land on one of the window's controls"
      );

      // The bar fills and the hold runs out while the reader is still on that control.
      notification.finish_on = now.saturating_sub(60_000);

      harness.settled(&mut notification);

      assert!(
         notification.open,
         "the window must not vanish from under the keyboard"
      );
   }

   /// A dialog drawn over the window hands the focus back to it when it closes, so the window waits
   /// for the dialog instead of going while the reader is in it.
   #[test]
   fn a_dialog_over_the_window_keeps_it_up() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(&mut notification, now, now.saturating_add(60_000));

      let mut dialog = |ui: &mut Ui| {
         egui::Modal::new(egui::Id::unique("a dialog")).show(ui.ctx(), |ui| {
            ui.label("Details");
         });
      };

      // The dialog goes up first; the bar fills and the hold runs out underneath it.
      for _ in 0..2 {
         harness.draw_frame_with(&mut notification, Vec::new(), &mut dialog);
      }

      notification.finish_on = now.saturating_sub(60_000);

      harness.draw_frame_with(&mut notification, Vec::new(), &mut dialog);

      assert!(
         notification.open,
         "a window under a dialog must wait for it to close"
      );
   }

   /// Dismiss is driven the way a reader drives it: an AccessKit click on the node the previous frame
   /// named "Dismiss", not a pointer event at a rectangle.
   #[test]
   fn the_dismiss_button_closes_the_window() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(&mut notification, now, now.saturating_add(60_000));

      let nodes = harness.settled(&mut notification);
      let dismiss = node_named(&nodes, "Dismiss");

      let click = Event::AccessKitActionRequest(ActionRequest {
         action: Action::Click,
         target_tree: TreeId::ROOT,
         target_node: dismiss,
         data: None,
      });

      harness.draw_frame(&mut notification, vec![click]);
      assert!(
         !notification.open,
         "the Dismiss button must reset the notification"
      );

      let nodes = harness.settled(&mut notification);
      assert!(
         !spoken(&nodes).iter().any(|text| text == "Dismiss"),
         "a closed window draws nothing: {:?}",
         spoken(&nodes)
      );
   }

   /// The spinner variant had no close control at all and never closed by itself, so the same
   /// Dismiss is the way out of it too.
   #[test]
   fn the_spinner_variant_has_a_way_out() {
      let harness = Harness::new();
      let mut notification = Notification::new();
      notification.open_with_spinner(
         TITLE.to_string(),
         NotificationType::Other(String::new()),
      );

      let nodes = harness.settled(&mut notification);
      let dismiss = node_named(&nodes, "Dismiss");

      let click = Event::AccessKitActionRequest(ActionRequest {
         action: Action::Click,
         target_tree: TreeId::ROOT,
         target_node: dismiss,
         data: None,
      });

      harness.draw_frame(&mut notification, vec![click]);
      assert!(
         !notification.open,
         "the spinner's Dismiss must close it too"
      );
   }

   /// The transition is said once, in the tense of the variant, and only while the window is up.
   #[test]
   fn the_transition_is_said_once() {
      let harness = Harness::new();

      let mut confirmed = Notification::new();
      let now = now_ms();
      mined(&mut confirmed, now, now.saturating_add(60_000));

      let (_, said) = harness.draw_frame(&mut confirmed, Vec::new());
      assert_eq!(said.as_deref(), Some("Transfer confirmed."));

      let (_, said) = harness.draw_frame(&mut confirmed, Vec::new());
      assert_eq!(
         said, None,
         "a notification says its transition once"
      );

      let mut pending = Notification::new();
      pending.open_with_spinner(
         TITLE.to_string(),
         NotificationType::Other(String::new()),
      );

      let (_, said) = harness.draw_frame(&mut pending, Vec::new());
      assert_eq!(said.as_deref(), Some("Transfer pending."));

      let (_, said) = harness.draw_frame(&mut pending, Vec::new());
      assert_eq!(
         said, None,
         "and the pending sentence is said once too"
      );

      // The unknown-action title is already a sentence, so it is not put in front of a second one.
      let mut unknown = Notification::new();
      unknown.open_with_spinner(
         UNKNOWN_ACTION_TITLE.to_string(),
         NotificationType::Other(String::new()),
      );

      let (_, said) = harness.draw_frame(&mut unknown, Vec::new());
      assert_eq!(said.as_deref(), Some("Transaction pending."));

      // A window that was never opened says nothing.
      let mut closed = Notification::new();

      let (_, said) = harness.draw_frame(&mut closed, Vec::new());
      assert_eq!(said, None, "a closed window says nothing");
   }

   /// The window's own node is named for people: egui labels it from the window title, which this
   /// undecorated window never draws, so the id it used to carry is gone from the tree and not a
   /// pixel moved.
   #[test]
   fn the_window_node_is_named_notification() {
      let harness = Harness::new();
      let mut notification = Notification::new();

      let now = now_ms();
      mined(&mut notification, now, now.saturating_add(60_000));

      let nodes = harness.settled(&mut notification);

      let node = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Window)
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "the window must be in the tree: {:?}",
               spoken(&nodes)
            )
         });

      assert_eq!(
         node.label(),
         Some("Notification"),
         "the window is named for people"
      );
      assert!(
         !spoken(&nodes).iter().any(|text| text.contains("notification_window")),
         "a reader must never be given the window id: {:?}",
         spoken(&nodes)
      );
   }
}
