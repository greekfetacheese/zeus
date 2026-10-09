pub mod icons;
pub use icons::Icons;

pub const INTER_BOLD_18: &[u8] = include_bytes!("./Inter_18pt-Bold.ttf");

/// The Zeus mark for the system tray: the 128px export from the brand kit.
pub const ZEUS_TRAY: &[u8] = include_bytes!("./brand/zeus-tray-128.png");

/// The Zeus mark for the window/taskbar icon.
///
/// winit takes a single image and scales it for every surface it is asked to fill, so this
/// is the largest export from the brand kit (256px) rather than the tray's 128px asset.
pub const ZEUS_ICON: &[u8] = include_bytes!("./brand/zeus-tray-256.png");

/// The mark at every size the brand kit exports, for the icon theme's
/// `hicolor/<size>x<size>/apps/` directories.
///
/// The kit renders each size rather than downscaling a master, so these are installed
/// verbatim — a scaled 16px is visibly muddier than the rendered one.
pub const ZEUS_ICON_SIZES: &[(u32, &[u8])] = &[
   (16, include_bytes!("./brand/zeus-tray-16.png")),
   (24, include_bytes!("./brand/zeus-tray-24.png")),
   (32, include_bytes!("./brand/zeus-tray-32.png")),
   (48, include_bytes!("./brand/zeus-tray-48.png")),
   (64, include_bytes!("./brand/zeus-tray-64.png")),
   (128, include_bytes!("./brand/zeus-tray-128.png")),
   (256, include_bytes!("./brand/zeus-tray-256.png")),
];

/// Decode a PNG mark to the raw RGBA that both `egui::IconData` and `tray_icon::Icon` take.
pub fn decode_mark(png: &[u8]) -> Result<(Vec<u8>, u32, u32), image::ImageError> {
   let image = image::load_from_memory(png)?.into_rgba8();
   let (width, height) = image.dimensions();
   Ok((image.into_raw(), width, height))
}
