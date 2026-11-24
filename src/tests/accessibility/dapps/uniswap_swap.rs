use super::super::app::{main_window, wallet_in_cache};
use super::super::harness::{accesskit_action, drawn_names, frame_nodes, is_text_input};
use super::{render_uniswap_window, weth_usdc};
use crate::gui::GUI;
use crate::gui::ui::dapps::uniswap::settings::{DEADLINE_TIP, SLIPPAGE_TIP, UniswapSettingsUi};
use crate::utils::swap_quoter::SwapStep;
use egui::accesskit::{Action, Node, NodeId, Role, Toggled};
use egui::{Context, Event};
use egui_elements::{Theme, ThemeKind};
use zeus_eth::alloy_primitives::{Address, U256};
use zeus_eth::amm::uniswap::{AnyUniswapPool, DexKind, UniswapPool, UniswapV3Pool};
use zeus_eth::utils::NumericValue;

/// The swap settings page on its own: no window and no fade around it, so one pass is the whole
/// page.
fn render_swap_settings(
   ctx: &Context,
   settings: &mut UniswapSettingsUi,
   theme: &Theme,
   events: Vec<Event>,
) -> Vec<(NodeId, Node)> {
   frame_nodes(ctx, events, |ui| {
      settings.show(true, false, theme, ui)
   })
}

/// The pool the simulate page lists a fee for: one V3 WETH/USDC pool at 0.05%.
fn weth_usdc_pool() -> UniswapV3Pool {
   let (weth, usdc) = weth_usdc();

   UniswapV3Pool::new(
      1,
      Address::from([0xaa; 20]),
      500,
      weth.to_erc20().into_owned(),
      usdc.to_erc20().into_owned(),
      DexKind::UniswapV3,
   )
}

/// One hop of a seeded quote: a whole WETH through the WETH/USDC pool and out as USDC.
fn weth_usdc_step(pool: &UniswapV3Pool) -> SwapStep<AnyUniswapPool> {
   let (weth, usdc) = weth_usdc();

   SwapStep::new(
      pool.clone().into(),
      NumericValue::parse_to_wei("1", 18),
      NumericValue::parse_to_wei("2500", 6),
      weth,
      usdc,
   )
}

/// Type a new slippage into the settings page, which is the only way the swap flow is handed one:
/// the raw text is sanitised there and written into the number the form enforces.
fn type_slippage(settings: &mut UniswapSettingsUi, ctx: &Context, theme: &Theme, text: &str) {
   settings.slippage.clear();

   let nodes = render_swap_settings(ctx, settings, theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      ctx,
      settings,
      theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      ctx,
      settings,
      theme,
      vec![Event::Text(text.to_owned())],
   );
}

/// The swap form on a real pair with both amounts typed, so the button and the details have
/// something to work from.
fn swap_form_on_weth_usdc(gui: &mut GUI) {
   let (weth, usdc) = weth_usdc();

   gui.uniswap.open();
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;
   gui.uniswap.swap_ui.amount_in_field.amount = "1".to_string();
   gui.uniswap.swap_ui.amount_in_field.amount_wei = U256::from(10).pow(U256::from(18));
   gui.uniswap.swap_ui.amount_out_field.amount = "2500".to_string();
   gui.uniswap.swap_ui.amount_out_field.amount_wei = U256::from(2_500_000_000u64);
}

/// The swap knobs read as knobs: the slippage field carries its own name and the tip the drawn word
/// held, and each slider says what its number counts instead of leaving a bare number beside it.
#[test]
fn the_swap_settings_name_their_controls() {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let mut settings = UniswapSettingsUi::new();
   let nodes = render_swap_settings(&ctx, &mut settings, &theme, Vec::new());

   let slippage = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the slippage field must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      slippage.description(),
      Some(SLIPPAGE_TIP),
      "the tip the drawn word held belongs to the field"
   );

   let deadline = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Slider && node.label() == Some("Deadline in minutes"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the deadline slider must be named: {:?}",
            drawn_names(&nodes)
         )
      });

   let description = deadline.description().unwrap_or_default();

   assert!(
      description.starts_with("Now 5 minutes"),
      "the deadline slider must say the number it counts: {description:?}"
   );

   assert!(
      description.contains(DEADLINE_TIP),
      "the tip the drawn word holds must reach the slider, not only a mouse: {description:?}"
   );

   for (name, value) in [("Max hops", "Now 2 hops"), ("Max routes", "Now 3 routes")] {
      let slider = nodes
         .iter()
         .find(|(_, node)| node.role() == Role::Slider && node.label() == Some(name))
         .map(|(_, node)| node)
         .unwrap_or_else(|| {
            panic!(
               "the {name} slider must be named: {:?}",
               drawn_names(&nodes)
            )
         });

      assert_eq!(
         slider.description(),
         Some(value),
         "the {name} slider must say the number it counts"
      );
   }

   // Each slider holds its number as a value with no words of its own, so the unit lives in the
   // description above — and the drawn captions and numbers are gone from the tree as items of their
   // own, which is what kept a bare "5" being read with nothing saying what it counts.
   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   for text in [
      "Slippage",
      "Deadline (minutes)",
      "Max Hops",
      "Max Routes",
      "5",
      "2",
      "3",
   ] {
      assert!(
         !spoken.contains(&text),
         "the drawn {text:?} must leave the tree, the control says it: {spoken:?}"
      );
   }
}

/// The sanitiser rewrites what was typed, and the correction is said: "-1" becomes "0" and the page
/// says so, while a value the field keeps as typed says nothing.
#[test]
fn a_corrected_slippage_is_said() {
   let ctx = Context::default();
   let mut theme = Theme::new(ThemeKind::TokyoNight);
   theme.install(&ctx);

   let mut settings = UniswapSettingsUi::new();
   settings.slippage.clear();

   let nodes = render_swap_settings(&ctx, &mut settings, &theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      &ctx,
      &mut settings,
      &theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      &ctx,
      &mut settings,
      &theme,
      vec![Event::Text("-1".to_owned())],
   );

   assert_eq!(
      settings.take_notice().as_deref(),
      Some("Slippage set to 0 percent."),
      "a field that rewrites what was typed must say what it holds now"
   );

   let mut kept = UniswapSettingsUi::new();
   kept.slippage.clear();

   let nodes = render_swap_settings(&ctx, &mut kept, &theme, Vec::new());
   let field = nodes
      .iter()
      .find(|(_, node)| is_text_input(node.role()) && node.label() == Some("Slippage percent"))
      .map(|(id, _)| *id)
      .expect("the slippage field must be in the tree before it is typed into");

   render_swap_settings(
      &ctx,
      &mut kept,
      &theme,
      vec![accesskit_action(Action::Focus, field)],
   );
   render_swap_settings(
      &ctx,
      &mut kept,
      &theme,
      vec![Event::Text("1".to_owned())],
   );

   assert_eq!(
      kept.take_notice(),
      None,
      "a value the field keeps as typed is not a correction"
   );
}

/// The version picker is a combobox whose trigger draws the version alone, so the caption has to
/// reach its node — and the fee tiers are buttons that draw a number and show their state with a
/// fill no reader can see.
#[test]
fn the_swap_form_says_which_pool_version_is_in_force() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   let (weth, usdc) = weth_usdc();

   zeus.write(|ctx| ctx.pool_manager.add_pool(pool.clone()));

   // The version picker and the fee tiers belong to the simulate page, which is the page that has to
   // be told which pool it is simulating.
   gui.uniswap.open();
   gui.uniswap.settings.simulate_mode = true;
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;
   gui.uniswap.swap_ui.pool = Some(pool.clone().into());

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Pool version: V3")),
      "the version picker must say which version it shows: {:?}",
      drawn_names(&nodes)
   );

   let tier = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Fee tier 0.05%"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the fee tier button must be named after its tier: {:?}",
            drawn_names(&nodes)
         )
      });

   assert_eq!(
      tier.toggled(),
      Some(Toggled::True),
      "the tier the form is on must say it is the chosen one"
   );
}

/// The reason a swap is blocked used to be the disabled button's label, which no reader can reach.
/// It is a line of its own now, said once when it changes, and the button keeps its name.
#[test]
fn the_swap_button_keeps_its_name_and_says_why_it_is_blocked() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let (weth, usdc) = weth_usdc();
   gui.uniswap.open();
   gui.uniswap.swap_ui.currency_in = weth;
   gui.uniswap.swap_ui.currency_out = usdc;

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let swap = nodes
      .iter()
      .find(|(_, node)| node.role() == Role::Button && node.label() == Some("Swap"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the action button must be named for what it does: {:?}",
            drawn_names(&nodes)
         )
      });

   assert!(
      swap.is_disabled(),
      "an empty amount leaves nothing to send, which is the state this test is about"
   );

   let spoken: Vec<&str> = nodes
      .iter()
      .filter(|(_, node)| node.role() == Role::Label && !node.is_hidden())
      .filter_map(|(_, node)| node.value())
      .collect();

   assert!(
      spoken.contains(&"Enter Amount"),
      "the reason must be a node of its own, reachable while the button is not: {spoken:?}"
   );

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Enter Amount"),
      "the reason is said once when it appears"
   );

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice(),
      None,
      "the same reason a frame later is not said again"
   );

   // More than the wallet holds. The balance was the last check the old label made, so it wins.
   gui.uniswap.swap_ui.amount_in_field.amount = "1".to_string();
   gui.uniswap.swap_ui.amount_in_field.amount_wei = U256::from(10).pow(U256::from(18));

   // The reason is recomputed the way a frame recomputes it: the sentence is the recompute's, not
   // the draw's, so a form that changed between two draws still says what now blocks it.
   zeus.write(|ctx| gui.uniswap.swap_ui.refresh_blocked_for_test(ctx));

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Swap")
         && node.is_disabled()),
      "the button keeps its name whatever is blocking it: {:?}",
      drawn_names(&nodes)
   );

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Insufficient WETH Balance"),
      "a reason that changes is a new sentence"
   );
}

/// The details pane drew its labels as words and its severity as colour: the rows say what they
/// count now, and the slippage says how bad it is in words too.
#[test]
fn the_swap_details_say_the_slippage_the_floor_and_the_impact() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   // Half a percent: low enough that the value is plain, and the number the form was handed.
   let theme = gui.theme.clone();
   type_slippage(&mut gui.uniswap.settings, &ctx, &theme, "0.5");

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);
   let min_received = NumericValue::format_wei(U256::from(2_498_750_000u64), 6);

   zeus.write(|ctx| {
      gui.uniswap
         .swap_ui
         .seed_quote_for_test(ctx, &settings, vec![step], min_received, 0.42);
   });

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let spoken = drawn_names(&nodes);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Slippage 0.50%")),
      "the slippage value must say what it counts: {spoken:?}"
   );

   let floor = nodes
      .iter()
      .find(|(_, node)| node.label().is_some_and(|label| label.starts_with("Minimum Received ")))
      .map(|(_, node)| node)
      .unwrap_or_else(|| panic!("the floor row must name its amount: {spoken:?}"));

   assert!(
      floor.label().is_some_and(|label| label.ends_with("USDC")),
      "the floor must be named in the currency being bought: {:?}",
      floor.label()
   );

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Price impact 0.42%")),
      "the impact must say what it counts: {spoken:?}"
   );

   // The words the rows drew are on their values now, not in the tree a second time.
   for word in ["Slippage", "Minimum Received", "Price Impact"] {
      let copies: Vec<(bool, Option<String>)> = nodes
         .iter()
         .filter(|(_, node)| node.role() == Role::Label && node.value() == Some(word))
         .map(|(_, node)| (node.is_hidden(), node.label().map(str::to_string)))
         .collect();

      assert!(
         !copies.iter().any(|(hidden, _)| !hidden),
         "the drawn {word:?} must leave the tree, its value says it: {copies:?}"
      );
   }

   // The severity is colour only, so the value has to carry it: at 2.5 percent the colour is the
   // error one and the words say "risky".
   type_slippage(&mut gui.uniswap.settings, &ctx, &theme, "2.5");

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.label() == Some("Slippage 2.50% (risky)")),
      "the severity the colour carries must be words: {:?}",
      drawn_names(&nodes)
   );
}

/// The route was drawn in a popup only a mouse could open. The count goes on the row's name now and
/// the hops on its description, so a reader gets the trade without hovering.
#[test]
fn the_route_is_spoken_without_hovering() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);

   let hop = format!(
      "{} {} to {} {} in the {}/{} pool, fee {} percent",
      step.amount_in.abbreviated(),
      step.currency_in.symbol(),
      step.amount_out.abbreviated(),
      step.currency_out.symbol(),
      step.pool.currency0().symbol(),
      step.pool.currency1().symbol(),
      step.pool.fee().fee_percent(),
   );

   zeus.write(|ctx| {
      gui.uniswap.swap_ui.seed_quote_for_test(
         ctx,
         &settings,
         vec![step],
         NumericValue::default(),
         0.0,
      );
   });

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   let routing = nodes
      .iter()
      .find(|(_, node)| node.label() == Some("Routing, 1 swap"))
      .map(|(_, node)| node)
      .unwrap_or_else(|| {
         panic!(
            "the routing row must say how many swaps it routes: {:?}",
            drawn_names(&nodes)
         )
      });

   // One hop is one swap: the name has to agree with the number it reads out.
   assert!(
      !nodes.iter().any(|(_, node)| node.label() == Some("Routing, 1 swaps")),
      "a single hop must not be named with the plural"
   );

   let description = routing.description().unwrap_or_default();

   assert!(
      description.starts_with("Total swaps: 1."),
      "the description must count the hops: {description:?}"
   );

   assert!(
      description.contains(&hop),
      "the description must spell the hop out: {description:?}"
   );

   // The popup's lines are not items of their own, and the bare word the row used to say is gone
   // with them: the reader gets the route once, from the row.
   for (_, node) in &nodes {
      if node.role() != Role::Label {
         continue;
      }

      let value = node.value().unwrap_or_default();

      assert!(
         !value.contains("->") && !value.contains("Total swaps"),
         "the hover popup's lines must not be bare labels: {value:?}"
      );

      assert!(
         value != "Routing",
         "the row says the count instead of the bare word"
      );
   }
}

/// A quote that lands is news the form had none of before, and it is said once — not on every
/// refresh of the same quote.
#[test]
fn a_quote_arriving_is_said_once() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);

   let pool = weth_usdc_pool();
   swap_form_on_weth_usdc(&mut gui);

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Insufficient WETH Balance"),
      "a form with no quote blocks the button, which is not the quote arriving"
   );

   let settings = gui.uniswap.settings.clone();
   let step = weth_usdc_step(&pool);

   zeus.write(|ctx| {
      gui.uniswap.swap_ui.seed_quote_for_test(
         ctx,
         &settings,
         vec![step],
         NumericValue::default(),
         0.0,
      );
   });

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice().as_deref(),
      Some("Quote ready: 2500 USDC."),
      "the quote is said with the amount the Buy field holds"
   );

   render_uniswap_window(&ctx, &mut gui, &zeus);

   assert_eq!(
      gui.uniswap.take_notice(),
      None,
      "the same quote a frame later is not said again"
   );
}

/// The button that exchanges the two currencies is a picture of two arrows, so it has no words of
/// its own to read.
#[test]
fn the_direction_button_says_what_it_swaps() {
   let (ctx, mut gui, zeus) = main_window();
   wallet_in_cache(&zeus);
   gui.uniswap.open();

   let nodes = render_uniswap_window(&ctx, &mut gui, &zeus);

   assert!(
      nodes.iter().any(|(_, node)| node.role() == Role::Button
         && node.label() == Some("Swap Sell and Buy tokens")),
      "an image-only button must say what it exchanges: {:?}",
      drawn_names(&nodes)
   );
}
