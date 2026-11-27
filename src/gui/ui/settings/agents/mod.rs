//! Settings → Agents: the endpoint, the profiles that may reach it, and what each of them may do.
//!
//! The page is a list of agents, not a dump of switches. One line per credential says the two things a
//! user needs before anything else — whether it may connect, and how much of the wallet it holds — and
//! one control opens that agent into four tabs: what it is, what it may do on each wallet, what it may
//! do to the app's settings, and what it has done.
//!
//! The wallet grants are the one place a table is the right shape: a user compares wallets against each
//! other, so they are the rows of one grid rather than a stack of cards, and the switches for one wallet
//! are a dialog rather than a section that grows the page.

mod activity;
mod connect;
mod grants;
mod profile_editor;
pub mod server;
mod wallet_grants;

use crate::core::agent::{AgentProfile, AgentProfileId, generate_token};
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::Page;
use crate::gui::ui::common::section_heading;
use activity::Activity;
use connect::ConnectPanel;
use egui::{RichText, Ui, vec2};
use egui_elements::{
   Button, Checkbox, Chip, ChipRole, Indicator, IndicatorState, SecureTextEdit, Slider, Tabs, Theme,
};
use profile_editor::ProfileEditor;
use wallet_grants::WalletGrants;
use zeus_mcp::ServerHandle;

/// The height of a control on this page.
///
/// One number, so a button, a field and a switch agree about how tall they are: the page is a column
/// of rows a user reads down, and a row whose controls disagree about their height reads as a layout
/// accident rather than as a set.
const CONTROL: f32 = 30.0;

/// The column a setting's name keeps, so the two ceilings' rails start at the same x.
const LABEL_WIDTH: f32 = 170.0;

/// The rail's own width: wide enough that a percentage can be aimed at, and the same on both rows.
const RAIL_WIDTH: f32 = 220.0;

/// Which of an agent's four screens is in front.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
   /// What the agent is: its name, and the token that is its whole credential.
   #[default]
   Profile,
   /// What it may do on each wallet.
   Wallets,
   /// What it may read or write in the app's own settings.
   Settings,
   /// What it has done.
   Activity,
}

impl Tab {
   /// Every screen, in the order the tab bar offers them.
   const ALL: [Self; 4] = [Self::Profile, Self::Wallets, Self::Settings, Self::Activity];

   /// The name on the tab and the heading of the screen behind it, so the two cannot disagree.
   fn name(self) -> &'static str {
      match self {
         Self::Profile => "Profile",
         Self::Wallets => "Wallets",
         Self::Settings => "Settings",
         Self::Activity => "Activity",
      }
   }

   /// The index the strip holds, which is the order [`Self::ALL`] names.
   fn index(self) -> usize {
      Self::ALL.iter().position(|tab| *tab == self).unwrap_or_default()
   }

   /// The screen at `index`, or the first one for an index that outlived its strip.
   fn at(index: usize) -> Self {
      Self::ALL.get(index).copied().unwrap_or_default()
   }
}

/// The Agents page.
#[derive(Default)]
pub struct AgentsUi {
   /// The agent whose screens are open, if any.
   selected: Option<AgentProfileId>,
   /// The screen of the open agent that is in front.
   tab: Tab,
   /// The open agent as this page has it.
   ///
   /// Every switch writes here rather than straight to the store, so dragging a rail is one edit and
   /// not one store write — and one sealed-file save — per frame. The store is written once, by
   /// [`Self::store`], when the window closes.
   draft: Option<AgentProfile>,
   /// Whether `draft` holds an edit the store has not seen.
   edited: bool,
   /// The one visible token a create or a rotation minted, with the profile it belongs to. It is
   /// never stored: the profile holds its hash, and this field is the only copy that exists.
   token: Option<(AgentProfileId, String)>,
   /// What the Rename field holds, and whose name it is editing.
   rename: String,
   rename_for: Option<AgentProfileId>,
   /// The profile a delete is waiting on a second press for.
   confirm_delete: Option<AgentProfileId>,
   editor: ProfileEditor,
   grants: WalletGrants,
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

   /// Called when the window closes: the open agent's edits are written once, and a credential that
   /// was shown once is not kept on screen.
   pub fn close(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx) {
      self.flush(ctx, handle);

      self.token = None;
      self.confirm_delete = None;
      self.editor.reset();
      self.grants.close();
      self.connect.reset();
      self.page = None;
      self.notice = None;
   }

   /// Draw the page: the endpoint's light, the list of agents, and the one agent that is open.
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

      self.status(ctx, handle, server, theme, ui);

      ui.add_space(theme.spacing.md);

      self.agents(ctx, handle, theme, ui);

      let Some(id) = self.selected.clone() else {
         return;
      };

      // The page edits its own copy of the profile, re-seeded whenever the open agent changes.
      if self.draft.as_ref().is_none_or(|draft| draft.id != id) {
         self.draft = ctx.agent_profiles.profile(&id);
         self.edited = false;
      }

      let Some(mut profile) = self.draft.take() else {
         // The profile went away under the selection (a delete, an import): the page drops it.
         self.selected = None;
         return;
      };

      ui.add_space(theme.spacing.md);
      ui.separator();
      ui.add_space(theme.spacing.md);

      // The agent's own screens open here, under the name of the agent they belong to.
      self.tabs(&profile, theme, ui);

      ui.add_space(theme.spacing.md);

      let edited = match self.tab {
         Tab::Profile => self.profile_tab(ctx, handle, &mut profile, server, theme, ui),
         Tab::Wallets => self.wallets_tab(ctx, &mut profile, theme, ui),
         Tab::Settings => self.settings_tab(&mut profile, theme, ui),
         Tab::Activity => {
            self.activity_tab(ctx, &profile, theme, ui);
            false
         }
      };

      self.edited |= edited;

      // A delete takes the profile away under this pass; anything else hands the edits to the close.
      if self.selected.as_ref() == Some(&profile.id) {
         self.draft = Some(profile);
      }
   }

   /// The endpoint's state as one light, and the one brake the page offers.
   ///
   /// There is no Start/Stop control: the listener is up while Zeus runs and there is an agent to
   /// authenticate, so its state is something to read. What can still be done is take every credential
   /// away at once, which is the same guarantee in one press.
   fn status(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      server: &Option<ServerHandle>,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let (state, sentence) = match server::running(server) {
            Some(port) => (
               IndicatorState::On,
               format!("Listening on 127.0.0.1:{port}/mcp"),
            ),
            None => (
               IndicatorState::Off,
               "Starts by itself as soon as one agent exists".to_owned(),
            ),
         };

         // The light carries the state; the sentence beside it says the same thing in words, because
         // a colour alone is nothing to a reader and not much to anyone else either.
         ui.add(Indicator::new(state).size(12.0).label(format!("Agent server: {sentence}")));

         ui.add(egui::Label::new(RichText::new(sentence).size(theme.typography.normal)).wrap());

         if !ctx.agent_profiles.enabled_profiles().is_empty() {
            ui.with_layout(
               egui::Layout::right_to_left(egui::Align::Center),
               |ui| {
                  let button = button(theme, "Stop all agents", 160.0);

                  if ui.add(button).clicked() {
                     self.notice = Some(server::disconnect_all(ctx, handle));
                  }
               },
            );
         }
      });
   }

   /// Every credential as one line, and the one control that opens it.
   fn agents(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx, theme: &Theme, ui: &mut Ui) {
      section_heading(ui, theme.typography.large, "Agents");

      let profiles = ctx.agent_profiles.profiles();

      if profiles.is_empty() {
         paragraph(
            ui,
            theme,
            "No agent may connect yet. Create one, grant it something, then paste its token into \
             the harness that will use it.",
         );
      }

      for profile in &profiles {
         // The row says what the page under it holds: while an agent is open, that is its draft.
         let (label, summary) = match &self.draft {
            Some(draft) if draft.id == profile.id => (draft.label.clone(), grants::summary(draft)),
            _ => (profile.label.clone(), grants::summary(profile)),
         };

         ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing.sm;

            let mut enabled = profile.enabled;

            // The name rides on the switch, so the row's first control says which agent it is for.
            let toggle = ui.add(Checkbox::new(
               &mut enabled,
               RichText::new(format!("{label} may connect")).size(theme.typography.normal),
            ));

            etiquetted(
               &toggle,
               ui,
               &format!("{label} may connect"),
               "Off means every request that carries this agent's token is refused.",
            );

            if toggle.changed() {
               // The kill switch is not a grant: it takes effect now, so the server refuses the next
               // request. Whatever the page still holds is written first, so the two cannot disagree.
               self.flush(ctx, handle);
               ctx.agent_profiles.set_enabled(&profile.id, enabled);
               persist_profiles(handle);

               self.notice = Some(match enabled {
                  true => format!("{label} may connect again."),
                  false => format!("{label} may no longer connect."),
               });
            }

            ui.add(egui::Label::new(RichText::new(summary).size(theme.typography.small)).wrap());

            ui.with_layout(
               egui::Layout::right_to_left(egui::Align::Center),
               |ui| {
                  let selected = self.selected == Some(profile.id.clone());
                  let label = if selected { "Close" } else { "Open" };

                  let button = button(theme, label, 90.0);

                  if ui.add(button).clicked() {
                     self.select(ctx, handle, profile, selected);
                  }
               },
            );
         });

         if self.confirm_delete == Some(profile.id.clone()) {
            self.delete_block(ctx, handle, profile, theme, ui);
         }
      }

      ui.add_space(theme.spacing.xs);

      let create = button(theme, "New agent", 160.0);

      if ui.add(create).clicked() {
         self.create(ctx, handle);
      }
   }

   /// The agent's four screens as one segmented bar, under the agent's own name.
   fn tabs(&mut self, profile: &AgentProfile, theme: &Theme, ui: &mut Ui) {
      section_heading(ui, theme.typography.large, profile.label.clone());

      let mut index = self.tab.index();

      Tabs::new(
         ("agent_screens", &profile.id),
         &mut index,
         Tab::ALL.map(Tab::name),
      )
      .fill_width(false)
      .text_size(theme.typography.normal)
      .height(CONTROL + 6.0)
      .show(ui);

      self.tab = Tab::at(index);
   }
   /// What the agent is: its name, and — once, when it is new — the token that is its credential.
   fn profile_tab(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      profile: &mut AgentProfile,
      server: &mut Option<ServerHandle>,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let renamed = self.identity(profile, theme, ui);

      let token = match self.token.clone() {
         Some((token_for, token)) if token_for == profile.id => Some(token),
         _ => None,
      };

      match token {
         Some(token) => {
            self.token_block(&token, theme, ui);
            ui.add_space(theme.spacing.md);
            self.connect.show(ui, theme, server::running(server), &token);
         }
         None => {
            ui.add_space(theme.spacing.md);
            self.token_actions(ctx, handle, profile, theme, ui);
            ui.add_space(theme.spacing.sm);
            paragraph(
               ui,
               theme,
               "The token is shown once, when the agent is created or its token is rotated, so the \
                Connect panel appears only while one is on screen.",
            );
         }
      }

      renamed
   }

   /// The Wallets screen: the order the agent acts in, then one row per wallet.
   fn wallets_tab(
      &mut self,
      ctx: &ZeusContext,
      profile: &mut AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) -> bool {
      let mut changed = self.editor.mode_and_limits(profile, theme, ui).changed;

      ui.add_space(theme.spacing.md);

      let wallets = ctx.wallet_info_cache.ordered_vec.clone();

      let edited = self.grants.show(profile, &wallets, theme, ui);

      changed |= edited.changed;

      if let Some(sentence) = edited.notice {
         self.notice = Some(sentence);
      }

      changed
   }

   /// The Settings screen: one switch per screen and verb, grouped by screen.
   fn settings_tab(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> bool {
      let edited = self.editor.screens(profile, theme, ui);

      if let Some(sentence) = edited.notice {
         self.notice = Some(sentence);
      }

      edited.changed
   }

   fn activity_tab(
      &mut self,
      ctx: &ZeusContext,
      profile: &AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      if let Some(page) = self.activity.show(ctx, profile, theme, ui) {
         self.page = Some(page);
      }
   }

   /// Open or close one agent, and drop what belonged to the one that was open.
   ///
   /// What the agent that was open changed is written before another one takes its place: a screen
   /// that is closed is a session that is over.
   fn select(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      profile: &AgentProfile,
      selected: bool,
   ) {
      self.flush(ctx, handle);

      self.selected = match selected {
         true => None,
         false => Some(profile.id.clone()),
      };

      // A screen is the same four words for every agent, so the one in front resets rather than
      // carrying the previous agent's place into the next one.
      self.tab = Tab::Profile;
      self.rename_for = None;
      self.editor.reset();
      self.grants.close();
      self.connect.reset();
      self.confirm_delete = None;

      if self.token.as_ref().is_some_and(|(id, _)| id != &profile.id) {
         self.token = None;
      }
   }

   /// The name field, which is the one thing about a profile that is not a permission.
   ///
   /// Returns whether the name changed: it is written with the rest of the draft, not on its own.
   fn identity(&mut self, profile: &mut AgentProfile, theme: &Theme, ui: &mut Ui) -> bool {
      if self.rename_for != Some(profile.id.clone()) {
         self.rename = profile.label.clone();
         self.rename_for = Some(profile.id.clone());
      }

      let mut renamed = false;

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let label = ui.label(RichText::new("Name").size(theme.typography.normal));
         ui.ctx().accesskit_node_builder(label.id, |node| node.set_hidden());

         // The field, the button beside it and every other control on the page share one height, so
         // a row reads as one line rather than as three widgets of three sizes.
         ui.add(
            SecureTextEdit::singleline(&mut self.rename)
               .accessible_name("Profile name")
               .hint_text("Profile name")
               .desired_width(260.0)
               .min_size(vec2(260.0, CONTROL))
               .font(egui::FontId::proportional(
                  theme.typography.normal,
               )),
         );

         let rename = button(theme, "Rename", 100.0);

         if ui.add(rename).clicked() {
            let name = self.rename.trim().to_owned();

            if name.is_empty() || name == profile.label {
               self.notice = Some("The name is unchanged.".to_owned());

               return;
            }

            profile.label = name;
            renamed = true;
            self.notice = Some("Profile renamed.".to_owned());
         }
      });

      renamed
   }

   /// Rotate or delete, which are the two things a credential can have done to it.
   fn token_actions(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      profile: &mut AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let rotate = button(theme, "Rotate token", 130.0);

         if ui.add(rotate).clicked() {
            self.rotate(ctx, handle, profile);
         }

         let delete = button(theme, "Delete", 90.0);

         if ui.add(delete).clicked() {
            self.confirm_delete = Some(profile.id.clone());
         }
      });
   }

   /// The second press a delete needs, and the sentence that says what it costs.
   fn delete_block(
      &mut self,
      ctx: &mut ZeusContext,
      handle: &ZeusCtx,
      profile: &AgentProfile,
      theme: &Theme,
      ui: &mut Ui,
   ) {
      let sentence = format!(
         "Delete {}? Its token stops working immediately.",
         profile.label
      );

      paragraph(ui, theme, sentence);

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let yes = button(theme, "Delete for good", 150.0);

         if ui.add(yes).clicked() {
            ctx.agent_profiles.remove(&profile.id);
            persist_profiles(handle);

            // The profile is gone, so its draft has nothing left to write.
            if self.draft.as_ref().is_some_and(|draft| draft.id == profile.id) {
               self.draft = None;
               self.edited = false;
            }

            if self.selected == Some(profile.id.clone()) {
               self.selected = None;
            }

            self.confirm_delete = None;
            self.notice = Some(format!("{} was deleted.", profile.label));
         }

         let keep = button(theme, "Keep", 90.0);

         if ui.add(keep).clicked() {
            self.confirm_delete = None;
         }
      });
   }

   /// The token, once: it is the only time Zeus has it.
   fn token_block(&mut self, token: &str, theme: &Theme, ui: &mut Ui) {
      ui.add_space(theme.spacing.sm);

      section_heading(ui, theme.typography.large, "Token");

      paragraph(
         ui,
         theme,
         "Copy this now: it is shown once, and Zeus keeps only its hash.",
      );

      let shown = ui.add(
         egui::Label::new(RichText::new(token).monospace().size(theme.typography.normal)).wrap(),
      );

      ui.ctx().accesskit_node_builder(shown.id, |node| {
         node.set_label("Token, shown once");
      });

      ui.horizontal(|ui| {
         ui.spacing_mut().item_spacing.x = theme.spacing.sm;

         let copy = button(theme, "Copy token", 130.0);

         if ui.add(copy).clicked() {
            ui.ctx().copy_text(token.to_owned());
            self.notice = Some("Token copied.".to_owned());
         }

         let done = button(theme, "Done", 90.0);

         if ui.add(done).clicked() {
            self.token = None;
         }
      });
   }

   /// Mint a credential with nothing granted, and show its token once.
   fn create(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx) {
      // The edits belong to the agent that is open, not to the one about to be created.
      self.flush(ctx, handle);

      let (token, hash) = generate_token();
      let profile = AgentProfile::new("New agent", hash, now_secs());

      self.store(ctx, handle, profile.clone());

      self.selected = Some(profile.id.clone());
      self.tab = Tab::Profile;
      self.token = Some((profile.id, token.as_str().to_owned()));
      self.rename_for = None;
      self.notice = Some(
         "Profile created with nothing granted. Copy its token now, then grant what it may do."
            .to_owned(),
      );
   }

   fn rotate(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx, profile: &mut AgentProfile) {
      let (token, hash) = generate_token();

      if ctx.agent_profiles.rotate_token(&profile.id, hash) {
         // The rotation is written now, so the draft must carry it: the write on close would
         // otherwise put the token the server has already refused back into the file.
         profile.token_hash = hash;

         persist_profiles(handle);

         self.selected = Some(profile.id.clone());
         self.token = Some((profile.id.clone(), token.as_str().to_owned()));
         self.tab = Tab::Profile;
         self.notice = Some(format!(
            "New token for {}. The old one no longer works; copy this one now.",
            profile.label
         ));
      }
   }

   /// Write the open agent, once, if this session changed it.
   ///
   /// Every switch on the page lands in [`Self::draft`]; this is where that draft reaches the store,
   /// and with it the sealed file. Called when the window closes, when another agent takes its place,
   /// and before an action that writes the store itself.
   fn flush(&mut self, ctx: &mut ZeusContext, handle: &ZeusCtx) {
      let Some(profile) = self.draft.take() else {
         return;
      };

      if std::mem::take(&mut self.edited) {
         self.store(ctx, handle, profile);
      }
   }

   /// Write one profile into the store, and ask for the sealed file behind it.
   fn store(&self, ctx: &mut ZeusContext, handle: &ZeusCtx, profile: AgentProfile) {
      ctx.agent_profiles.insert(profile);
      persist_profiles(handle);
   }
}

/// One wrapped paragraph at the page's own width.
///
/// A settings page is narrow and a sentence about a grant is not, so every one of these is drawn
/// unwrapped rather than left to egui's default, which keeps the source indentation of a continued
/// string literal and opens the line with a stretch of blank space both to the eye and to a reader.
fn paragraph(ui: &mut Ui, theme: &Theme, text: impl Into<String>) -> egui::Response {
   ui.add(egui::Label::new(RichText::new(text.into()).size(theme.typography.normal)).wrap())
}

/// One long sentence beside a control, which is the same paragraph in a smaller size.
fn note(ui: &mut Ui, theme: &Theme, text: impl Into<String>) -> egui::Response {
   ui.add(egui::Label::new(RichText::new(text.into()).size(theme.typography.small)).wrap())
}

/// A button the page draws.
///
/// One height and one alignment for every button on the page: a user reads these as a control set, and a
/// row whose buttons disagree about where their words sit reads as a layout accident. `width` is the
/// column the button keeps so a stack of them lines up.
pub(super) fn button(theme: &Theme, text: impl Into<String>, width: f32) -> Button<'static> {
   Button::new(RichText::new(text.into()).size(theme.typography.normal))
      .min_size(vec2(width, CONTROL))
      .label_align(egui::Align::Center)
}

/// The `?` that explains what a setting does, beside the setting.
///
/// The same pattern the Appearance page uses: the chip keeps its drawn question mark, and its node
/// carries the name and the sentence, because a hover is not reachable without a pointer. Draw it as the
/// last thing in a row, so two rows' tooltips cannot stack.
pub(super) fn q_mark(ui: &mut Ui, theme: &Theme, says: &'static str) -> egui::Response {
   let q_mark = RichText::new("?").size(theme.typography.normal);
   let chip = ui.add(Chip::new(q_mark).role(ChipRole::Info)).on_hover_text(says);

   ui.ctx().accesskit_node_builder(chip.id, |node| {
      node.set_label("What this means");
      node.set_description(says);
   });

   chip
}

/// A row's label, drawn for the eye and hidden from the tree, with the question mark that replaces it.
///
/// The chip is what a reader reaches, so the word beside it is not left in the tree as a second orphan
/// to Tab through.
pub(super) fn labelled(ui: &mut Ui, theme: &Theme, label: &str) -> egui::Response {
   // Not `.extend()`: a label that extends claims the whole row, which leaves the rail beside it with
   // nothing to be drawn in — the setting then only moves when the row is re-laid out, and a pointer
   // over the collapsed rail is a pointer over nothing.
   // The column is allocated first and exactly, then the word is placed inside it: a label that
   // reports its own width makes the rail beside it start somewhere else on every row.
   let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, CONTROL), egui::Sense::hover());

   let mut cell = ui.new_child(
      egui::UiBuilder::new()
         .max_rect(rect)
         .layout(egui::Layout::left_to_right(egui::Align::Center)),
   );

   let text = cell.add(
      egui::Label::new(RichText::new(label).size(theme.typography.normal))
         .extend()
         .truncate(),
   );

   ui.ctx().accesskit_node_builder(text.id, |node| node.set_hidden());

   text
}

/// A percentage setting on the crate's own rail, with the number beside it.
///
/// One rail and one number, so the two ceilings read as the same kind of thing — a bound a user moves —
/// rather than as two hand-rolled number fields, which is what they were.
pub(super) fn percent_slider(
   ui: &mut Ui,
   theme: &Theme,
   label: &str,
   says: &'static str,
   stored: &mut u16,
   ceiling_bps: u16,
) -> bool {
   let mut percent = f64::from(*stored) / 100.0;
   let mut changed = false;

   ui.horizontal(|ui| {
      ui.spacing_mut().item_spacing.x = theme.spacing.sm;

      labelled(ui, theme, label);

      // Fixed decimals rather than a maximum, so the number beside the rail keeps one width for the
      // whole drag. A cell that widens when the number does makes the crate's own row re-measure
      // mid-drag, and the pass that places it then puts the rail at the row's far edge: the pointer
      // reads near zero and the slider snaps to the floor.
      let slider = ui.add(
         Slider::new(&mut percent, 0.0..=f64::from(ceiling_bps) / 100.0)
            .step_by(0.05)
            .width(RAIL_WIDTH)
            .fixed_decimals(2),
      );

      // The rail carries the setting; the chip beside it is where a reader hears what it bounds.
      ui.ctx().accesskit_node_builder(slider.id, |node| {
         node.set_label(label);
         node.set_description(says);
      });

      q_mark(ui, theme, says);

      changed = slider.changed();
   });

   if changed {
      *stored = (percent * 100.0).round().clamp(0.0, f64::from(u16::MAX)) as u16;
   }

   changed
}

/// Name what a control holds for a reader, without changing the words drawn beside it.
pub(super) fn describe(ui: &Ui, response: &egui::Response, description: impl Into<String>) {
   let description = description.into();

   ui.ctx().accesskit_node_builder(response.id, |node| {
      node.set_description(description)
   });
}

/// Save a control's meaning for a reader who lands on it without the sentence under it.
fn etiquetted(response: &egui::Response, ui: &Ui, label: &str, description: &str) {
   ui.ctx().accesskit_node_builder(response.id, |node| {
      node.set_label(label);
      node.set_description(description);
   });
}

/// Save the profile store off the frame path.
///
/// The save waits for the frame's own write guard, so it must not run on this thread: a blocking
/// task lands after the pass that asked for it, which is exactly once per user action.
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
   let seconds = now_secs().saturating_sub(at);

   match seconds {
      0..=59 => "just now".to_owned(),
      60..=3_599 => format!("{} minutes ago", seconds / 60),
      3_600..=86_399 => format!("{} hours ago", seconds / 3_600),
      _ => format!("{} days ago", seconds / 86_400),
   }
}

/// Unix seconds, the unit a stored timestamp uses.
fn now_secs() -> u64 {
   std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map(|elapsed| elapsed.as_secs())
      .unwrap_or(0)
}
