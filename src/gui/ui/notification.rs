use crate::assets::Icons;
use crate::utils::RT;
use crate::core::{DecodedEvent, TransactionRich, transaction::*};
use crate::gui::{SHARED_GUI, ui::GREEN_CHECK};
use egui::{Align2, Button, ProgressBar, RichText, Spinner, Ui, Window, vec2};
use zeus_theme::Theme;
use egui_widgets::{Label, MultiLabel};

use std::{
   sync::Arc,
   time::{SystemTime, UNIX_EPOCH},
};

use zeus_eth::{
   alloy_primitives::U256,
   currency::{Currency, ERC20Token, NativeCurrency},
   types::ChainId,
};

#[derive(Clone)]
pub enum NotificationType {
   Swap(SwapParams),

   Bridge(BridgeParams),

   WrapETH(WrapETHParams),

   UnwrapWETH(UnwrapWETHParams),

   Transfer(TransferParams),

   TokenApproval(TokenApproveParams),

   Other(String),
}

impl NotificationType {
   pub fn from_main_event(main_event: DecodedEvent) -> Self {
      match main_event {
         DecodedEvent::Bridge(params) => Self::Bridge(params),
         DecodedEvent::SwapToken(params) => Self::Swap(params),
         DecodedEvent::Transfer(params) => Self::Transfer(params),
         DecodedEvent::TokenApprove(params) => Self::TokenApproval(params),
         DecodedEvent::WrapETH(params) => Self::WrapETH(params),
         DecodedEvent::UnwrapWETH(params) => Self::UnwrapWETH(params),
         DecodedEvent::UniswapPositionOperation(_params) => Self::Other(String::new()),
         DecodedEvent::EOADelegate(_params) => Self::Other(String::new()),
         DecodedEvent::Permit(_params) => Self::Other(String::new()),
         DecodedEvent::Other => Self::Other("Transaction".to_string()),
      }
   }

   pub fn is_swap(&self) -> bool {
      matches!(self, NotificationType::Swap { .. })
   }

   pub fn is_bridge(&self) -> bool {
      matches!(self, NotificationType::Bridge { .. })
   }

   pub fn is_wrap(&self) -> bool {
      matches!(self, NotificationType::WrapETH { .. })
   }

   pub fn is_unwrap(&self) -> bool {
      matches!(self, NotificationType::UnwrapWETH { .. })
   }

   pub fn is_transfer(&self) -> bool {
      matches!(self, NotificationType::Transfer { .. })
   }

   pub fn is_token_approval(&self) -> bool {
      matches!(self, NotificationType::TokenApproval { .. })
   }

   pub fn is_other(&self) -> bool {
      matches!(self, NotificationType::Other { .. })
   }

   pub fn swap_params(&self) -> &SwapParams {
      match self {
         NotificationType::Swap(params) => params,
         _ => panic!("NotificationType is not a swap"),
      }
   }

   pub fn bridge_params(&self) -> &BridgeParams {
      match self {
         NotificationType::Bridge(params) => params,
         _ => panic!("NotificationType is not a bridge"),
      }
   }

   pub fn wrap_eth_params(&self) -> &WrapETHParams {
      match self {
         NotificationType::WrapETH(params) => params,
         _ => panic!("NotificationType is not a wrap eth"),
      }
   }

   pub fn unwrap_weth_params(&self) -> &UnwrapWETHParams {
      match self {
         NotificationType::UnwrapWETH(params) => params,
         _ => panic!("NotificationType is not a unwrap weth"),
      }
   }

   pub fn transfer_params(&self) -> &TransferParams {
      match self {
         NotificationType::Transfer(params) => params,
         _ => panic!("NotificationType is not a transfer"),
      }
   }

   pub fn token_approval_params(&self) -> &TokenApproveParams {
      match self {
         NotificationType::TokenApproval(params) => params,
         _ => panic!("NotificationType is not a token approval"),
      }
   }

   pub fn other_params(&self) -> String {
      match self {
         NotificationType::Other(text) => text.clone(),
         _ => panic!("NotificationType is not Other"),
      }
   }
}

/// A notification that appears at the top of the screen.
pub struct Notification {
   open: bool,
   with_progress_bar: bool,
   // UNIX timestamp in seconds of when the notification must be started
   start_on: u64,
   // UNIX timestamp in seconds of when the notification must be closed
   finish_on: u64,
   title: String,
   notification: NotificationType,
   tx: Option<TransactionRich>,
   size: (f32, f32),
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
         size: (350.0, 100.0),
      }
   }

   /// Open this [Notification] with a progress bar that when finished it will automatically close
   /// the notification
   ///
   /// Used to show a finished event
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
   }

   /// Open this [Notification] with a spinner, it does not automatically close
   ///
   /// Can be used as an alternative to a loading window
   pub fn open_with_spinner(&mut self, title: String, notification: NotificationType) {
      self.open = true;
      self.with_progress_bar = false;
      self.title = title;
      self.notification = notification;
   }

   pub fn reset(&mut self) {
      *self = Self::new();
   }

   pub fn show(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      if !self.open {
         return;
      }

      let frame = theme.frame1;

      Window::new("notification_window")
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .anchor(Align2::CENTER_CENTER, vec2(0.0, -310.0))
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 10.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);
            ui.set_max_width(self.size.0);
            ui.set_max_height(self.size.1);

            if self.with_progress_bar {
               self.show_with_progress_bar(theme, icons.clone(), ui);
            } else {
               self.show_with_spinner(theme, icons, ui);
            }
         });
   }

   fn show_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      match &self.notification {
         NotificationType::Bridge(_) => {
            self.show_bridge_nofitication(theme, icons.clone(), ui);
         }
         NotificationType::Swap(_) => {
            self.show_swap_notification(theme, icons.clone(), ui);
         }
         NotificationType::Transfer(_) => {
            self.show_transfer_notification(theme, icons, ui);
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

         NotificationType::Other(_) => {
            // Do nothing, just show the title
         }
      }
   }

   fn show_with_progress_bar(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let bar_width = self.size.0 / 2.0;
      let bar_color = theme.colors.text;

      let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
      let start = (self.start_on as u128) * 1000u128;
      let finish = (self.finish_on as u128) * 1000u128;
      let elapsed = now.saturating_sub(start);
      let total = finish.saturating_sub(start);

      let progress: f32 = if total == 0 {
         1.0
      } else {
         (elapsed as f64 / total as f64).min(1.0) as f32
      };

      if progress >= 1.0 {
         self.reset();
      }

      ui.vertical_centered(|ui| {
         let text = format!("{}{}", &self.title, GREEN_CHECK);
         ui.label(RichText::new(text).size(theme.text_sizes.large));
         self.show_notification(theme, icons, ui);

         let text = RichText::new("Transaction Details").size(theme.text_sizes.normal);
         let button = Button::new(text);

         if ui.add_enabled(self.tx.is_some(), button).clicked() {
            let tx = self.tx.take();
            RT.spawn_blocking(move || {
               SHARED_GUI.write(|gui| {
                  gui.tx_window.open(tx);
               });
            });
         }

         ui.add(
            ProgressBar::new(progress)
               .animate(true)
               .fill(bar_color)
               .desired_width(bar_width)
               .desired_height(8.0),
         );
      });
   }

   fn show_with_spinner(&mut self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let spinner_color = theme.colors.text;

      ui.vertical_centered(|ui| {
         ui.label(RichText::new(&self.title).size(theme.text_sizes.large));
         self.show_notification(theme, icons, ui);

         ui.add(Spinner::new().color(spinner_color).size(20.0));
      });
   }

   fn show_swap_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.swap_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let symbol_in = params.input_currency.symbol();
         let amount_in = params.amount_in.abbreviated();

         let symbol_out = params.output_currency.symbol();
         let amount_out = params.received.abbreviated();

         let text_in = format!("{} {}", amount_in, symbol_in);
         let text_in = RichText::new(text_in).size(theme.text_sizes.large);
         let icon_in = icons.currency_icon_x24(&params.input_currency, tint);

         let arrow = icons.arrow_right_white_x24(tint);

         let text_out = format!("{} {}", amount_out, symbol_out);
         let text_out = RichText::new(text_out).size(theme.text_sizes.large);
         let icon_out = icons.currency_icon_x24(&params.output_currency, tint);

         let label_in = Label::new(text_in, Some(icon_in)).wrap();
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0);
         let label_out = Label::new(text_out, Some(icon_out)).wrap();

         let multi_label = MultiLabel::new(vec![label_in, label_arrow, label_out]);
         ui.add(multi_label);
      });
   }

   fn show_bridge_nofitication(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.bridge_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let from_chain: ChainId = params.origin_chain.into();
         let symbol_in = params.input_currency.symbol();
         let amount_in = params.amount.abbreviated();

         let to_chain: ChainId = params.destination_chain.into();
         let symbol_out = params.output_currency.symbol();
         let amount_out = params.received.abbreviated();

         let text_in = format!("{} {}", amount_in, symbol_in);
         let text_in = RichText::new(text_in).size(theme.text_sizes.large);
         let icon_in = icons.currency_icon_x24(&params.input_currency, tint);

         let arrow = icons.arrow_right_white_x24(tint);

         let text_out = format!("{} {}", amount_out, symbol_out);
         let text_out = RichText::new(text_out).size(theme.text_sizes.large);
         let icon_out = icons.currency_icon_x24(&params.output_currency, tint);

         let label_in = Label::new(text_in, Some(icon_in)).wrap();
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0);
         let label_out = Label::new(text_out, Some(icon_out)).wrap();

         let multi_label = MultiLabel::new(vec![label_in, label_arrow, label_out]);
         ui.add(multi_label);

         let chain_in = RichText::new(from_chain.name()).size(theme.text_sizes.large);
         let chain_in_icon = icons.chain_icon(from_chain.id(), tint);
         let label1 = Label::new(chain_in, Some(chain_in_icon));

         let arrow = icons.arrow_right_white_x24(tint);
         let label_arrow = Label::new("", Some(arrow)).spacing(0.0);

         let chain_out = RichText::new(to_chain.name()).size(theme.text_sizes.large);
         let chain_out_icon = icons.chain_icon(to_chain.id(), tint);
         let label2 = Label::new(chain_out, Some(chain_out_icon));

         let multi_label = MultiLabel::new(vec![label1, label_arrow, label2]);
         ui.add(multi_label);
      });
   }

   fn show_wrap_eth_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.wrap_eth_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let native: Currency = NativeCurrency::from(params.chain).into();
         let weth: Currency = ERC20Token::wrapped_native_token(params.chain).into();
         let eth_wrapped = params.eth_wrapped.abbreviated();
         let weth_received = params.weth_received.abbreviated();

         let text = format!("{} {}", eth_wrapped, native.symbol());
         let text_amount = RichText::new(text).size(theme.text_sizes.large);
         let icon = icons.currency_icon_x24(&native, tint);
         let label1 = Label::new(text_amount, Some(icon));

         let arrow_icon = icons.arrow_right_white_x24(tint);
         let arrow_label = Label::new("", Some(arrow_icon)).spacing(0.0);

         let text = format!("{} {}", weth_received, weth.symbol());
         let text_amount = RichText::new(text).size(theme.text_sizes.large);
         let icon = icons.currency_icon_x24(&weth, tint);

         let label2 = Label::new(text_amount, Some(icon));

         let multi_label = MultiLabel::new(vec![label1, arrow_label, label2]);
         ui.add(multi_label);
      });
   }

   fn show_unwrap_weth_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.unwrap_weth_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let weth: Currency = ERC20Token::wrapped_native_token(params.chain).into();
         let native: Currency = NativeCurrency::from(params.chain).into();
         let weth_unwrapped = params.weth_unwrapped.abbreviated();
         let eth_received = params.eth_received.abbreviated();

         let text = format!("{} {}", weth_unwrapped, weth.symbol());
         let text_amount = RichText::new(text).size(theme.text_sizes.large);
         let icon = icons.currency_icon_x24(&weth, tint);
         let label1 = Label::new(text_amount, Some(icon));

         let arrow_icon = icons.arrow_right_white_x24(tint);
         let arrow_label = Label::new("", Some(arrow_icon)).spacing(0.0);

         let text = format!("{} {}", eth_received, native.symbol());
         let text_amount = RichText::new(text).size(theme.text_sizes.large);
         let icon = icons.currency_icon_x24(&native, tint);

         let label2 = Label::new(text_amount, Some(icon));

         let multi_label = MultiLabel::new(vec![label1, arrow_label, label2]);
         ui.add(multi_label);
      });
   }

   fn show_transfer_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.transfer_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let currency = &params.currency;
         let amount = if let Some(amount) = &params.real_amount_sent {
            amount.abbreviated()
         } else {
            params.amount.abbreviated()
         };

         let text = format!("{} {}", amount, currency.symbol());
         let text = RichText::new(text).size(theme.text_sizes.large);
         let icon = icons.currency_icon_x24(&currency, tint);

         let label = Label::new(text, Some(icon)).wrap();
         ui.add(label);

         let text = RichText::new(&params.sender_str).size(theme.text_sizes.normal);
         let from_label = Label::new(text, None);

         let arrow = icons.arrow_right_white_x24(tint);
         let arrow_label = Label::new("", Some(arrow)).spacing(0.0);

         let text = RichText::new(&params.recipient_str).size(theme.text_sizes.normal);
         let to_label = Label::new(text, None);

         let multi_label = MultiLabel::new(vec![from_label, arrow_label, to_label]);
         ui.add(multi_label);
      });
   }

   fn show_token_approval_notification(&self, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      let params = self.notification.token_approval_params();
      let tint = theme.image_tint_recommended;

      ui.vertical_centered(|ui| {
         let token_details =
            params.token.iter().zip(params.amount.iter()).zip(params.amount_usd.iter());

         for ((token, amount), amount_usd) in token_details {
            let is_unlimited = amount.wei() == U256::MAX;
            let amount = if is_unlimited {
               "Unlimited".to_string()
            } else {
               amount.abbreviated()
            };

            let show_usd_value = !is_unlimited && amount_usd.is_some();

            let icon = icons.currency_icon(&Currency::from(token.clone()), tint);
            let text = if show_usd_value {
               let amount_usd = amount_usd.as_ref().unwrap();
               RichText::new(format!(
                  "{} {} ~ ${}",
                  amount,
                  token.symbol,
                  amount_usd.abbreviated()
               ))
               .size(theme.text_sizes.normal)
            } else {
               RichText::new(format!("{} {}", amount, token.symbol)).size(theme.text_sizes.normal)
            };

            let label = Label::new(text, Some(icon)).image_on_left();
            ui.add(label);
         }
      });
   }
}
