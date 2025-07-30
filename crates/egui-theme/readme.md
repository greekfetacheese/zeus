# egui-Theme

# Theme selection & customization for egui

Currently there are 4 themes to select:

- [Frappe](https://catppuccin.com)
- [Latte](https://catppuccin.com)
- [Tokyo Night](https://github.com/tokyo-night)
- [Nord](https://www.nordtheme.com/)

The theme coloring still needs work, Nord looks the best so far.

# Usage:

``` rust
use egui::Context;
use egui_theme::{Theme, ThemeKind};

let theme = Theme::new(ThemeKind::Nord);
egui_ctx.set_style(theme.style.clone());

```

# Feature Flags

`serde` enables serialization.