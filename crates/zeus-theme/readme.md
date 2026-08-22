# zeus-theme

# This crate has been deprecated in favor of [egui-elements](https://github.com/greekfetacheese/egui-elements)

# Theme color styling for egui

Available themes:
- Dark
- Tokyo Night

## This crate is still being actively developed, there will be breaking changes either to some apis or to the theme specs.

# Usage:

``` rust
use egui::Context;
use zeus_theme::{Theme, ThemeKind};

let theme = Theme::new(ThemeKind::Dark);
egui_ctx.set_style(theme.style.clone());

```

# Feature Flags

`serde` enables serialization.