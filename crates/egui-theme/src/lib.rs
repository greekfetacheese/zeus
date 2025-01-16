use egui::{ style::Style, Color32, Frame };

const PANIC_MSG: &str = "Custom theme not supported, use Theme::from_custom() instead";

pub mod editor;
pub mod utils;
pub mod themes;

pub use editor::ThemeEditor;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ThemeKind {
    Midnight,

    /// A custom theme
    Custom,
}

impl ThemeKind {
    pub fn to_str(&self) -> &str {
        match self {
            ThemeKind::Midnight => "Midnight",
            ThemeKind::Custom => "Custom",
        }
    }

    pub fn to_vec() -> Vec<Self> {
        vec![Self::Midnight]
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Theme {
    pub kind: ThemeKind,
    pub style: Style,
    pub colors: ThemeColors,
    pub frame1: Frame,
    pub frame2: Frame,
    pub frame1_visuals: FrameVisuals,
    pub frame2_visuals: FrameVisuals,
}

impl Theme {
    /// Panics if the kind is [ThemeKind::Custom]
    ///
    /// Use [Theme::from_custom()] instead
    pub fn new(kind: ThemeKind) -> Self {
        let theme = match kind {
            ThemeKind::Midnight => themes::midnight::theme(),
            ThemeKind::Custom => panic!("{}", PANIC_MSG),
        };

        theme
    }

    /// Load a custom theme from a json file
    ///
    /// We expect the [Theme] to be serialized as it is
    pub fn from_custom(path: std::path::PathBuf) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        let mut theme: Theme = serde_json::from_slice::<Theme>(&data)?;
        theme.kind = ThemeKind::Custom;

        Ok(theme)
    }

    /// Serialize the theme to a json string
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}

/// These colors can be used to override the visuals using [egui::Ui::visuals_mut]
///
/// `border_color` = [egui::Stroke] color
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ThemeColors {
    /// Background color for the entire app
    pub bg_color: Color32,

    /// Text color
    pub text_color: Color32,

    /// Widget background color for inactive widgets (no hover or clicks)
    ///
    /// It affects the following widgets: Button, ComboxBox, Slider
    pub widget_bg_color_idle: Color32,

    /// Background color for active widgets (click)
    ///
    /// It affects the following widgets: Button, ComboxBox, Slider
    pub widget_bg_color_click: Color32,

    /// Background color for hovered widgets
    ///
    /// It affects the following widgets: Button, ComboxBox, Slider
    pub widget_bg_color_hover: Color32,

    /// Background color for open widgets (eg. a combo box)
    ///
    /// It affects the following widgets: ComboxBox
    pub widget_bg_color_open: Color32,

    /// Border color for inactive widgets (no hover or clicks)
    ///
    /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
    pub border_color_idle: Color32,

    /// Border color for active widgets (click)
    ///
    /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
    pub border_color_click: Color32,

    /// Border color for hovered widgets
    ///
    /// It affects the following widgets: Button, ComboxBox, TextEdit, Slider, RadioButton
    pub border_color_hover: Color32,

    /// Border color for open widgets (eg. an opened combo box)
    ///
    /// It affects the following widgets: ComboxBox
    pub border_color_open: Color32,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FrameVisuals {
    pub bg_on_hover: Color32,
    pub bg_on_click: Color32,
    pub border_on_hover: (f32, Color32),
    pub border_on_click: (f32, Color32),
}