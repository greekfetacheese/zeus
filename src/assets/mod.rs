pub mod icons;
pub use icons::Icons;

pub const INTER_BOLD_18: &[u8] = include_bytes!("./Inter_18pt-Bold.ttf");

/// The system-tray icon: the Zeus wallet mark, decoded to raw RGBA on startup.
pub const TRAY_ICON_PNG: &[u8] = include_bytes!("./icons/misc/wallet-main.png");
