//! Settings window: category nav + inlined pages.

use crate::assets::icons::Icons;
use crate::core::{ZeusContext, ZeusCtx};
use crate::gui::appearance::AppearanceSettings;
use crate::gui::ui::{
   common::{heading, privacy_mode_switch},
   shortcuts::KeyBindings,
   show_with_fade,
};
use egui::{Key, Response, RichText, Role, ScrollArea, Shadow, Stroke, Ui, WidgetInfo, vec2};
use egui_elements::{Fill, Frame as Frame2, Label, Theme};
use egui_lucide::Lucide;
use std::sync::Arc;

pub mod agents;
pub mod appearance;
pub mod autolock;
pub mod change_credentials;
pub mod contacts;
pub mod encryption;
pub mod export;
pub mod general;
pub mod import;
pub mod keyboard;
pub mod networks;
pub mod railgun;
pub mod theme;

pub use agents::AgentsUi;
pub use appearance::AppearanceSettingsUi;
pub use autolock::AutoLockSettings;
pub use change_credentials::ChangeCredentialsUi;
pub use contacts::ContactsUi;
pub use encryption::EncryptionSettings;
pub use export::ExportDataUi;
pub use general::GeneralSettings;
pub use import::ImportDataUi;
pub use keyboard::KeyboardSettings;
pub use networks::NetworkSettings;
pub use railgun::RailgunSettings;
pub use theme::ThemeSettings;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsPage {
   General,
   Appearance,
   Keyboard,
   Networks,
   Security,
   Contacts,
   Railgun,
   Agents,
   Data,
}

impl SettingsPage {
   /// The page's word, shared by its tab and its heading so the two cannot disagree.
   pub fn name(&self) -> &'static str {
      match self {
         Self::General => "General",
         Self::Appearance => "Appearance",
         Self::Keyboard => "Keyboard",
         Self::Networks => "Networks",
         Self::Security => "Security",
         Self::Contacts => "Contacts",
         Self::Railgun => "Railgun",
         Self::Agents => "Agents",
         Self::Data => "Data",
      }
   }
}

pub struct SettingsUi {
   open: bool,
   /// Set when the user closed the window itself — its Close button or Escape — so the left panel
   /// can put the focus back on the plate that opened it.
   focus_return: bool,
   page: SettingsPage,
   /// Whether an assistive technology is attached. The Appearance page is how Zeus looks and
   /// feels, so it is not offered to a reader.
   reader_attached: bool,
   /// The page whose heading has already been given the focus, so a change of page moves it once.
   last_page: Option<SettingsPage>,
   /// Set when the window opens, so its first frame puts the keyboard on the page switcher rather
   /// than on the page: the bar is the window's first control, and a reader who Tabs expects to walk
   /// it before the page, the way Windows Settings hands over its own sidebar.
   focus_nav: bool,
   /// Set when the plate of the page in front is pressed, so the keyboard goes to the page even
   /// though nothing changed: asking for the page already open is still asking for it.
   focus_page: bool,
   /// The words a page asked the app to say, drained right after the window is drawn.
   notice: Option<String>,
   /// The context the pages that start background work need.
   ctx: ZeusCtx,
   agents: AgentsUi,
   general: GeneralSettings,
   appearance: AppearanceSettingsUi,
   keyboard: KeyboardSettings,
   pub encryption: EncryptionSettings,
   pub autolock: AutoLockSettings,
   pub network: NetworkSettings,
   theme: ThemeSettings,
   pub contacts_ui: ContactsUi,
   pub change_credentials_ui: ChangeCredentialsUi,
   pub export: ExportDataUi,
   pub import: ImportDataUi,
   pub railgun: RailgunSettings,
}

impl SettingsUi {
   pub fn new(handle: &ZeusCtx, appearance: &mut AppearanceSettings) -> Self {
      let (general, railgun) = handle.write(|ctx| {
         (
            GeneralSettings::new(ctx),
            RailgunSettings::new(ctx),
         )
      });

      Self {
         open: false,
         focus_return: false,
         page: SettingsPage::General,
         reader_attached: false,
         last_page: None,
         focus_nav: false,
         focus_page: false,
         notice: None,
         ctx: handle.clone(),
         agents: AgentsUi::new(),
         general,
         appearance: AppearanceSettingsUi::new(appearance),
         keyboard: KeyboardSettings::new(),
         encryption: EncryptionSettings::new(),
         autolock: AutoLockSettings::new(),
         network: NetworkSettings::new(),
         theme: ThemeSettings::new(),
         contacts_ui: ContactsUi::new(),
         change_credentials_ui: ChangeCredentialsUi::new(),
         export: ExportDataUi::new(),
         import: ImportDataUi::new(),
         railgun,
      }
   }

   /// Draw or drop the field controls that only a sighted user can use.
   pub fn set_field_controls(&mut self, shown: bool) {
      self.import.set_field_controls(shown);
      self.change_credentials_ui.set_field_controls(shown);
      self.contacts_ui.set_field_controls(shown);
   }

   pub fn erase(&mut self) {
      self.change_credentials_ui.erase();
      self.import.erase();
   }

   pub fn is_open(&self) -> bool {
      self.open
   }

   /// Tell the window whether a screen reader is attached.
   pub fn set_reader_attached(&mut self, attached: bool) {
      self.reader_attached = attached;
   }

   pub fn open(&mut self, ctx: &mut ZeusContext, appearance: &mut AppearanceSettings) {
      if !self.open {
         self.open = true;
         self.focus_nav = true;
         self.general.sync_from_ctx(ctx);
         self.appearance.sync_from_settings(appearance);
         self.encryption.sync_from_ctx(ctx);
         self.autolock.sync_from_ctx(ctx);
         self.railgun.sync_from_ctx(ctx);
      }
   }

   pub fn close(&mut self) {
      if self.open {
         self.promote_network_notice();
         self.promote_contacts_notice();
         self.promote_keyboard_notice();
         self.promote_agents_notice();
      }
      self.open = false;
      self.change_credentials_ui.reset();
      // Drop any typed import credentials: the form redraws them unconditionally
      // (`with_open(true)`), so a vault password left in the field would be shown
      // again the next time Settings opens — e.g. after an auto-lock/unlock.
      self.import.erase();
      self.contacts_ui.reset_page();
      self.agents.reset();
      self.keyboard.reset();
      self.network.reset_view();
      // Reopening lands on the open page's heading again, even though the page itself did not change.
      self.last_page = None;
   }

   /// Whether the user closed the window itself since the last call, which asks the left panel to
   /// put the focus back on the plate that opened it.
   ///
   /// Only a close the user asked for is reported: when the wallet locks or a page closes Settings,
   /// the focus is either gone with the lock screen or still where that page left it.
   pub fn take_focus_return(&mut self) -> bool {
      std::mem::take(&mut self.focus_return)
   }

   /// The words this frame should say, drained by the app right after the window is drawn.
   pub fn take_notice(&mut self) -> Option<String> {
      self.notice.take()
   }

   /// Whether the Keyboard page is recording a chord.
   ///
   /// The shortcut layer hands the whole keyboard over while it is: a key taken here would make the
   /// chord being recorded the chord that runs something.
   pub fn is_capturing(&self) -> bool {
      self.keyboard.is_capturing()
   }

   /// Whether the General page asked for the keyboard shortcut sheet since the last call.
   ///
   /// The window is drawn after the frame has drawn the sheet, so the app opens it here and the
   /// next pass puts it up over the window it was opened from.
   pub fn take_shortcuts_request(&mut self) -> bool {
      self.general.take_shortcuts_request()
   }

   /// Hand a notice the Networks page raised up to the window, where the app drains it.
   fn promote_network_notice(&mut self) {
      if let Some(text) = self.network.take_notice() {
         self.notice = Some(text);
      }
   }

   /// Hand a notice the Contacts page raised up to the window, where the app drains it.
   fn promote_contacts_notice(&mut self) {
      if let Some(text) = self.contacts_ui.take_notice() {
         self.notice = Some(text);
      }
   }

   /// Hand a notice the Keyboard page raised up to the window, where the app drains it.
   fn promote_keyboard_notice(&mut self) {
      if let Some(text) = self.keyboard.take_notice() {
         self.notice = Some(text);
      }
   }

   /// Hand a notice the Agents page raised up to the window, where the app drains it.
   fn promote_agents_notice(&mut self) {
      if let Some(text) = self.agents.take_notice() {
         self.notice = Some(text);
      }
   }

   pub fn open_page(
      &mut self,
      page: SettingsPage,
      ctx: &mut ZeusContext,
      appearance: &mut AppearanceSettings,
   ) {
      self.set_page(page, ctx);
      self.open(ctx, appearance);
   }

   pub fn open_network_settings(
      &mut self,
      ctx: &mut ZeusContext,
      appearance: &mut AppearanceSettings,
   ) {
      self.open_page(SettingsPage::Networks, ctx, appearance);
   }

   fn set_page(&mut self, page: SettingsPage, ctx: &mut ZeusContext) {
      if page == SettingsPage::Security {
         self.encryption.sync_from_ctx(ctx);
         self.autolock.sync_from_ctx(ctx);
      }
      if page == SettingsPage::Railgun {
         self.railgun.sync_from_ctx(ctx);
      }
      if self.page == SettingsPage::Contacts && page != SettingsPage::Contacts {
         // Leaving the page stops drawing the address fields; stop their scanners.
         self.contacts_ui.close_qr_scanners();
      }
      if self.page == SettingsPage::Keyboard && page != SettingsPage::Keyboard {
         // A capture begun here must not follow the user to another page.
         self.keyboard.reset();
      }
      self.page = page;
   }

   /// Draw the Settings window in the root viewport.
   ///
   /// It used to be a second OS window, and a second OS window has no accessibility tree at all:
   /// nothing inside it — the nav, a page, a message raised from one — was reachable by a reader.
   /// Here it is drawn by the same pass as the main window, so everything in it joins the tree the
   /// app already builds, and the window itself is a named `Role::Window` with a Close button.
   pub fn show(
      &mut self,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      theme: &Theme,
      appearance: &mut AppearanceSettings,
      keymap: &mut KeyBindings,
      server: &mut Option<zeus_mcp::ServerHandle>,
      ui: &mut Ui,
   ) {
      if ctx.locked {
         // A locked wallet must not leave Settings usable.
         if self.open {
            self.close();
         }
         return;
      }

      if !self.open {
         return;
      }

      let mut open = true;
      let mut escaped = false;

      // Left on the default `Order`: the ambient particle pass paints on `Order::Foreground`
      // over the whole app, so its particles fall over this window as over every other surface.
      egui::Window::new("Settings")
         .id(egui::Id::unique("zeus_settings_window"))
         .collapsible(false)
         .resizable(true)
         .default_size([1120.0, 720.0])
         .open(&mut open)
         .show(ui.ctx(), |ui| {
            self.paint(ctx, icons, theme, appearance, keymap, server, ui);
            escaped = escape_closes_window(ui);
         });

      if !open || escaped {
         self.close();
         self.focus_return = true;
      }
   }

   fn paint(
      &mut self,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      theme: &Theme,
      appearance: &mut AppearanceSettings,
      keymap: &mut KeyBindings,
      server: &mut Option<zeus_mcp::ServerHandle>,
      ui: &mut Ui,
   ) {
      // Tab belongs to this window while it is up. Without a modal layer egui hands the keyboard to
      // the page behind once the window's last control is walked, so a reader who was working
      // through the pages suddenly hears the chain select of the page that opened Settings. The
      // layer is the window's own: what is drawn above it — a dialog raised from a page, the
      // shortcut sheet — still takes the keyboard, and the layer is gone the frame the window is.
      ui.ctx().memory_mut(|memory| memory.set_modal_layer(ui.layer_id()));

      self.show_shell(ctx, icons, theme, appearance, keymap, server, ui);
   }

   fn show_shell(
      &mut self,
      ctx: &mut ZeusContext,
      icons: Arc<Icons>,
      theme: &Theme,
      appearance: &mut AppearanceSettings,
      keymap: &mut KeyBindings,
      server: &mut Option<zeus_mcp::ServerHandle>,
      ui: &mut Ui,
   ) {
      // The page a reader is not offered cannot stay in front: its plate is gone from the nav, so
      // a window left on Appearance when a reader attaches falls back to General.
      if self.reader_attached && self.page == SettingsPage::Appearance {
         self.set_page(SettingsPage::General, ctx);
      }

      let focus_nav = std::mem::take(&mut self.focus_nav);
      let nav_bg = theme.frame1.fill;
      let selected_plate = egui::Panel::left("settings_nav")
         .min_size(180.0)
         .max_size(180.0)
         .resizable(false)
         .show_separator_line(false)
         .frame(egui::Frame::new().fill(nav_bg).inner_margin(8.0))
         .show(ui, |ui| {
            // The plates are one page switcher: named here so a reader knows what they belong
            // to before it walks them.
            ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
               node.set_role(Role::TabList);
               node.set_label("Settings pages");
            });

            self.nav(ctx, theme, focus_nav, ui)
         })
         .inner;

      egui::CentralPanel::default()
         .frame(egui::Frame::new().fill(theme.colors.bg).inner_margin(16.0))
         .show(ui, |ui| {
            // The page's first node, and never inside the scroll area, so it neither moves nor
            // scrolls away. A page change moves the focus here, and that focus move is what makes a
            // reader say the page's name — the name is not drawn or announced anywhere else.
            let page_heading = heading(ui, self.page.name());

            if self.last_page != Some(self.page) {
               // Opening the window lands on the switcher instead. A page change — the reader
               // picking another tab — lands here, and that focus move is what says the page's
               // name: the name is not drawn or announced anywhere else.
               if !focus_nav {
                  page_heading.request_focus();
               }

               self.last_page = Some(self.page);
            }

            // Asking for the page already in front is still asking for it: the keyboard goes to the
            // page, not to the plate that was pressed.
            if std::mem::take(&mut self.focus_page) {
               page_heading.request_focus();
            }

            ScrollArea::vertical().auto_shrink([false; 2]).show(ui, |ui| {
               egui::Frame::new().inner_margin(10.0).show(ui, |ui| {
                  ui.set_width(ui.available_width());
                  let page = self.page;

                  show_with_fade(
                     ui,
                     "settings_general_ui_fade",
                     page == SettingsPage::General,
                     |ui| {
                        self.general.show(ctx, theme, self.reader_attached, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_appearance_ui_fade",
                     page == SettingsPage::Appearance,
                     |ui| {
                        self.theme.show(theme, ui);
                        ui.add_space(theme.spacing.xl);
                        ui.separator();
                        ui.add_space(theme.spacing.xl);
                        self.appearance.show(appearance, theme, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_keyboard_ui_fade",
                     page == SettingsPage::Keyboard,
                     |ui| {
                        ui.add_space(10.0);
                        self.keyboard.show(keymap, theme, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_networks_ui_fade",
                     page == SettingsPage::Networks,
                     |ui| {
                        self.network.show(ctx, theme, icons.clone(), ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_security_ui_fade",
                     page == SettingsPage::Security,
                     |ui| {
                        ui.add_space(10.0);
                        self.autolock.show(ctx, theme, ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(16.0);
                        self.change_credentials_ui.show(theme, ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(16.0);
                        self.encryption.show(theme, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_contacts_ui_fade",
                     page == SettingsPage::Contacts,
                     |ui| {
                        self.contacts_ui.show_page(ctx, theme, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_railgun_ui_fade",
                     page == SettingsPage::Railgun,
                     |ui| {
                        self.railgun.show(ctx, theme, icons.clone(), ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_data_ui_fade",
                     page == SettingsPage::Data,
                     |ui| {
                        self.export.show(theme, ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.add_space(16.0);
                        self.import.show_page(theme, ui);
                     },
                  );

                  show_with_fade(
                     ui,
                     "settings_agents_ui_fade",
                     page == SettingsPage::Agents,
                     |ui| {
                        self.agents.show(ctx, &self.ctx, server, theme, ui);
                     },
                  );
               });
            });
         });

      // Tab at the end of a page has nothing under it to move to: egui drops the keyboard for that
      // press, and the next one starts over from the top of the window. A Tab that says nothing
      // reads as a lost keyboard, so the walk goes on at the page switcher instead. Only a forward
      // Tab wraps — Shift+Tab at the top of the window is a reader backing out of the window's own
      // frame, the one place the key should stay put.
      let ran_out_of_page = ui.input(|input| input.key_pressed(Key::Tab) && !input.modifiers.shift)
         && ui.ctx().memory(|memory| memory.focused()).is_none();

      if ran_out_of_page && let Some(plate) = selected_plate {
         // The flag egui uses to hand the keyboard to the next widget that wants it is still set
         // by the press that ran out of page, so registering the plate as interested is what puts
         // the keyboard back on it. Requesting the focus alone would leave that flag standing, and
         // the window's own Close button — drawn before the bar — would take it next frame.
         ui.ctx()
            .memory_mut(|memory| memory.interested_in_focus(plate.id, plate.layer_id));
      }

      // A page's reset, copy or inline error raised its words; hand them up now, so the app can
      // say them in the frame that drew the page.
      self.promote_network_notice();
      self.promote_contacts_notice();
      self.promote_keyboard_notice();
      self.promote_agents_notice();
   }

   /// Draw the page switcher and return the plate of the page in front.
   ///
   /// That plate is the window's first control, so it is also where a forward Tab that runs out of
   /// page goes back to, and where the keyboard starts when the window opens.
   fn nav(
      &mut self,
      ctx: &mut ZeusContext,
      theme: &Theme,
      focus_nav: bool,
      ui: &mut Ui,
   ) -> Option<Response> {
      ui.spacing_mut().item_spacing = vec2(0.0, theme.spacing.xs);

      let icon_color = theme.colors.text;

      let mut items: Vec<(SettingsPage, egui::Image<'static>)> = vec![
         (
            SettingsPage::General,
            Lucide::SlidersHorizontal.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Appearance,
            Lucide::Palette.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Keyboard,
            Lucide::Keyboard.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Networks,
            Lucide::Globe.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Security,
            Lucide::LockKeyhole.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Contacts,
            Lucide::Users.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Railgun,
            Lucide::Shield.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Agents,
            Lucide::Bot.size(20.0).color(icon_color).image(),
         ),
         (
            SettingsPage::Data,
            Lucide::Archive.size(20.0).color(icon_color).image(),
         ),
      ];

      // Appearance is the theme, the ambient particles and the hold-to-confirm gesture: a page
      // about how Zeus looks and feels, which a reader has no use for.
      if self.reader_attached {
         items.retain(|(page, _)| *page != SettingsPage::Appearance);
      }

      privacy_mode_switch(ctx, theme, ui);
      ui.add_space(10.0);

      let mut selected_plate = None;

      for (page, icon) in items {
         let selected = self.page == page;
         let response = nav_plate(ui, theme, page.name(), icon, selected);

         // The window was just opened: the keyboard starts on the page the reader is on, so the
         // first Tab walks the rest of the bar instead of diving into the page.
         if focus_nav && selected {
            response.request_focus();
         }

         if response.clicked() {
            if selected {
               self.focus_page = true;
            }

            self.set_page(page, ctx);
         }

         if selected {
            selected_plate = Some(response);
         }
      }

      selected_plate
   }
}

/// One entry of the page switcher: a plate that carries its tab's role, name and selected state.
///
/// The frame registers a placeholder with an empty sense before its contents, under the id the
/// plate is about to take, and that withdraws the focus a Tab put on it — Enter and Space would
/// then never reach a plate reached by keyboard. `show` is `begin` + contents + `end`, so writing
/// the three out leaves a moment to put the focus back before the final registration.
///
/// The icon and word inside are an `egui_elements::Label`, a node of its own; hidden, so a reader
/// hears the entry once instead of "General tab, General".
fn nav_plate(
   ui: &mut Ui,
   theme: &Theme,
   text: &str,
   icon: egui::Image<'static>,
   selected: bool,
) -> Response {
   let mut visuals = theme.frame2_visuals();
   visuals.bg = Fill::Solid(theme.frame1.fill);
   visuals.border = Stroke::NONE;
   visuals.shadow = Shadow::NONE;

   let frame = Frame2::from_egui(theme.frame2)
      .interactive(true)
      .fill_width(true)
      .visuals(visuals)
      .corner_radius(0);

   let label = Label::new(
      RichText::new(text).size(theme.typography.normal),
      Some(icon),
   )
   .interactive(false)
   .image_on_left();

   let focused_before = ui.ctx().memory(|memory| memory.focused());

   let mut plate = frame.selected(selected).begin(ui);
   let id = ui.next_auto_id();

   if focused_before == Some(id) {
      ui.ctx().memory_mut(|memory| memory.request_focus(id));
   }

   let content = plate.content_ui.add(label);
   let response = plate.end(ui);

   response.widget_info(|| WidgetInfo::selected(Role::Tab, ui.is_enabled(), selected, text));

   ui.ctx().accesskit_node_builder(content.id, |node| node.set_hidden());

   response
}

/// Whether the Escape pressed this frame belongs to the Settings window.
///
/// A dialog raised from a Settings page must keep the first claim on the key: its backdrop and its
/// content are drawn on layers above the window, whatever order they use, so only a window that still
/// owns the top layer may close on it.
fn escape_closes_window(ui: &Ui) -> bool {
   if !ui.input(|input| input.key_pressed(Key::Escape)) {
      return false;
   }

   let layer = ui.layer_id();

   // An area drawn above this window — a dialog's backdrop or its content, on any order — takes the
   // key first; among the window order, only the area egui puts on top may close on it.
   ui.ctx().memory(|memory| {
      let above = memory
         .areas()
         .visible_layer_ids()
         .into_iter()
         .any(|other| other.order > layer.order);

      !above && ui.ctx().top_layer_id() == Some(layer)
   })
}
