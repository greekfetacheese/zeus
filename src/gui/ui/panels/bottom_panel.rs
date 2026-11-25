//! The status bar: the agent switch, and the indicator beside it.
//!
//! It is one of the three places D24 puts the Start/Stop control, next to the connected indicator,
//! so the listener is one action away from every page rather than three from Settings.

use crate::core::ZeusContext;
use crate::gui::GUI;
use eframe::egui::{Align, Layout, RichText, Ui, vec2};
use egui_elements::Button;

/// How recently an agent must have called for the indicator to say it is connected.
const CONNECTED_FOR: u64 = 300;

pub fn _show(gui: &mut GUI, ctx: &mut ZeusContext, ui: &mut Ui) {
   let theme = gui.theme.clone();

   ui.spacing_mut().item_spacing = vec2(theme.spacing.sm, theme.spacing.xs);
   ui.spacing_mut().button_padding = theme.button_padding;

   let running = crate::gui::ui::settings::agents::server::running(&gui.agent_server);

   ui.horizontal(|ui| {
      let status = match running {
         Some(port) => format!("Agent server: listening on 127.0.0.1:{port}/mcp"),
         None => "Agent server: stopped".to_owned(),
      };

      ui.label(RichText::new(status).size(theme.typography.normal));

      let connected = running.is_some() && connected_recently(ctx);
      let indicator = match connected {
         true => "An agent is connected.",
         false => "No agent is connected.",
      };

      ui.label(RichText::new(indicator).size(theme.typography.normal));

      ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
         let label = match running {
            Some(_) => "Stop server",
            None => "Start server",
         };

         let button = Button::new(RichText::new(label).size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(140.0, 26.0));

         if ui.add(button).clicked() {
            let handle = gui.ctx.clone();
            let sentence = crate::gui::ui::settings::agents::server::toggle(
               &mut gui.agent_server,
               ctx,
               &handle,
            );

            gui.announce(sentence);
         }
      });
   });
}

/// Whether an enabled credential has been used recently enough to call the agent connected.
fn connected_recently(ctx: &ZeusContext) -> bool {
   let now = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|elapsed| elapsed.as_secs())
      .unwrap_or(0);

   ctx.agent_profiles
      .enabled_profiles()
      .iter()
      .filter_map(|profile| profile.last_used_at)
      .max()
      .is_some_and(|at| now.saturating_sub(at) < CONNECTED_FOR)
}
