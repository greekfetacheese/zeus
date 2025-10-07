use eframe::egui::*;
use egui_theme::{Theme, ThemeEditor, ThemeKind, utils, window::window_frame};

const LOREM_IPSUM: &str = "Lorem ipsum dolor sit amet (Muted Text)";

#[derive(Clone, Copy, PartialEq)]
enum Chain {
   Ethereum,
   Optimism,
   Base,
   BinanceSmartChain,
   Arbitrum,
}

impl Chain {
   fn to_str(&self) -> &'static str {
      match self {
         Chain::Ethereum => "Ethereum",
         Chain::Optimism => "Optimism",
         Chain::Base => "Base",
         Chain::BinanceSmartChain => "Binance Smart Chain",
         Chain::Arbitrum => "Arbitrum",
      }
   }

   fn all() -> Vec<Self> {
      vec![
         Self::Ethereum,
         Self::Optimism,
         Self::Base,
         Self::BinanceSmartChain,
         Self::Arbitrum,
      ]
   }
}

struct DemoApp {
   set_theme: bool,
   check: bool,
   min_value: f32,
   tx_confirm_window_open: bool,
   recipient_window_open: bool,
   string_value: String,
   current_chain: Chain,
   theme: Theme,
   editor: ThemeEditor,
}

impl DemoApp {
   fn new(cc: &eframe::CreationContext<'_>) -> Self {
      let theme = Theme::new(ThemeKind::Dark);
      let editor = ThemeEditor::new();
      cc.egui_ctx.set_style(theme.style.clone());

      Self {
         set_theme: false,
         check: false,
         tx_confirm_window_open: false,
         recipient_window_open: false,
         string_value: String::from("Hello World!"),
         current_chain: Chain::Ethereum,
         min_value: 0.0,
         theme,
         editor,
      }
   }
}

impl eframe::App for DemoApp {
   fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
      egui::Rgba::TRANSPARENT.to_array()
   }

   fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
      let theme2 = self.theme.clone();

      window_frame(ctx, "egui Theme Demo", theme2, |ui| {
         utils::apply_theme_changes(&mut self.theme, ui);

         self.left_panel(ui);

         ui.add_space(50.0);
         self.central_panel(ui);
      });
   }
}

impl DemoApp {
   fn central_panel(&mut self, ui: &mut Ui) {
      let bg_color = self.theme.colors.bg_dark;
      let frame = Frame::new().fill(bg_color);

      egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
         if !self.set_theme {
            ui.ctx().set_style(self.theme.style.clone());
            self.set_theme = true;
         }

         let new_theme = self.editor.show(&mut self.theme, ui);
         if let Some(new_theme) = new_theme {
            self.theme = new_theme;
         }

         if self.tx_confirm_window_open {
            tx_window(&self.theme, ui);
         }

         if self.recipient_window_open {
            recipient_window(&self.theme, ui);
         }

         ScrollArea::vertical().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
               ui.add_space(50.0);

               let text = RichText::new("BG Dark Color").size(self.theme.text_sizes.heading);
               ui.label(text);

               ui.add_space(20.0);

               theme_colors(&self.theme, ui);

               self.text_sizes(ui);

               self.nested_frames(ui);

               ui.add_space(20.0);

               self.interactive_frame(ui);

               ui.add_space(20.0);

               let text = RichText::new("Widgets").size(self.theme.text_sizes.heading);
               ui.label(text);

               ui.add_space(20.0);

               self.widgets(ui);
            });
         });
      });
   }

   fn left_panel(&mut self, ui: &mut Ui) {
      egui::SidePanel::left("left_panel")
         .min_width(150.0)
         .max_width(150.0)
         .resizable(false)
         .show_separator_line(false)
         .show_inside(ui, |ui| {
            utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
            utils::no_border_on_idle(ui);
            ui.set_width(140.0);

            let frame = self.theme.frame1;

            frame.show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  let text_size = self.theme.text_sizes.normal;
                  let button_size = vec2(100.0, 50.0);

                  let home_text = RichText::new("Home").size(text_size);
                  let home_button = Button::new(home_text).min_size(button_size);
                  ui.add(home_button);

                  let settings_text = RichText::new("Settings").size(text_size);
                  let settings_button = Button::new(settings_text).min_size(button_size);
                  ui.add(settings_button);

                  let editor_text = RichText::new("Toggle Editor").size(text_size);
                  let editor_button = Button::new(editor_text).min_size(button_size);
                  if ui.add(editor_button).clicked() {
                     self.editor.open = !self.editor.open;
                  }

                  let text = RichText::new("Tx Window").size(text_size);
                  let button = Button::new(text).min_size(button_size);
                  if ui.add(button).clicked() {
                     self.tx_confirm_window_open = !self.tx_confirm_window_open;
                  }

                  let text = RichText::new("Recipient Window").size(text_size);
                  let button = Button::new(text).min_size(button_size);
                  if ui.add(button).clicked() {
                     self.recipient_window_open = !self.recipient_window_open;
                  }

                  let about_text = RichText::new("About").size(text_size);
                  let about_button = Button::new(about_text).min_size(button_size);
                  ui.add(about_button);
               });
            });
         });
   }

   fn interactive_frame(&mut self, ui: &mut Ui) {
      let mut frame = self.theme.frame1;
      let visuals = self.theme.frame1_visuals;

      utils::frame_it(&mut frame, Some(visuals), ui, |ui| {
         ui.set_width(300.0);
         ui.set_height(200.0);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         let text = RichText::new("Interactive Frame").size(self.theme.text_sizes.heading);
         ui.label(text);

         let text = RichText::new("Button 1").size(self.theme.text_sizes.normal);
         ui.add(Button::new(text));

         let text = RichText::new("Button 2").size(self.theme.text_sizes.normal);
         ui.add(Button::new(text));
      });
   }

   fn text_sizes(&mut self, ui: &mut Ui) {
      let heading = RichText::new("Heading").size(self.theme.text_sizes.heading);
      ui.label(heading);

      let very_large = RichText::new("Very Large").size(self.theme.text_sizes.very_large);
      ui.label(very_large);

      let large = RichText::new("Large").size(self.theme.text_sizes.large);
      ui.label(large);

      let text = RichText::new("Normal").size(self.theme.text_sizes.normal);
      ui.label(text);

      let small = RichText::new("Small").size(self.theme.text_sizes.small);
      ui.label(small);

      let very_small = RichText::new("Very Small").size(self.theme.text_sizes.very_small);
      ui.label(very_small);
   }

   fn nested_frames(&mut self, ui: &mut Ui) {
      let base_frame = self.theme.frame1;
      let inner_frame = self.theme.frame2;
      let text_color = self.theme.colors.text;

      ui.add_space(50.0);

      base_frame.show(ui, |ui| {
         ui.set_width(350.0);
         ui.set_height(300.0);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         let heading =
            RichText::new("BG Color").size(self.theme.text_sizes.heading).color(text_color);
         ui.label(heading);

         inner_frame.show(ui, |ui| {
            let text = RichText::new("BG Light Color")
               .size(self.theme.text_sizes.large)
               .color(text_color);
            ui.label(text);

            // Muted text
            let text = RichText::new(LOREM_IPSUM)
               .size(self.theme.text_sizes.normal)
               .color(self.theme.colors.text_muted);
            ui.label(text);

            self.widgets(ui);
         });
      });
   }

   fn widgets(&mut self, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      let size = vec2(ui.available_width() * 0.7, 45.0);

      // Button 1
      let text = RichText::new("Delete").size(self.theme.text_sizes.normal);
      let button = Button::new(text);
      ui.add(button);

      // Button 2
      let text = RichText::new("Edit").size(self.theme.text_sizes.normal);
      let button = Button::new(text);
      ui.add(button);

      let text = RichText::new("Checkbox").size(self.theme.text_sizes.normal);
      ui.checkbox(&mut self.check, text);

      let text = RichText::new("Radio").size(self.theme.text_sizes.normal);
      ui.radio_value(&mut self.check, true, text);

      let text = RichText::new("Slider").size(self.theme.text_sizes.normal);
      ui.label(text);

      ui.allocate_ui(size, |ui| {
         ui.add(Slider::new(&mut self.min_value, 0.0..=100.0));
      });

      let text = RichText::new("Text Edit").size(self.theme.text_sizes.normal);
      ui.label(text);

      ui.add(
         TextEdit::singleline(&mut self.string_value)
            .margin(Margin::same(10))
            .desired_width(200.0)
            .font(FontId::proportional(self.theme.text_sizes.normal)),
      );

      let text = RichText::new("Combo Box").size(self.theme.text_sizes.normal);
      ui.label(text);

      let all_chains = Chain::all();
      let current = self.current_chain;
      let text = RichText::new(current.to_str()).size(self.theme.text_sizes.normal);

      ComboBox::from_label("").width(150.0).selected_text(text).show_ui(ui, |ui| {
         for chain in all_chains {
            let text = RichText::new(chain.to_str()).size(self.theme.text_sizes.normal);
            let value = ui.selectable_label(current == chain, text);
            if value.clicked() {
               self.current_chain = chain;
            }
            ui.add_space(3.0);
         }
      });
   }
}

fn recipient_selection(theme: &Theme, ui: &mut Ui) {
   let frame = theme.frame2;
   let size = vec2(ui.available_width() * 0.7, 45.0);

   ui.allocate_ui(size, |ui| {
      let text = RichText::new("Contacts").size(theme.text_sizes.heading);
      ui.label(text);

      ui.add_space(10.0);

      for _ in 0..5 {
         frame.show(ui, |ui| {
            ui.horizontal(|ui| {
               let text = RichText::new("John Doe").size(theme.text_sizes.normal);
               let button = Button::new(text);
               ui.add(button);

               ui.add_space(30.0);

               let text = RichText::new("0x0000...00000")
                  .size(theme.text_sizes.normal)
                  .color(theme.colors.info)
                  .strong();
               ui.hyperlink_to(text, "https://www.google.com");

               ui.add_space(30.0);

               let text = RichText::new("$1,600").size(theme.text_sizes.normal);
               ui.label(text);
            });
         });
      }
   });
}

fn theme_colors(theme: &Theme, ui: &mut Ui) {
   let layout = Layout::left_to_right(Align::Min).with_main_wrap(true);

   ui.with_layout(layout, |ui| {
      ui.spacing_mut().item_spacing = vec2(10.0, 10.0);

      let stroke_color = match theme.dark_mode {
         true => Color32::WHITE,
         false => Color32::BLACK,
      };

      let stroke = Stroke::new(1.0, stroke_color);

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("BG Dark");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.bg_dark, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("BG");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.bg, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("BG Light");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.bg_light, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Text");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.text, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Text Muted");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.text_muted, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Highlight");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.highlight, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Border");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.border, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Primary");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.primary, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Secondary");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.secondary, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Error");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.error, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Warning");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.warning, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Success");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.success, stroke);
      });

      ui.vertical(|ui| {
         ui.set_height(70.0);
         ui.label("Info");
         let painter = ui.painter();
         let pos = ui.min_rect().center();
         painter.circle(pos, 15.0, theme.colors.info, stroke);
      });
   });
}

fn tx_window(theme: &Theme, ui: &mut Ui) {
   let frame = Frame::window(&theme.style);

   Window::new("Tx Confirm")
      .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
      .title_bar(false)
      .resizable(false)
      .collapsible(false)
      .frame(frame)
      .show(ui.ctx(), |ui| {
         ui.set_min_width(400.0);
         ui.set_min_height(350.0);

         tx_confirm(theme, ui);
      });
}

fn recipient_window(theme: &Theme, ui: &mut Ui) {
   let frame = Frame::window(&theme.style);

   Window::new("Recipient")
      .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
      .title_bar(false)
      .resizable(false)
      .collapsible(false)
      .frame(frame)
      .show(ui.ctx(), |ui| {
         ui.vertical_centered(|ui| {
            ui.set_min_width(550.0);
            ui.set_min_height(400.0);
            ui.spacing_mut().button_padding = vec2(10.0, 8.0);

            recipient_selection(theme, ui);
         });
      });
}

fn tx_confirm(theme: &Theme, ui: &mut Ui) {
   let frame = theme.frame2.outer_margin(Margin::same(0));
   let text_size = theme.text_sizes.large;

   Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
      ui.vertical_centered(|ui| {
         ui.set_width(400.0);
         ui.set_height(350.0);
         ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
         ui.spacing_mut().button_padding = vec2(10.0, 8.0);

         let heading = RichText::new("Swap").size(theme.text_sizes.heading);
         ui.label(heading);

         frame.show(ui, |ui| {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("- 1 WETH").color(theme.colors.error).size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("$1,600").size(text_size);
                  ui.label(text);
               });
            });

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text =
                     RichText::new("+ 1,600 DAI").color(theme.colors.success).size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("$1,600").size(text_size);
                  ui.label(text);
               });
            });
         });

         frame.show(ui, |ui| {
            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("Chain").size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("Ethereum").size(text_size);
                  ui.label(text);
               });
            });

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("From").size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text =
                     RichText::new("Mike").size(text_size).color(theme.colors.info).strong();
                  ui.hyperlink_to(text, "https://www.google.com");
               });
            });

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("Contract interaction").size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("Universal Router")
                     .size(text_size)
                     .color(theme.colors.info)
                     .strong();
                  ui.hyperlink_to(
                     text,
                     "https://basescan.org/address/0x6fF5693b99212Da76ad316178A184AB56D299b43",
                  );
               });
            });

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("Value").size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("0 ETH ~ $0").size(text_size);
                  ui.label(text);
               });
            });

            ui.horizontal(|ui| {
               ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                  let text = RichText::new("Cost").size(text_size);
                  ui.label(text);
               });

               ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                  let text = RichText::new("0.000167 ETH ~ $0.75").size(text_size);
                  ui.label(text);
               });
            });
         });

         let size = vec2(ui.available_width() * 0.8, 45.0);
         ui.allocate_ui(size, |ui| {
            frame.show(ui, |ui| {
               ui.horizontal(|ui| {
                  let availabled_width = ui.available_width();
                  let fee_width = ui.available_width() * 0.3;
                  let gas_width = ui.available_width() * 0.5;

                  // Priority Fee
                  ui.vertical(|ui| {
                     let mut fee = String::from("1");
                     let text = "Priority Fee (Gwei)";
                     ui.label(RichText::new(text).size(theme.text_sizes.normal));

                     ui.add(
                        TextEdit::singleline(&mut fee)
                           .margin(Margin::same(10))
                           .background_color(theme.colors.bg)
                           .desired_width(fee_width)
                           .font(FontId::proportional(theme.text_sizes.normal)),
                     );
                  });

                  // Take the available space because otherwise the gas limit
                  // will not be pushed to the far right
                  ui.add_space(availabled_width - (fee_width + gas_width));

                  // Gas Limit
                  ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                     ui.vertical(|ui| {
                        let mut gas_limit = String::from("50000");
                        let text = "Gas Limit";
                        ui.label(RichText::new(text).size(theme.text_sizes.normal));

                        ui.add(
                           TextEdit::singleline(&mut gas_limit)
                              .margin(Margin::same(10))
                              .background_color(theme.colors.bg)
                              .desired_width(gas_width)
                              .font(FontId::proportional(theme.text_sizes.normal)),
                        );
                     });
                  });
               });
            });
         });

         ui.add_space(5.0);

         let text = RichText::new("MEV protect is not enabled")
            .size(text_size)
            .color(theme.colors.warning);
         ui.label(text);

         ui.add_space(10.0);

         let text = RichText::new("Insufficient funds to send transaction")
            .size(text_size)
            .color(theme.colors.error);
         ui.label(text);

         ui.add_space(10.0);

         let button_size = vec2(ui.available_width() * 0.5, 45.0);
         let size = vec2(ui.available_width(), 45.0);

         ui.allocate_ui(size, |ui| {
            ui.horizontal(|ui| {
               let text = RichText::new("Confirm").size(theme.text_sizes.normal);
               let button = Button::new(text).min_size(button_size);
               ui.add(button);

               ui.add_space(10.0);

               let text = RichText::new("Reject").size(theme.text_sizes.normal);
               let button = Button::new(text).min_size(button_size);
               ui.add(button);
            });
         });
      });
   });
}

fn main() -> eframe::Result {
   let options = eframe::NativeOptions {
      viewport: egui::ViewportBuilder::default()
         .with_decorations(false)
         .with_inner_size([1024.0, 800.0])
         .with_transparent(true)
         .with_resizable(true),
      ..Default::default()
   };

   eframe::run_native(
      "egui Theme Demo",
      options,
      Box::new(|cc| {
         let app = DemoApp::new(cc);
         Ok(Box::new(app))
      }),
   )
}
