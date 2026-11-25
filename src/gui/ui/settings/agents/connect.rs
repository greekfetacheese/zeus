//! The Connect panel: what to paste into a harness, and a handshake performed for real.
//!
//! Two snippets, each a complete command or file the user can paste without assembling JSON by
//! hand — that is the difference between a feature a blind user can turn on and one that needs
//! sighted help. Adding a harness is one row here, not a new screen.

use crate::gui::ui::common::section_heading;
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Theme};
use std::sync::{Arc, Mutex};

/// One harness's snippet, with the two placeholders the panel fills in.
struct Harness {
   /// What the user calls the harness.
   name: &'static str,
   /// The transport word, so the row says how it connects as well as where.
   transport: &'static str,
   /// The words to paste: `{port}` and `{token}` are replaced.
   template: &'static str,
}

/// The harnesses the panel offers, in the order they are drawn.
const HARNESSES: &[Harness] = &[
   Harness {
      name: "Claude Code",
      transport: "http",
      template: "claude mcp add --transport http zeus http://127.0.0.1:{port}/mcp \
                 --header \"Authorization: Bearer {token}\"",
   },
   Harness {
      name: "Any harness with a JSON config",
      transport: "http",
      template: "{\n  \"mcpServers\": {\n    \"zeus\": {\n      \
                 \"type\": \"http\",\n      \
                 \"url\": \"http://127.0.0.1:{port}/mcp\",\n      \
                 \"headers\": { \"Authorization\": \"Bearer {token}\" }\n    }\n  }\n}",
   },
];

/// The Connect panel's own state: a probe in flight, and what the last one said.
#[derive(Default)]
pub struct ConnectPanel {
   /// The slot a running probe fills, so the frame that draws the panel can show its answer.
   probe: Option<Arc<Mutex<Option<String>>>>,
   /// What the last probe answered.
   result: Option<String>,
}

impl ConnectPanel {
   pub fn reset(&mut self) {
      self.probe = None;
      self.result = None;
   }

   /// Draw the panel for a credential in hand.
   ///
   /// `port` is `None` while the server is stopped: the snippets still name the harness, but the
   /// address they would carry is not known, and a snippet with a guessed port is worse than none.
   pub fn show(&mut self, ui: &mut Ui, theme: &Theme, port: Option<u16>, token: &str) {
      section_heading(ui, theme.typography.very_large, "Connect");

      let intro = "Paste one of these into the harness. The token above is the whole credential, \
                   and Zeus does not keep a copy of it.";
      ui.label(RichText::new(intro).size(theme.typography.normal));

      for harness in HARNESSES {
         ui.add_space(theme.spacing.md);

         let heading = format!("{} ({})", harness.name, harness.transport);
         ui.label(RichText::new(heading).size(theme.typography.large));

         let Some(port) = port else {
            ui.label(
               RichText::new("Start the agent server to see the address.")
                  .size(theme.typography.normal),
            );
            continue;
         };

         let snippet = fill(harness.template, port, token);

         ui.label(RichText::new(&snippet).monospace().size(theme.typography.normal));

         let button = Button::new(RichText::new(format!(
            "Copy {} snippet",
            harness.name
         )))
         .visuals(theme.button_visuals())
         .min_size(vec2(220.0, 28.0));

         if ui.add(button).clicked() {
            ui.ctx().copy_text(snippet);
         }
      }

      self.probe_block(ui, theme, port, token);
   }

   /// Test connection: the real handshake, from inside Zeus, and the sentence it produced.
   fn probe_block(&mut self, ui: &mut Ui, theme: &Theme, port: Option<u16>, token: &str) {
      let Some(port) = port else {
         return;
      };

      ui.add_space(theme.spacing.md);

      let button = Button::new(RichText::new("Test connection"))
         .visuals(theme.button_visuals())
         .min_size(vec2(220.0, 28.0));

      if ui.add(button).clicked() {
         let slot = Arc::new(Mutex::new(None));
         self.probe = Some(slot.clone());
         self.result = None;

         let token = token.to_owned();

         crate::runtime::rt().spawn_blocking(move || {
            let answer = match zeus_mcp::probe::probe(port, &token) {
               Ok(report) => format!(
                  "Connected: {} {} lists {} tools for this token.",
                  report.server, report.version, report.tools
               ),
               Err(e) => format!("Could not connect: {e}"),
            };

            if let Ok(mut slot) = slot.lock() {
               *slot = Some(answer);
            }

            // The answer lands on a worker; without this the page would sit on "Testing…" until
            // the user moved the pointer.
            crate::gui::SHARED_GUI.read(|gui| gui.egui_ctx.clone()).request_repaint();
         });
      }

      if let Some(slot) = self.probe.clone()
         && let Some(answer) = slot.lock().ok().and_then(|mut slot| slot.take())
      {
         self.result = Some(answer);
         self.probe = None;
      }

      if self.probe.is_some() {
         ui.label(RichText::new("Testing the connection…").size(theme.typography.normal));
      }

      if let Some(result) = &self.result {
         ui.label(RichText::new(result).size(theme.typography.normal));
      }
   }
}

/// The snippet with the port and the token filled in.
fn fill(template: &str, port: u16, token: &str) -> String {
   template.replace("{port}", &port.to_string()).replace("{token}", token)
}

#[cfg(test)]
mod tests {
   use super::*;

   const TOKEN: &str = "zk_0123456789abcdef";

   #[test]
   fn every_snippet_carries_the_address_and_the_token() {
      for harness in HARNESSES {
         let snippet = fill(harness.template, 65502, TOKEN);
         assert!(
            snippet.contains("http://127.0.0.1:65502/mcp"),
            "{}",
            harness.name
         );
         assert!(snippet.contains(TOKEN), "{}", harness.name);
         assert!(
            !snippet.contains("{port}"),
            "{} left its port placeholder",
            harness.name
         );
         assert!(
            !snippet.contains("{token}"),
            "{} left its token placeholder",
            harness.name
         );
      }
   }

   #[test]
   fn the_json_snippet_is_valid_json() {
      let snippet = fill(HARNESSES[1].template, 65502, TOKEN);
      let parsed: serde_json::Value =
         serde_json::from_str(&snippet).expect("the JSON row must parse");
      assert_eq!(
         parsed.pointer("/mcpServers/zeus/url").and_then(|v| v.as_str()),
         Some("http://127.0.0.1:65502/mcp")
      );
   }

   #[test]
   fn the_claude_code_snippet_is_the_documented_command() {
      let snippet = fill(HARNESSES[0].template, 65502, TOKEN);
      assert!(
         snippet.starts_with("claude mcp add --transport http zeus"),
         "{snippet}"
      );
      assert!(
         snippet.contains(&format!("Authorization: Bearer {TOKEN}")),
         "{snippet}"
      );
   }
}
