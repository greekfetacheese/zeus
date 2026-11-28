//! UI that allows the user to change the network settings.

use crate::assets::icons::Icons;
use crate::core::{
   ZeusContext, ZeusCtx,
   client::{
      MAX_CONCURRENCY, MAX_RPC_CU_PER_SECOND, MAX_RPC_RPS, MIN_CONCURRENCY, MIN_RPC_CU_PER_SECOND,
      MIN_RPC_RPS, Rpc,
   },
};
use crate::gui::ui::common::section_heading;
use crate::gui::{SHARED_GUI, ui::ChainSelect, ui::show_with_fade};
use crate::utils::state;
use eframe::egui::{
   Align, CornerRadius, CursorIcon, FontId, InnerResponse, Layout, Margin, RichText, Role,
   ScrollArea, Ui, WidgetInfo, vec2,
};
use egui_elements::{
   Button, Checkbox, Indicator, IndicatorState, SecureTextEdit, Spinner, Theme,
   visuals::ButtonVisuals,
};
use egui_lucide::Lucide;
use std::sync::Arc;
use zeus_eth::alloy_provider::Provider;

enum NetworkView {
   List,
   AddRpc,
   EditRpc,
}

/// Why a row's URL is not offered for editing: reading it back could put a provider key in the
/// accessibility tree, and the words the page says instead identify the row by its host.
const RPC_URL_TIP: &str = "Editing it here is not offered to a reader: the URL can contain a provider key and stays masked. Remove the endpoint and add it again instead.";

/// What the concurrency field says when its text is not a number in range.
const CONCURRENCY_ERROR: &str = "Enter a whole number within the range below";

/// What the add-RPC form says while its text is not a URL.
const INVALID_URL_ERROR: &str = "Invalid URL";

/// The margin a row's URL field draws inside its cell.
///
/// It is part of the field's box, so the cell has to give it back: a field sized at the cell's
/// own width overran its column by both margins, and the page then drew wider than the width
/// the window had measured for it — which widened the window a little more on every frame.
const RPC_URL_MARGIN: i8 = 5;

pub struct NetworkSettings {
   view: NetworkView,
   refreshing: bool,
   rpc_to_edit: Option<Rpc>,
   url_to_add: String,
   /// Rate-limit inputs for the endpoint being edited.
   rps_input: String,
   cu_input: String,
   /// Global concurrency input; `None` until seeded from the client on first render.
   concurrency_input: Option<String>,
   /// The inline error already spoken: the page draws these in place of a dialog, so each one is
   /// said once, on the frame it starts showing.
   inline_error_said: Option<&'static str>,
   /// The words the page's last action should say, drained by the window right after it draws.
   notice: Option<String>,
   chain_select: ChainSelect,
}

impl NetworkSettings {
   pub fn new() -> Self {
      let mut chain_select =
         ChainSelect::new("network_settings_chain_select", 1).size(vec2(250.0, 15.0));
      chain_select.show_disabled_chains = true;

      Self {
         view: NetworkView::List,
         refreshing: false,
         rpc_to_edit: None,
         url_to_add: String::new(),
         rps_input: String::new(),
         cu_input: String::new(),
         concurrency_input: None,
         inline_error_said: None,
         notice: None,
         chain_select,
      }
   }

   pub fn reset_view(&mut self) {
      self.view = NetworkView::List;
      self.rpc_to_edit = None;
      self.url_to_add.clear();
      self.rps_input.clear();
      self.cu_input.clear();
      self.concurrency_input = None;
      self.inline_error_said = None;
      self.notice = None;
   }

   pub fn open_add_rpc(&mut self) {
      self.view = NetworkView::AddRpc;
   }

   pub fn close_add_rpc(&mut self) {
      self.view = NetworkView::List;
      self.url_to_add.clear();
   }

   pub fn open_rpc_settings(&mut self, rpc: Rpc) {
      self.view = NetworkView::EditRpc;
      self.rps_input = rpc.rps.to_string();
      self.cu_input = rpc.cu_per_second.to_string();
      self.rpc_to_edit = Some(rpc);
   }

   pub fn close_rpc_settings(&mut self) {
      self.view = NetworkView::List;
      self.rpc_to_edit = None;
      self.rps_input.clear();
      self.cu_input.clear();
   }

   /// The words this page's last action should say, drained by the window right after it draws.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Say an inline error once, on the frame it starts showing.
   ///
   /// These lines are drawn in place of a dialog, so nothing else would say them, and a reader
   /// would only find the error by re-reading the form.
   fn say_inline_error(&mut self, text: &'static str) {
      if self.inline_error_said != Some(text) {
         self.inline_error_said = Some(text);
         self.notice = Some(text.to_string());
      }
   }

   /// Forget an inline error that is no longer drawn, so the next one is said as well.
   fn clear_inline_error(&mut self, text: &'static str) {
      if self.inline_error_said == Some(text) {
         self.inline_error_said = None;
      }
   }

   fn valid_url(&self) -> bool {
      self.url_to_add.starts_with("http://")
         || self.url_to_add.starts_with("https://")
         || self.url_to_add.starts_with("ws://")
         || self.url_to_add.starts_with("wss://")
   }

   fn back_button(&mut self, theme: &Theme, ui: &mut Ui) {
      let text = RichText::new("Back").size(theme.typography.normal);
      let button = Button::new(text).visuals(theme.button_visuals());
      if ui.add(button).clicked() {
         self.reset_view();
      }
   }

   pub fn show(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.add_space(8.0);

      let view = &self.view;
      let is_list = matches!(view, NetworkView::List);
      let is_add = matches!(view, NetworkView::AddRpc);
      let is_edit = matches!(view, NetworkView::EditRpc);

      show_with_fade(ui, "network_list_ui_fade", is_list, |ui| {
         self.list_ui(ctx, theme, icons.clone(), ui);
      });

      show_with_fade(ui, "network_add_rpc_ui_fade", is_add, |ui| {
         self.add_rpc(theme, ui);
      });

      show_with_fade(ui, "network_edit_rpc_ui_fade", is_edit, |ui| {
         self.rpc_settings(ctx, theme, ui);
      });
   }

   fn list_ui(&mut self, ctx: &mut ZeusContext, theme: &Theme, icons: Arc<Icons>, ui: &mut Ui) {
      ui.spacing_mut().button_padding = theme.button_padding;

      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();

      let chain = self.chain_select.chain.id();
      let z_client = ctx.client_manager.clone();
      let mut rpcs = z_client.get_rpcs(chain);

      ui.add_space(10.0);

      ui.horizontal(|ui| {
         ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
            ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);
            self.chain_select.show(ctx, &[0], theme, icons.clone(), ui);
         });

         ui.add_space(30.0);

         ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.sm);

            let disabled = ctx.is_chain_disabled(chain);
            // The drawn word flips with the flag; the name says what a press does now, so a reader
            // never has to work out which chain state the drawn word was written for.
            let (text, name) = match disabled {
               true => (
                  RichText::new("Enable Network").size(theme.typography.normal),
                  "Enable this network",
               ),
               false => (
                  RichText::new("Disable Network").size(theme.typography.normal),
                  "Disable this network",
               ),
            };

            let button = Button::new(text).visuals(button_visuals);
            let toggle = ui.add(button);

            toggle.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), name));

            if toggle.clicked() {
               if disabled {
                  ctx.enable_chain(chain);
               } else {
                  ctx.disable_chain(chain);
               }

               crate::runtime::rt().spawn_blocking(move || {
                  let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                  ctx.save_disabled_chains();
                  post_toggle_network(ctx, chain, disabled);
               });
            }

            let text = RichText::new("Add RPC Url").size(theme.typography.normal);
            let button = Button::new(text).visuals(button_visuals);

            if ui.add(button).clicked() {
               self.open_add_rpc();
            }

            let icon = Lucide::RefreshCw.size(20.0).color(theme.colors.text).image();

            if !self.refreshing {
               let visuals = ButtonVisuals {
                  bg_hover: button_visuals.bg_hover,
                  corner_radius: CornerRadius::same(25),
                  ..Default::default()
               };
               // A picture of a refresh arrow: the text alternative says what it checks.
               let button = Button::image(icon).small().visuals(visuals).alt("Check RPC endpoints");
               let res = ui.add(button).on_hover_cursor(CursorIcon::PointingHand);

               if res.clicked() {
                  self.refreshing = true;

                  crate::runtime::rt().spawn(async move {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     let z_client = ctx.get_client_manager();
                     z_client.run_rpc_checks(ctx.clone()).await;
                     SHARED_GUI.write(|gui| {
                        gui.settings.network.refreshing = false;
                     });
                  });
               }
            } else {
               ui.add(Spinner::new().size(17.0));
            }
         });
      });

      let p = "Zeus supports load-balancing across enabled RPCs.";
      let p2 = "You can keep more than one RPC enabled and Zeus will automatically load-balance requests across them.";
      let text = RichText::new(p).size(theme.typography.small).color(theme.colors.text_muted);
      let text2 = RichText::new(p2).size(theme.typography.small).color(theme.colors.text_muted);

      ui.scope(|ui| {
         ui.spacing_mut().item_spacing.y = theme.spacing.xs;
         ui.label(text);
         ui.label(text2);
      });

      ui.add_space(theme.spacing.md);
      ui.separator();

      let concurrency_hint = format!(
         "How many requests Zeus may run at once (range {MIN_CONCURRENCY}-{MAX_CONCURRENCY}). Raise it for a paid or local RPC, keep it low for public endpoints."
      );

      // Kept above the RPC table: the table's scroll area consumes the remaining height, so
      // anything below it would fall off-screen on a short window.
      ui.allocate_ui_with_layout(
         vec2(ui.available_width(), 32.0),
         Layout::left_to_right(Align::Center),
         |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;
            let label_text = RichText::new("Request Concurrency").size(theme.typography.normal);
            let label = ui.label(label_text);

            let current = z_client.concurrency();
            let input = self.concurrency_input.get_or_insert_with(|| current.to_string());

            let field = ui.add(
               SecureTextEdit::singleline(input)
                  .accessible_name("Request Concurrency")
                  .visuals(text_edit_visuals)
                  .font(FontId::proportional(theme.typography.normal))
                  .desired_width(32.0)
                  .margin(Margin::same(6)),
            );

            // The sentence beside the field names it, and the range drawn below is its
            // description: a reader hears both on the field instead of two orphans.
            ui.ctx().accesskit_node_builder(label.id, |node| node.set_hidden());
            ui.ctx().accesskit_node_builder(field.id, |node| {
               node.set_description(concurrency_hint.as_str());
            });

            let parsed = self
               .concurrency_input
               .as_ref()
               .and_then(|value| value.trim().parse::<usize>().ok());
            let valid =
               parsed.is_some_and(|value| (MIN_CONCURRENCY..=MAX_CONCURRENCY).contains(&value));

            let text = RichText::new("Apply").size(theme.typography.normal);
            let button = Button::new(text).visuals(button_visuals);
            let apply = ui.add_enabled(valid, button);
            let apply_name = "Apply concurrency";

            apply.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), apply_name));

            if apply.clicked()
               && let Some(value) = parsed
            {
               z_client.set_concurrency(value);
               self.concurrency_input = Some(z_client.concurrency().to_string());

               crate::runtime::rt().spawn_blocking(move || {
                  let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                  ctx.save_client_manager();
               });
            }

            if !valid {
               let text = RichText::new(CONCURRENCY_ERROR)
                  .size(theme.typography.small)
                  .color(theme.colors.error);
               ui.label(text);
               self.say_inline_error(CONCURRENCY_ERROR);
            } else {
               self.clear_inline_error(CONCURRENCY_ERROR);
            }
         },
      );

      ui.label(
         RichText::new(concurrency_hint)
            .size(theme.typography.small)
            .color(theme.colors.text_muted),
      );

      ui.separator();

      ui.add_space(12.0);

      // Status columns + actions are fixed; leftover width goes to the URL so
      // Test / X stay on-screen. Fractions of available_width overflowed.
      const COL_SPACING: f32 = 8.0;
      const HEADER_H: f32 = 24.0;
      const ROW_H: f32 = 32.0;
      const ENABLED_W: f32 = 64.0;
      const STATUS_W: f32 = 52.0;
      const ARCHIVE_W: f32 = 60.0;
      const MEV_W: f32 = 88.0;
      // Wide enough for "Not available", the longest reading a latency cell draws. At 64 the
      // word overran its cell by a word's width, which pushed the actions column of every row
      // without a reading to the right of the one row that had it.
      const LATENCY_W: f32 = 88.0;
      const ACTIONS_W: f32 = 148.0;
      const N_GAPS: f32 = 6.0;

      let fixed = ENABLED_W + STATUS_W + ARCHIVE_W + MEV_W + LATENCY_W + ACTIONS_W;
      let url_w = (ui.available_width() - fixed - COL_SPACING * N_GAPS).max(140.0);

      ui.spacing_mut().item_spacing = vec2(COL_SPACING, theme.spacing.sm);
      ui.spacing_mut().button_padding = vec2(theme.spacing.sm, theme.spacing.xs);

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = COL_SPACING;
         rpc_col(ui, url_w, HEADER_H, |ui| {
            ui.label(RichText::new("Url").size(theme.typography.small));
         });
         rpc_col(ui, ENABLED_W, HEADER_H, |ui| {
            ui.label(RichText::new("Enabled").size(theme.typography.small));
         });
         rpc_col(ui, STATUS_W, HEADER_H, |ui| {
            ui.label(RichText::new("Status").size(theme.typography.small));
         });
         rpc_col(ui, ARCHIVE_W, HEADER_H, |ui| {
            ui.label(RichText::new("Archive").size(theme.typography.small));
         });
         rpc_col(ui, MEV_W, HEADER_H, |ui| {
            ui.label(RichText::new("MEV Protect").size(theme.typography.small));
         });
         rpc_col(ui, LATENCY_W, HEADER_H, |ui| {
            ui.label(RichText::new("Latency").size(theme.typography.small));
         });
      });

      // The rows are sized from the width this page is given, so the area they scroll in keeps no
      // horizontal room of its own: a margin on that axis made every row wider than the width the
      // page had been measured for, and the Settings window — which sizes itself from its content
      // — grew by that margin on every frame until it took the whole screen.
      let rows = ScrollArea::vertical()
         .auto_shrink([false; 2])
         .content_margin(Margin::symmetric(0, 5));

      rows.show(ui, |ui| {
         ui.set_width(ui.available_width());
         ui.spacing_mut().item_spacing = vec2(COL_SPACING, theme.spacing.sm);

         for rpc in rpcs.values_mut() {
            ui.horizontal(|ui| {
               ui.spacing_mut().item_spacing.x = COL_SPACING;

               let host = rpc_host(&rpc.url).to_string();
               let mut url = rpc.url.to_string();
               let field = rpc_col(ui, url_w, ROW_H, |ui| {
                  ui.add(
                     SecureTextEdit::singleline(&mut url)
                        .accessible_name(format!("RPC URL at {host}"))
                        .visuals(text_edit_visuals)
                        .font(FontId::proportional(theme.typography.small))
                        .min_size(vec2(
                           url_w - 2.0 * f32::from(RPC_URL_MARGIN),
                           20.0,
                        ))
                        .margin(Margin::same(RPC_URL_MARGIN)),
                  )
               });

               // A URL can carry a provider key, so its text never enters the accessibility tree:
               // the host names the row and the value is cleared before the tree is built.
               ui.ctx().accesskit_node_builder(field.inner.id, |node| {
                  node.set_description(RPC_URL_TIP);
                  node.clear_value();
               });

               let was_enabled = rpc.enabled;
               let box_name = format!("Enabled, {host}");
               let res = rpc_col(ui, ENABLED_W, ROW_H, |ui| {
                  ui.add(Checkbox::without_text(&mut rpc.enabled))
               });

               // The box draws no words, so it is named after the endpoint it turns on or off.
               // The flag is read after the widget ran, so a click frame reports the new state.
               res.inner.widget_info(|| {
                  WidgetInfo::selected(
                     Role::CheckBox,
                     ui.is_enabled(),
                     rpc.enabled,
                     &box_name,
                  )
               });

               if res.inner.clicked() {
                  let z_client = ctx.client_manager.clone();
                  z_client.set_rpc_enabled(chain, &rpc.url, rpc.enabled);

                  if !was_enabled && rpc.enabled {
                     let rpc = rpc.clone();
                     crate::runtime::rt().spawn(async move {
                        let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                        let z_client = ctx.get_client_manager();
                        z_client.run_check_for(ctx.clone(), rpc).await;

                        post_enable_rpc(ctx, chain).await
                     });
                  }

                  crate::runtime::rt().spawn_blocking(move || {
                     let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                     ctx.save_client_manager();
                  });
               }

               rpc_col(ui, STATUS_W, ROW_H, |ui| {
                  ui.add(rpc_status_indicator(rpc));
               });

               rpc_col(ui, ARCHIVE_W, ROW_H, |ui| {
                  ui.add(rpc_archive_indicator(rpc));
               });

               rpc_col(ui, MEV_W, ROW_H, |ui| {
                  ui.add(rpc_mev_indicator(rpc));
               });

               rpc_col(ui, LATENCY_W, ROW_H, |ui| {
                  ui.label(RichText::new(rpc.latency_str()).size(theme.typography.small));
               });

               rpc_col(ui, ACTIONS_W, ROW_H, |ui| {
                  ui.spacing_mut().item_spacing.x = theme.spacing.xs;

                  let icon = Lucide::Settings.size(20.0).color(theme.colors.text).image();
                  let visuals = ButtonVisuals {
                     bg_hover: button_visuals.bg_hover,
                     corner_radius: CornerRadius::same(15),
                     ..Default::default()
                  };
                  // A picture of a gear: the text alternative says whose settings it opens.
                  let settings_name = format!("Endpoint settings for {host}");
                  let settings_btn =
                     Button::image(icon).small().visuals(visuals).alt(settings_name);
                  let settings = ui.add(settings_btn).on_hover_cursor(CursorIcon::PointingHand);

                  if settings.clicked() {
                     self.open_rpc_settings(rpc.clone());
                  }

                  if rpc.test_in_progress {
                     ui.add(Spinner::new().size(14.0));
                  } else {
                     let text = RichText::new("Test").size(theme.typography.normal);
                     let button = Button::new(text).visuals(button_visuals);
                     let test = ui.add(button);
                     let test_name = format!("Test {host}");

                     test.widget_info(|| {
                        WidgetInfo::labeled(Role::Button, ui.is_enabled(), &test_name)
                     });

                     if test.clicked() {
                        let rpc_clone = rpc.clone();
                        crate::runtime::rt().spawn(async move {
                           let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                           let z_client = ctx.get_client_manager();
                           z_client.run_check_for(ctx, rpc_clone).await;
                        });
                     }
                  }

                  ui.add_space(5.0);

                  let button = Button::new(RichText::new("X").size(theme.typography.normal))
                     .visuals(button_visuals);
                  let remove = ui.add(button);
                  let remove_name = format!("Remove {host}");

                  // "X" says nothing aloud about what it deletes.
                  remove.widget_info(|| {
                     WidgetInfo::labeled(Role::Button, ui.is_enabled(), &remove_name)
                  });

                  if remove.clicked() {
                     let z_client = ctx.client_manager.clone();
                     z_client.remove_rpc(chain, rpc.url.clone());

                     crate::runtime::rt().spawn_blocking(move || {
                        let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
                        ctx.save_client_manager();
                     });
                  }
               });
            });
         }
      });
   }

   fn rpc_settings(&mut self, ctx: &mut ZeusContext, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().button_padding = theme.button_padding;
      ui.spacing_mut().item_spacing.y = theme.spacing.md;

      self.back_button(theme, ui);

      section_heading(ui, theme.typography.large, "Endpoint Settings");

      if self.rpc_to_edit.is_none() {
         let text = RichText::new("No RPC selected").size(theme.typography.normal);
         ui.label(text);
         return;
      }

      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();

      {
         let rpc = self.rpc_to_edit.as_mut().unwrap();

         let text = RichText::new("MEV Protect").size(theme.typography.normal);
         let label = ui.label(text);

         // Like the row's boxes, this one draws no words: it takes the words beside it as its own
         // name, and that label leaves the tree, so the box is the one node that says what it sets.
         let checkbox = ui.add(Checkbox::without_text(&mut rpc.mev_protect).name("MEV Protect"));
         ui.ctx().accesskit_node_builder(label.id, |node| node.set_hidden());

         if checkbox.clicked() {
            let z_client = ctx.client_manager.clone();
            z_client.set_rpc_mev_protect(rpc.chain_id, &rpc.url, rpc.mev_protect);
            crate::runtime::rt().spawn_blocking(move || {
               let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
               ctx.save_client_manager();
            });
         }
      }

      let rpc = self.rpc_to_edit.as_ref().unwrap();
      let chain = rpc.chain_id;
      let url = rpc.url.clone();

      ui.add_space(theme.spacing.sm);

      let input_size = vec2(140.0, 20.0);
      let hint = format!(
         "Range {MIN_RPC_RPS}-{MAX_RPC_RPS} req/s, {MIN_RPC_CU_PER_SECOND}-{MAX_RPC_CU_PER_SECOND} CU/s. Applied per connection."
      );

      let rps_name = "Requests / second";
      let rps_text = RichText::new(rps_name).size(theme.typography.normal);
      let rps_label = ui.label(rps_text);
      ui.add(
         SecureTextEdit::singleline(&mut self.rps_input)
            .accessible_name(rps_name)
            .visuals(text_edit_visuals)
            .font(FontId::proportional(theme.typography.normal))
            .min_size(input_size)
            .margin(Margin::same(10)),
      );

      let cu_name = "Compute units / second";
      let cu_text = RichText::new(cu_name).size(theme.typography.normal);
      let cu_label = ui.label(cu_text);
      ui.add(
         SecureTextEdit::singleline(&mut self.cu_input)
            .accessible_name(cu_name)
            .visuals(text_edit_visuals)
            .font(FontId::proportional(theme.typography.normal))
            .min_size(input_size)
            .margin(Margin::same(10)),
      );

      // Each field carries the words drawn beside it, so the label nodes leave the tree.
      ui.ctx().accesskit_node_builder(rps_label.id, |node| node.set_hidden());
      ui.ctx().accesskit_node_builder(cu_label.id, |node| node.set_hidden());

      let rps = self.rps_input.trim().parse::<u32>().ok();
      let cu = self.cu_input.trim().parse::<u64>().ok();
      let valid = rps.is_some_and(|v| (MIN_RPC_RPS..=MAX_RPC_RPS).contains(&v))
         && cu.is_some_and(|v| (MIN_RPC_CU_PER_SECOND..=MAX_RPC_CU_PER_SECOND).contains(&v));

      ui.label(RichText::new(hint).size(theme.typography.small).color(theme.colors.text_muted));

      if !valid {
         let text = RichText::new("Enter a whole number within the range above")
            .size(theme.typography.small)
            .color(theme.colors.error);
         ui.label(text);
      }

      let text = RichText::new("Apply").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);
      let apply = ui.add_enabled(valid, button);
      let apply_name = "Apply endpoint limits";

      apply.widget_info(|| WidgetInfo::labeled(Role::Button, ui.is_enabled(), apply_name));

      if apply.clicked()
         && let (Some(rps), Some(cu)) = (rps, cu)
      {
         let z_client = ctx.client_manager.clone();
         z_client.set_rpc_limits(chain, &url, rps, cu);

         if let Some(rpc) = self.rpc_to_edit.as_mut() {
            rpc.rps = rps;
            rpc.cu_per_second = cu;
            rpc.clamp_limits();
            self.rps_input = rpc.rps.to_string();
            self.cu_input = rpc.cu_per_second.to_string();
         }

         crate::runtime::rt().spawn_blocking(move || {
            let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());
            ctx.save_client_manager();
         });
      }
   }

   fn add_rpc(&mut self, theme: &Theme, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.md);
      ui.spacing_mut().button_padding = theme.button_padding;

      self.back_button(theme, ui);

      section_heading(ui, theme.typography.large, "Add RPC");

      let button_visuals = theme.button_visuals();
      let text_edit_visuals = theme.text_edit_visuals();
      let ui_width = ui.available_width();

      let hint_text = RichText::new("Enter a url").size(theme.typography.normal);
      let field = ui.add(
         SecureTextEdit::singleline(&mut self.url_to_add)
            .accessible_name("RPC URL")
            .visuals(text_edit_visuals)
            .hint_text(hint_text)
            .font(FontId::proportional(theme.typography.normal))
            .min_size(vec2(ui_width * 0.5, 20.0))
            .margin(Margin::same(10)),
      );

      // This one is being typed rather than read back, so the hint it draws is the description
      // that tells a reader what to type.
      ui.ctx().accesskit_node_builder(field.id, |node| {
         node.set_description("Enter a url")
      });

      if !self.valid_url() && !self.url_to_add.is_empty() {
         ui.label(
            RichText::new(INVALID_URL_ERROR)
               .size(theme.typography.small)
               .color(theme.colors.error),
         );
         self.say_inline_error(INVALID_URL_ERROR);
      } else {
         self.clear_inline_error(INVALID_URL_ERROR);
      }

      if self.refreshing {
         ui.add(Spinner::new().size(15.0));
      }

      let text = RichText::new("Add").size(theme.typography.normal);
      let button = Button::new(text).visuals(button_visuals);
      if self.valid_url() && ui.add_enabled(!self.refreshing, button).clicked() {
         self.refreshing = true;
         let chain = self.chain_select.chain.id();
         validate_rpc(chain, self.url_to_add.clone());
      }
   }
}

fn validate_rpc(chain: u64, url: String) {
   let rpc = Rpc::builder(url.clone(), chain).enabled().build();

   crate::runtime::rt().spawn(async move {
      let ctx = SHARED_GUI.read(|gui| gui.ctx.clone());

      let client = match ctx.connect_to_rpc(&rpc).await {
         Ok(client) => client,
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!("Failed to connect to RPC: {}", e));
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      let rpc_chain = match client.get_chain_id().await {
         Ok(chain) => chain,
         Err(e) => {
            SHARED_GUI.write(|gui| {
               gui.open_msg_window(format!("Failed to get chain ID: {}", e));
               gui.settings.network.refreshing = false;
            });
            return;
         }
      };

      if rpc_chain != chain {
         SHARED_GUI.write(|gui| {
            gui.open_msg_window(format!(
               "Chain Mismatch, RPC {} is for chain {}",
               rpc.url, rpc_chain
            ));
            gui.settings.network.refreshing = false;
         });
         return;
      }

      let z_client = ctx.get_client_manager();
      z_client.add_rpc(chain, rpc.clone());
      z_client.run_check_for(ctx.clone(), rpc).await;

      let ctx_clone = ctx.clone();
      crate::runtime::rt().spawn_blocking(move || {
         ctx_clone.save_client_manager();
      });

      SHARED_GUI.write(|gui| {
         gui.open_msg_window("RPC added successfully");
         gui.settings.network.url_to_add.clear();
         gui.settings.network.close_add_rpc();
         gui.settings.network.refreshing = false;
      });

      post_enable_rpc(ctx, chain).await
   });
}

/// The endpoint's host — what the page may say instead of a URL that can carry a provider key.
///
/// The host is the part between `://` and the first `/` or `?`, so a key in the path never reaches
/// the accessibility tree. A string without those parts falls back to a plain description.
fn rpc_host(url: &str) -> &str {
   let Some((_, rest)) = url.split_once("://") else {
      return "RPC endpoint";
   };

   match rest.split(['/', '?']).next() {
      Some(host) if !host.is_empty() => host,
      _ => "RPC endpoint",
   }
}

fn rpc_col<R>(
   ui: &mut Ui,
   width: f32,
   height: f32,
   add_contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
   ui.allocate_ui_with_layout(
      vec2(width, height),
      Layout::left_to_right(Align::Center),
      |ui| {
         ui.set_min_size(vec2(width, height));
         ui.set_max_size(vec2(width, height));
         add_contents(ui)
      },
   )
}

fn rpc_status_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_working() {
      match rpc.is_fully_functional() {
         true => Indicator::new(IndicatorState::On).size(12.0),
         false => Indicator::new(IndicatorState::Connecting).size(12.0),
      }
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

fn rpc_archive_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_archive() {
      Indicator::new(IndicatorState::On).size(12.0)
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

fn rpc_mev_indicator(rpc: &Rpc) -> Indicator {
   if rpc.is_mev_protect() {
      Indicator::new(IndicatorState::On).size(12.0)
   } else {
      Indicator::new(IndicatorState::Off).size(12.0)
   }
}

fn post_toggle_network(ctx: ZeusCtx, chain: u64, was_disabled: bool) {
   tracing::info!("Chain {} was disabled: {}", chain, was_disabled);

   if was_disabled {
      tracing::info!("Syncing state for chain {}", chain);
      crate::runtime::rt().spawn(async move {
         state::sync_state(ctx.clone(), chain).await;
      });
   }
}

async fn post_enable_rpc(ctx: ZeusCtx, chain: u64) {
   if ctx.is_chain_disabled(chain) {
      return;
   }

   let z_client = ctx.get_client_manager();

   let rpcs = z_client.get_rpcs(chain);
   let valid_rpcs = rpcs.iter().filter(|rpc| rpc.1.is_enabled() && rpc.1.is_working()).count();
   let available_rpc = z_client.rpc_available(chain);

   // If we only have 1 active RPC, that means maybe is the first time Zeus started so we need to do
   // full state sync
   if valid_rpcs == 1 && available_rpc {
      state::sync_state(ctx.clone(), chain).await;
   }
}
