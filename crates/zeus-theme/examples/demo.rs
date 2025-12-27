use eframe::egui::*;
use secure_types::SecureString;
use std::sync::Arc;
use zeus_theme::{
   Theme, ThemeEditor, ThemeKind, utils,
   window::{WindowCtx, window_frame},
};
use zeus_widgets::{Button, ComboBox, Label, SecureTextEdit};

const _LOREM_IPSUM: &str = "Lorem ipsum dolor sit amet (Muted Text)";
const INTER_BOLD_18: &[u8] = include_bytes!("../../../src/assets/Inter_18pt-Bold.ttf");

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
   tx_confirm_window: TxConfirmWindow,
   recipient_window: RecipientWindow,
   msg_window: MsgWindow,
   string_value: SecureString,
   string_value2: String,
   current_chain: Chain,
   widgets_selected_color: BGColor,
   theme: Theme,
   editor: ThemeEditor,
}

struct TxConfirmWindow {
   open: bool,
   should_darken_bg: bool,
}

impl TxConfirmWindow {
   fn new() -> Self {
      Self {
         open: false,
         should_darken_bg: false,
      }
   }
}

struct RecipientWindow {
   open: bool,
   should_darken_bg: bool,
}

impl RecipientWindow {
   fn new() -> Self {
      Self {
         open: false,
         should_darken_bg: false,
      }
   }
}

struct MsgWindow {
   open: bool,
}

impl MsgWindow {
   fn new() -> Self {
      Self { open: false }
   }
}

impl DemoApp {
   fn new(cc: &eframe::CreationContext<'_>) -> Self {
      let theme = Theme::new(ThemeKind::Dark);
      let editor = ThemeEditor::new();
      cc.egui_ctx.set_style(theme.style.clone());

      // setup_fonts(&cc.egui_ctx);

      let tx_confirm_window = TxConfirmWindow::new();
      let recipient_window = RecipientWindow::new();
      let string_value = SecureString::new().unwrap();
      let string_value2 = String::new();

      Self {
         set_theme: false,
         check: false,
         tx_confirm_window,
         recipient_window,
         msg_window: MsgWindow::new(),
         string_value,
         string_value2,
         current_chain: Chain::Ethereum,
         widgets_selected_color: BGColor::BG1,
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
      // println!("Overlay counter: {}", self.theme.overlay_manager.counter());
      let theme2 = self.theme.clone();

      let window = WindowCtx::new("egui Theme Demo", 40.0, &theme2);

      window_frame(ctx, window, |ui| {
         utils::apply_theme_changes(&mut self.theme, ui);

         self.left_panel(ui);
         self.central_panel(ui);
      });
   }
}

impl DemoApp {
   fn central_panel(&mut self, ui: &mut Ui) {
      let bg = self.theme.colors.bg;
      let frame = Frame::new().fill(bg);

      egui::CentralPanel::default().frame(frame).show_inside(ui, |ui| {
         if !self.set_theme {
            ui.ctx().set_style(self.theme.style.clone());
            self.set_theme = true;
         }

         let new_theme = self.editor.show(&mut self.theme, ui);
         if let Some(new_theme) = new_theme {
            self.theme = new_theme;
         }

         if self.tx_confirm_window.open {
            match self.msg_window.open {
               false => self.tx_confirm_window.should_darken_bg = false,
               true => self.tx_confirm_window.should_darken_bg = true,
            }
            self.tx_confirm_window.show(&mut self.theme, ui);
         }

         if self.recipient_window.open {
            match self.msg_window.open {
               false => self.recipient_window.should_darken_bg = false,
               true => self.recipient_window.should_darken_bg = true,
            }
            self.recipient_window.show(&mut self.theme, ui);
         }

         if self.msg_window.open {
            self.msg_window.show(&mut self.theme, ui);
         }

         ScrollArea::vertical().show(ui, |ui| {
            ui.vertical_centered(|ui| {
               ui.add_space(50.0);

               let text = RichText::new("BG Dark Color").size(self.theme.text_sizes.heading);
               ui.label(text);

               ui.add_space(20.0);

               self.text_sizes(ui);

               ui.add_space(20.0);

               self.bg_colors_on_frames(ui);

               ui.add_space(20.0);

               let text = RichText::new("Widgets").size(self.theme.text_sizes.heading);
               ui.label(text);

               ui.add_space(20.0);

               self.widgets(ui);

               ui.add_space(20.0);
            });
         });
      });
   }

   fn left_panel(&mut self, ui: &mut Ui) {
      let bg = self.theme.colors.bg3;
      let frame = Frame::new().fill(bg);

      egui::SidePanel::left("left_panel")
         .min_width(150.0)
         .max_width(150.0)
         .resizable(false)
         .show_separator_line(true)
         .frame(frame)
         .show_inside(ui, |ui| {
            utils::bg_color_on_idle(ui, Color32::TRANSPARENT);
            utils::no_border_on_idle(ui);
            ui.set_width(140.0);

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
                  self.tx_confirm_window.open(&mut self.theme);
               }

               let text = RichText::new("Recipient Window").size(text_size);
               let button = Button::new(text).min_size(button_size);
               if ui.add(button).clicked() {
                  self.recipient_window.open(&mut self.theme);
               }

               let text = RichText::new("Msg Window").size(text_size);
               let button = Button::new(text).min_size(button_size);
               if ui.add(button).clicked() {
                  self.msg_window.open(&mut self.theme);
               }

               let about_text = RichText::new("About").size(text_size);
               let about_button = Button::new(about_text).min_size(button_size);
               ui.add(about_button);
            });
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

   fn bg_colors_on_frames(&mut self, ui: &mut Ui) {
      let frame1 = self.theme.frame1;
      let frame2 = self.theme.frame2;
      let frame3 = self.theme.frame2.fill(self.theme.colors.bg4);

      frame1.show(ui, |ui| {
         ui.set_width(250.0);
         ui.set_height(200.0);

         let text = RichText::new("BG2 Color").size(self.theme.text_sizes.large);
         ui.label(text);

         frame2.show(ui, |ui| {
            ui.set_width(200.0);
            ui.set_height(100.0);
            let text = RichText::new("BG3 Color").size(self.theme.text_sizes.large);
            ui.label(text);

            frame3.show(ui, |ui| {
               ui.set_width(150.0);
               ui.set_height(50.0);
               let text = RichText::new("BG4 Color").size(self.theme.text_sizes.large);
               ui.label(text);
            });
         });
      });
   }

   fn widgets(&mut self, ui: &mut Ui) {
      ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
      ui.spacing_mut().button_padding = vec2(10.0, 8.0);
      let button_size = vec2(100.0, 30.0);

      let bg_color = match self.widgets_selected_color {
         BGColor::BG1 => self.theme.colors.bg,
         BGColor::BG2 => self.theme.colors.bg2,
         BGColor::BG3 => self.theme.colors.bg3,
         BGColor::BG4 => self.theme.colors.bg4,
      };

      let button_visuals = match self.widgets_selected_color {
         BGColor::BG1 => self.theme.colors.button_visuals_1,
         BGColor::BG2 => self.theme.colors.button_visuals_2,
         BGColor::BG3 => self.theme.colors.button_visuals_3,
         BGColor::BG4 => self.theme.colors.button_visuals_3,
      };

      let label_visuals = match self.widgets_selected_color {
         BGColor::BG1 => self.theme.colors.label_visuals_1,
         BGColor::BG2 => self.theme.colors.label_visuals_2,
         BGColor::BG3 => self.theme.colors.label_visuals_3,
         BGColor::BG4 => self.theme.colors.label_visuals_3,
      };

      let combo_visuals = match self.widgets_selected_color {
         BGColor::BG1 => self.theme.colors.combo_box_visuals_1,
         BGColor::BG2 => self.theme.colors.combo_box_visuals_2,
         BGColor::BG3 => self.theme.colors.combo_box_visuals_3,
         BGColor::BG4 => self.theme.colors.combo_box_visuals_3,
      };

      let text_edit_visuals = match self.widgets_selected_color {
         BGColor::BG1 => self.theme.colors.text_edit_visuals_1,
         BGColor::BG2 => self.theme.colors.text_edit_visuals_2,
         BGColor::BG3 => self.theme.colors.text_edit_visuals_3,
         BGColor::BG4 => self.theme.colors.text_edit_visuals_3,
      };

      let text = format!(
         "Selected BG Color: {}",
         self.widgets_selected_color.to_str()
      );
      let text = RichText::new(text).size(self.theme.text_sizes.normal);
      ui.label(text);

      let frame = Frame::new().inner_margin(Margin::same(10)).fill(bg_color);

      ui.allocate_ui(button_size, |ui| {
         let bgs = vec![BGColor::BG1, BGColor::BG2, BGColor::BG3, BGColor::BG4];
         let selected_text =
            RichText::new(self.widgets_selected_color.to_str()).size(self.theme.text_sizes.normal);
         let selected_label = Label::new(selected_text, None).visuals(label_visuals);
         ComboBox::new("combox_box", selected_label)
            .width(150.0)
            .visuals(combo_visuals)
            .show_ui(ui, |ui| {
               for bg in bgs {
                  let text = RichText::new(bg.to_str()).size(self.theme.text_sizes.normal);
                  let label = Label::new(text, None)
                     .visuals(label_visuals)
                     .expand(Some(3.0))
                     .selected(bg == self.widgets_selected_color)
                     .sense(Sense::click())
                     .fill_width(true);

                  if ui.add(label).clicked() {
                     self.widgets_selected_color = bg;
                  }
                  ui.add_space(4.0);
               }
            });
      });

      frame.show(ui, |ui| {
         let text = RichText::new("Button 1").size(self.theme.text_sizes.normal);
         let button = Button::new(text).visuals(button_visuals).min_size(button_size);
         ui.add(button);

         let text = RichText::new("Button (Selected)").size(self.theme.text_sizes.normal);
         let button =
            Button::new(text).visuals(button_visuals).selected(true).min_size(button_size);
         ui.add(button);

         let text = RichText::new("Combo Box").size(self.theme.text_sizes.normal);
         ui.label(text);

         let all_chains = Chain::all();
         let current = self.current_chain;
         let text = RichText::new(current.to_str()).size(self.theme.text_sizes.normal);
         let selected_label = Label::new(text, None).visuals(label_visuals);

         ui.allocate_ui(button_size, |ui| {
            ComboBox::new("combox_box", selected_label)
               .width(150.0)
               .visuals(combo_visuals)
               .show_ui(ui, |ui| {
                  for chain in all_chains {
                     let text = RichText::new(chain.to_str()).size(self.theme.text_sizes.normal);
                     let label = Label::new(text, None)
                        .visuals(label_visuals)
                        .expand(Some(3.0))
                        .selected(current == chain)
                        .sense(Sense::click())
                        .fill_width(true);

                     if ui.add(label).clicked() {
                        self.current_chain = chain;
                     }
                     ui.add_space(4.0);
                  }
               });
         });

         let text = RichText::new("Label (Interactive)").size(self.theme.text_sizes.normal);
         let label = Label::new(text, None).expand(Some(6.0)).visuals(label_visuals);
         ui.add(label);

         let text = RichText::new("Checkbox").size(self.theme.text_sizes.normal);
         ui.checkbox(&mut self.check, text);

         let text = RichText::new("Radio").size(self.theme.text_sizes.normal);
         ui.radio_value(&mut self.check, true, text);

         let text = RichText::new("Slider").size(self.theme.text_sizes.normal);
         ui.label(text);

         ui.allocate_ui(button_size, |ui| {
            ui.add(Slider::new(&mut self.min_value, 0.0..=100.0));
         });

         let text = RichText::new("Secure Text Edit").size(self.theme.text_sizes.normal);
         ui.label(text);

         let hint = RichText::new("Write something")
            .size(self.theme.text_sizes.normal)
            .color(self.theme.colors.text_muted);
         ui.add(
            SecureTextEdit::singleline(&mut self.string_value)
               .visuals(text_edit_visuals)
               .hint_text(hint.clone())
               .margin(Margin::same(10))
               .desired_width(200.0)
               .font(FontId::proportional(self.theme.text_sizes.normal)),
         );

         let text = RichText::new("Nomrmal Text Edit").size(self.theme.text_sizes.normal);
         ui.label(text);

         ui.add(
            TextEdit::singleline(&mut self.string_value2)
               .hint_text(hint)
               .margin(Margin::same(10))
               .desired_width(200.0)
               .font(FontId::proportional(self.theme.text_sizes.normal)),
         );
      });
   }
}

impl TxConfirmWindow {
   fn open(&mut self, theme: &mut Theme) {
      if !self.open {
         theme.overlay_manager.window_opened();
      }
      self.open = true;
   }

   fn close(&mut self, theme: &mut Theme) {
      if self.open {
         theme.overlay_manager.window_closed();
      }
      self.open = false;
   }

   fn show(&mut self, theme: &mut Theme, ui: &mut Ui) {
      let frame = Frame::window(&theme.style);

      Window::new("Tx Confirm")
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            let frame = theme.frame2.outer_margin(Margin::same(0));
            let text_size = theme.text_sizes.large;
            let button_visuals = theme.colors.button_visuals_2.clone();
            // let font_bold = FontFamily::Name("inter_bold".into());

            Frame::new().inner_margin(Margin::same(5)).show(ui, |ui| {
               ui.vertical_centered(|ui| {
                  ui.set_width(500.0);
                  ui.set_height(350.0);
                  ui.spacing_mut().item_spacing = vec2(0.0, 15.0);
                  ui.spacing_mut().button_padding = vec2(10.0, 8.0);

                  let heading = RichText::new("Swap").size(theme.text_sizes.heading);
                  ui.label(heading);

                  frame.show(ui, |ui| {
                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           let text =
                              RichText::new("- 1 WETH").color(theme.colors.error).size(text_size);
                           ui.label(text);
                        });

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           let text = RichText::new("$1,600").size(text_size);
                           ui.label(text);
                        });
                     });

                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           let text = RichText::new("+ 1,600 DAI")
                              .color(theme.colors.success)
                              .size(text_size);
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
                              RichText::new("Mike").size(text_size).color(theme.colors.info);
                           ui.hyperlink_to(text, "https://www.google.com");
                        });
                     });

                     ui.horizontal(|ui| {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                           let text = RichText::new("Contract interaction").size(text_size);
                           ui.label(text);
                        });

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                           let text = RichText::new("Uniswap: Universal Router V2")
                              .size(text_size)
                              .color(theme.colors.info);
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
                        let button =
                           Button::new(text).visuals(button_visuals.clone()).min_size(button_size);
                        if ui.add(button).clicked() {
                           self.close(theme);
                        }

                        ui.add_space(10.0);

                        let text = RichText::new("Reject").size(theme.text_sizes.normal);
                        let button =
                           Button::new(text).visuals(button_visuals).min_size(button_size);
                        if ui.add(button).clicked() {
                           self.close(theme);
                        }
                     });
                  });
               });
            });
         });
   }
}

impl MsgWindow {
   fn open(&mut self, theme: &mut Theme) {
      if !self.open {
         theme.overlay_manager.window_opened();
      }
      self.open = true;
   }

   fn close(&mut self, theme: &mut Theme) {
      if self.open {
         theme.overlay_manager.window_closed();
      }
      self.open = false;
   }

   fn show(&mut self, theme: &mut Theme, ui: &mut Ui) {
      let frame = Frame::window(&theme.style);
      let button_visuals = theme.colors.button_visuals_2.clone();

      Window::new("Msg")
         .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
         .order(Order::Debug)
         .title_bar(false)
         .resizable(false)
         .collapsible(false)
         .frame(frame)
         .show(ui.ctx(), |ui| {
            ui.set_width(250.0);
            ui.set_height(150.0);

            ui.vertical_centered(|ui| {
               ui.spacing_mut().button_padding = vec2(10.0, 8.0);

               let text = RichText::new("Hello World!").size(theme.text_sizes.normal);
               ui.label(text);

               ui.add_space(10.0);

               let text = RichText::new("Close").size(theme.text_sizes.normal);
               let button = Button::new(text).visuals(button_visuals).min_size(vec2(100.0, 45.0));
               if ui.add(button).clicked() {
                  self.close(theme);
               }
            });
         });
   }
}

impl RecipientWindow {
   fn open(&mut self, theme: &mut Theme) {
      if !self.open {
         theme.overlay_manager.window_opened();
      }
      self.open = true;
   }

   fn close(&mut self, theme: &mut Theme) {
      if self.open {
         theme.overlay_manager.window_closed();
      }
      self.open = false;
   }

   fn show(&mut self, theme: &mut Theme, ui: &mut Ui) {
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

               let frame = theme.frame2;
               let button_visuals = theme.colors.button_visuals_3.clone();
               let size = vec2(ui.available_width() * 0.7, 45.0);

               ui.allocate_ui(size, |ui| {
                  let text = RichText::new("Contacts").size(theme.text_sizes.heading);
                  ui.label(text);

                  ui.add_space(10.0);

                  for _ in 0..5 {
                     frame.show(ui, |ui| {
                        ui.horizontal(|ui| {
                           let text = RichText::new("John Doe").size(theme.text_sizes.normal);
                           let button = Button::new(text).visuals(button_visuals.clone());
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

               let button_visuals = theme.colors.button_visuals_2.clone();

               let close = Button::new(RichText::new("Close").size(theme.text_sizes.normal))
                  .visuals(button_visuals)
                  .min_size(vec2(100.0, 45.0));
               if ui.add(close).clicked() {
                  self.close(theme);
               }
            });
         });
   }
}

#[derive(Clone, Copy, PartialEq)]
enum BGColor {
   BG1,
   BG2,
   BG3,
   BG4,
}

impl BGColor {
   fn to_str(&self) -> &'static str {
      match self {
         BGColor::BG1 => "BG1",
         BGColor::BG2 => "BG2",
         BGColor::BG3 => "BG3",
         BGColor::BG4 => "BG4",
      }
   }
}

pub fn setup_fonts(ctx: &egui::Context) {
   // Start with defaults to keep built-in fonts.
   let mut fonts = FontDefinitions::default();

   let font = FontData::from_static(INTER_BOLD_18);
   fonts.font_data.insert("inter_bold".to_owned(), Arc::new(font));

   // Bind the font to the custom named family (this is the key step missing from add_font).
   let mut newfam = std::collections::BTreeMap::new();
   newfam.insert(
      FontFamily::Name("inter_bold".into()),
      vec!["inter_bold".to_owned()],
   );
   fonts.families.append(&mut newfam);

   // Apply once.
   ctx.set_fonts(fonts);
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
