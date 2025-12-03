# zeus-theme

# Theme color styling for egui

Currently there are 2 themes to use, `Dark` and `Light`.
The `Light` theme is still a work in progress so it is not recommended to use it.

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