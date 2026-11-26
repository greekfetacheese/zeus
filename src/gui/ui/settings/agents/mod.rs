//! Settings → Agents: the endpoint, the credentials and the grants behind them.
//!
//! The page is the user's only way in and out: it starts and stops the listener (D24), mints and
//! revokes credentials, and edits the grant a credential holds. Every control writes the store and
//! asks for the save, so what the page shows is what a request would be decided against.

mod activity;
mod connect;
mod profile_editor;
pub mod server;

use crate::core::agent::{AgentProfile, AgentProfileId, generate_token};
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::Page;
use crate::gui::ui::common::section_heading;
use activity::Activity;
use connect::ConnectPanel;
use egui::{RichText, Ui, vec2};
use egui_elements::{Button, Checkbox, Theme};
use profile_editor::ProfileEditor;
use zeus_mcp::ServerHandle;

/// The Agents page.
#[derive(Default)]
pub struct AgentsUi {
   /// The profile whose grants are open, if any.
   selected: Option<AgentProfileId>,
   /// The one visible token a create or a rotation minted, with the profile it belongs to. It is
   /// never stored: the profile holds its hash, and this field is the only copy that exists.
   token: Option<(AgentProfileId, String)>,
   /// What the Rename field holds, and whose name it is editing.
   rename: String,
   rename_for: Option<AgentProfileId>,
   /// The profile a delete is waiting on a second press for.
   confirm_delete: Option<AgentProfileId>,
   editor: ProfileEditor,
   connect: ConnectPanel,
   activity: Activity,
   /// The page a row of the Activity view asked for, drained by the app that can move pages.
   page: Option<Page>,
   /// The words this page asked the app to say.
   notice: Option<String>,
}

impl AgentsUi {
   pub fn new() -> Self {
      Self::default()
   }

   /// The words this page raised, drained by the window after it has drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// The page an Activity row asked for, drained by the app that can move pages.
   pub fn take_page(&mut self) -> Option<Page> {
      self.page.take()
   }

   /// Called when the window closes: a credential that was shown once is not kept on screen.
   pub fn reset(&mut self) {
      self.token = None;
      self.confirm_delete = None;
      self.editor.reset();
      self.connect.reset();
      self.page = None;
      self.notice = None;
   }

   /// Draw the page.
   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      server: &mut Option<ServerHandle>,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      ui.spacing_mut().item_spacing = vec2(theme.spacing.xs, theme.spacing.sm);
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Agent Server");
      self.switch(ctx, handle, server, theme, ui);

      ui.add_space(10.0);
      ui.separator();
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Credentials");
      self.credentials(ctx, handle, theme, ui);

      let Some(id) = self.selected.clone() else {
         return;
      };

      let Some(profile) = ctx.agent_profiles.profile(&id) else {
         // The profile went away under the selection (a delete, an import): the page drops it.
         self.selected = None;
         return;
      };

      let mut profile = profile;

      ui.add_space(10.0);
      ui.separator();
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Grants");
      ui.label(
         RichText::new(format!("What {} may do.", profile.label)).size(theme.typography.normal),
      );

      self.identity(ctx, handle, &mut profile, theme, ui);

      let wallets = ctx.wallet_info_cache.ordered_vec.clone();
      let edited = self.editor.show(&mut profile, &wallets, theme, ui);

      if edited.changed {
         self.store(ctx, handle, profile.clone());
      }

      if let Some(sentence) = edited.notice {
         self.notice = Some(sentence);
      }

      ui.add_space(10.0);
      ui.separator();
      ui.add_space(10.0);

      if let Some(page) = self.activity.show(ctx, &profile, theme, ui) {
         self.page = Some(page);
      }

      let token = match self.token.clone() {
         Some((token_for, token)) if token_for == profile.id => Some(token),
         _ => None,
      };

      if let Some(token) = &token {
         self.token_block(token, theme, ui);
      }

      if let Some(token) = &token {
         ui.add_space(10.0);
         self.connect.show(ui, theme, server::running(server), token);
      } else {
         ui.add_space(10.0);
         ui.label(
            RichText::new(
               "Rotate the token to see the snippet a harness needs: the token is shown once, so \
                the Connect panel only appears while it is on screen.",
            )
            .size(theme.typography.normal),
         );
      }
   }

   /// The Start/Stop switch, which names the action it will take (D24).
   fn switch(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      server: &mut Option<ServerHandle>,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let running = server::running(server);

      let (label, status) = match running {
         Some(port) => (
            "Stop server",
            format!("Listening on 127.0.0.1:{port}/mcp."),
         ),
         None => (
            "Start server",
            "Stopped: no socket is open.".to_owned(),
         ),
      };

      ui.horizontal(|ui| {
         let button = Button::new(RichText::new(label).size(theme.typography.large))
            .visuals(theme.button_visuals())
            .min_size(vec2(170.0, 34.0));

         if ui.add(button).clicked() {
            self.notice = Some(server::toggle(server, ctx, handle));
         }

         ui.label(RichText::new(status).size(theme.typography.normal));
      });
   }

   /// Every credential, with the switch that disables it and the two actions that replace it.
   fn credentials(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx, theme: &Theme, ui: &mut Ui) {
      let profiles = ctx.agent_profiles.profiles();

      if profiles.is_empty() {
         ui.label(
            RichText::new(
               "No agent may connect yet. Create a profile, grant it something, then copy its \
                token into the harness.",
            )
            .size(theme.typography.normal),
         );
      }

      for profile in &profiles {
         let mut enabled = profile.enabled;

         ui.horizontal(|ui| {
            let toggle = Checkbox::new(
               &mut enabled,
               RichText::new(format!("Enabled: {}", profile.label)).size(theme.typography.normal),
            );

            if ui.add(toggle).changed() {
               ctx.agent_profiles.set_enabled(&profile.id, enabled);
               persist_profiles(handle);

               self.notice = Some(match enabled {
                  true => format!("{} may connect again.", profile.label),
                  false => format!("{} may no longer connect.", profile.label),
               });
            }

            let selected = self.selected == Some(profile.id.clone());
            let label = if selected { "Close" } else { "Open" };

            let button = Button::new(RichText::new(label).size(theme.typography.normal))
               .visuals(theme.button_visuals())
               .min_size(vec2(90.0, 26.0));

            if ui.add(button).clicked() {
               self.selected = match selected {
                  true => None,
                  false => Some(profile.id.clone()),
               };
               self.rename_for = None;
               self.editor.reset();
               self.connect.reset();
               self.confirm_delete = None;

               if self.token.as_ref().is_some_and(|(id, _)| id != &profile.id) {
                  self.token = None;
               }
            }
         });

         ui.horizontal(|ui| {
            let used = match profile.last_used_at {
               Some(at) => format!("Last used {}.", ago(at)),
               None => "Never used.".to_owned(),
            };

            ui.label(RichText::new(used).size(theme.typography.normal));

            let rotate = Button::new(RichText::new("Rotate token").size(theme.typography.normal))
               .visuals(theme.button_visuals())
               .min_size(vec2(130.0, 26.0));

            if ui.add(rotate).clicked() {
               self.rotate(ctx, handle, profile);
            }

            let delete = Button::new(RichText::new("Delete").size(theme.typography.normal))
               .visuals(theme.button_visuals())
               .min_size(vec2(90.0, 26.0));

            if ui.add(delete).clicked() {
               self.confirm_delete = Some(profile.id.clone());
            }
         });

         if self.confirm_delete == Some(profile.id.clone()) {
            ui.label(
               RichText::new("Delete this profile? Its token stops working immediately.")
                  .size(theme.typography.normal),
            );

            ui.horizontal(|ui| {
               let yes =
                  Button::new(RichText::new("Delete for good").size(theme.typography.normal))
                     .visuals(theme.button_visuals())
                     .min_size(vec2(150.0, 26.0));

               if ui.add(yes).clicked() {
                  ctx.agent_profiles.remove(&profile.id);
                  persist_profiles(handle);

                  if self.selected == Some(profile.id.clone()) {
                     self.selected = None;
                  }

                  self.confirm_delete = None;
                  self.notice = Some(format!("{} was deleted.", profile.label));
               }

               let keep = Button::new(RichText::new("Keep").size(theme.typography.normal))
                  .visuals(theme.button_visuals())
                  .min_size(vec2(90.0, 26.0));

               if ui.add(keep).clicked() {
                  self.confirm_delete = None;
               }
            });
         }
      }

      ui.add_space(theme.spacing.sm);

      let create = Button::new(RichText::new("New agent").size(theme.typography.large))
         .visuals(theme.button_visuals())
         .min_size(vec2(170.0, 34.0));

      if ui.add(create).clicked() {
         self.create(ctx, handle);
      }
   }

   /// The name field, which is the one thing about a profile that is not a permission.
   fn identity(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      profile: &mut AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      if self.rename_for != Some(profile.id.clone()) {
         self.rename = profile.label.clone();
         self.rename_for = Some(profile.id.clone());
      }

      ui.horizontal(|ui| {
         ui.label(RichText::new("Name").size(theme.typography.normal));

         let field = ui.add(
            egui::TextEdit::singleline(&mut self.rename)
               .hint_text("Profile name")
               .desired_width(260.0),
         );

         ui.ctx().accesskit_node_builder(field.id, |node| node.set_label("Profile name"));

         let rename = Button::new(RichText::new("Rename").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(100.0, 26.0));

         if ui.add(rename).clicked() {
            let name = self.rename.trim().to_owned();

            if !name.is_empty() && name != profile.label {
               profile.label = name;
               ctx.agent_profiles.insert(profile.clone());
               persist_profiles(handle);
               self.notice = Some("Profile renamed.".to_owned());
            }
         }
      });
   }

   /// The token, once: it is the only time Zeus has it.
   fn token_block(&mut self, token: &str, theme: &Theme, ui: &mut Ui) {
      ui.add_space(10.0);
      ui.separator();
      ui.add_space(10.0);

      section_heading(ui, theme.typography.very_large, "Token");

      ui.label(
         RichText::new("Copy this now: it is shown once, and Zeus keeps only its hash.")
            .size(theme.typography.normal),
      );

      ui.label(RichText::new(token).monospace().size(theme.typography.normal));

      ui.horizontal(|ui| {
         let copy = Button::new(RichText::new("Copy token").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(130.0, 26.0));

         if ui.add(copy).clicked() {
            ui.ctx().copy_text(token.to_owned());
            self.notice = Some("Token copied.".to_owned());
         }

         let done = Button::new(RichText::new("Done").size(theme.typography.normal))
            .visuals(theme.button_visuals())
            .min_size(vec2(90.0, 26.0));

         if ui.add(done).clicked() {
            self.token = None;
         }
      });
   }

   /// Mint a credential with nothing granted, and show its token once.
   fn create(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx) {
      let (token, hash) = generate_token();
      let profile = AgentProfile::new("New agent", hash, now_secs());

      self.store(ctx, handle, profile.clone());

      self.selected = Some(profile.id.clone());
      self.token = Some((profile.id, token.as_str().to_owned()));
      self.rename_for = None;
      self.notice = Some(
         "Profile created with nothing granted. Copy its token now, then grant what it may do."
            .to_owned(),
      );
   }

   fn rotate(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx, profile: &AgentProfile) {
      let (token, hash) = generate_token();

      if ctx.agent_profiles.rotate_token(&profile.id, hash) {
         persist_profiles(handle);

         self.selected = Some(profile.id.clone());
         self.token = Some((profile.id.clone(), token.as_str().to_owned()));
         self.notice = Some(format!(
            "New token for {}. The old one no longer works; copy this one now.",
            profile.label
         ));
      }
   }

   /// Write one profile into the store, and ask for the sealed file behind it.
   fn store(&self, ctx: &mut ZeusContext, handle: &ZeusCtx, profile: AgentProfile) {
      ctx.agent_profiles.insert(profile);
      persist_profiles(handle);
   }
}

/// Save the profile store off the frame path.
///
/// The save waits for the frame's own write guard, so it must not run on this thread: a blocking
/// task lands after the pass that asked for it, which is exactly once per user action. The status
/// bar's Disconnect all turns every credential off, so it saves through the same path.
pub(super) fn persist_profiles(handle: &ZeusCtx) {
   let handle = handle.clone();

   crate::runtime::rt().spawn_blocking(move || {
      if let Err(e) = handle.save_agent_profiles() {
         tracing::error!("Could not save the agent profiles: {e:?}");
      }
   });
}

/// A timestamp as a sentence a reader can use.
pub(super) fn ago(at: u64) -> String {
   match now_secs().saturating_sub(at) {
      0..=59 => "just now".to_owned(),
      60..=3_599 => format!(
         "{} minutes ago",
         now_secs().saturating_sub(at) / 60
      ),
      3_600..=86_399 => format!(
         "{} hours ago",
         now_secs().saturating_sub(at) / 3_600
      ),
      _ => format!(
         "{} days ago",
         now_secs().saturating_sub(at) / 86_400
      ),
   }
}

/// Unix seconds, the unit a stored timestamp uses.
fn now_secs() -> u64 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|elapsed| elapsed.as_secs())
      .unwrap_or(0)
}
