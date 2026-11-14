//! Embedded icons, fonts and the menu-bar pictures. The icons are the paths the React app draws inline (titlebar.tsx,
//! bell.tsx); GPUI paints an SVG as a mask in the text colour. The fonts are Geist and Geist Mono 1.9.2
//! (OFL-1.1), cut from the variable fonts into static weights and subset (assets/fonts/README.md): GPUI loads a
//! variable font from memory at its default instance only, so each weight the type table names is its own face.

use std::borrow::Cow;

use gpui::{AssetSource, SharedString};

const HOME: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 7.2 8 2.5l5.5 4.7"/><path d="M4 6.2v7h3v-3.6h2v3.6h3v-7"/></svg>"#;
const BELL: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/></svg>"#;
const SEARCH: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/></svg>"#;
const CLOSE: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="1.6" stroke-linecap="round"><path d="M4 4l8 8M12 4l-8 8"/></svg>"#;
const MINIMAP: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="1.4" stroke-linejoin="round"><rect x="1.5" y="2.5" width="13" height="11" rx="2"/><rect x="8" y="8" width="5" height="4" rx="1" fill="white"/></svg>"#;
const DOTS: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><circle cx="3.5" cy="8" r="1.4" fill="white"/><circle cx="8" cy="8" r="1.4" fill="white"/><circle cx="12.5" cy="8" r="1.4" fill="white"/></svg>"#;
/// The Grid tooltip's arrow.
const CARET: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 12 6"><path d="M0 6 6 0 12 6z" fill="white"/></svg>"#;
/// The copy glyph (`⧉` in the React page, drawn so it sits on the text's middle).
const COPY: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="1.4"><rect x="5.5" y="5.5" width="8" height="8" rx="1.5"/><path d="M10.5 3.5v-.5a1 1 0 0 0-1-1h-6a1 1 0 0 0-1 1v6a1 1 0 0 0 1 1h.5"/></svg>"#;
/// The pin (overview.tsx `PinGlyph`).
const PIN: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="M6 1.5h4l-.8 4.2L12 8.5V10H8.6V15H7.4V10H4V8.5l2.8-2.8z" fill="white"/></svg>"#;
/// A tick (the start sheet's boxes).
const CHECK: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="white" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round"><path d="M3.5 8.5 6.5 11.5 12.5 4.5"/></svg>"#;
/// The doze mark, whole (website/brand/doze.svg): the Library's head draws it in its own colours.
const BRAND: &str = include_str!("../assets/brand/doze.svg");

/// Three bars `gap` apart, as density.tsx draws them.
fn lines(gap: f32) -> String {
    let top = 8.0 - gap;
    let bars: String = (0..3)
        .map(|i| {
            format!(
                r#"<rect x="2" y="{}" width="12" height="1.5" rx="0.75" fill="white"/>"#,
                top + i as f32 * gap - 0.75
            )
        })
        .collect();
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">{bars}</svg>"#)
}

pub(crate) struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let svg: Cow<'static, str> = match path {
            "icons/close.svg" => CLOSE.into(),
            "icons/minimap.svg" => MINIMAP.into(),
            "icons/home.svg" => HOME.into(),
            "icons/bell.svg" => BELL.into(),
            "icons/search.svg" => SEARCH.into(),
            "icons/dots.svg" => DOTS.into(),
            "icons/lines-3.svg" => lines(3.0).into(),
            "icons/lines-4.5.svg" => lines(4.5).into(),
            "icons/lines-6.svg" => lines(6.0).into(),
            "icons/copy.svg" => COPY.into(),
            "icons/caret.svg" => CARET.into(),
            "icons/pin.svg" => PIN.into(),
            "icons/check.svg" => CHECK.into(),
            "brand/doze.svg" => BRAND.into(),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Owned(svg.into_owned().into_bytes())))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

/// The faces: family "Geist" at 400, 500, 600, 650, 700 and 750, and "Geist Mono" at 400 to 700 (ty.rs names them).
pub(crate) const FONTS: [&[u8]; 10] = [
    include_bytes!("../assets/fonts/Geist-400.ttf"),
    include_bytes!("../assets/fonts/Geist-500.ttf"),
    include_bytes!("../assets/fonts/Geist-600.ttf"),
    include_bytes!("../assets/fonts/Geist-650.ttf"),
    include_bytes!("../assets/fonts/Geist-700.ttf"),
    include_bytes!("../assets/fonts/Geist-750.ttf"),
    include_bytes!("../assets/fonts/GeistMono-400.ttf"),
    include_bytes!("../assets/fonts/GeistMono-500.ttf"),
    include_bytes!("../assets/fonts/GeistMono-600.ttf"),
    include_bytes!("../assets/fonts/GeistMono-700.ttf"),
];

/// The menu-bar item's template images (the Go app's bat), 44 × 44 for a 22 pt bar.
pub(crate) const TRAY: doze_os::ui::Icons = doze_os::ui::Icons {
    calm: include_bytes!("../assets/tray/bat-44.png"),
    alert: include_bytes!("../assets/tray/bat-alert-44.png"),
};
