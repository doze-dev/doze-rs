//! The look: five palettes, each dark and light, from the Go app's design tokens (`themes/tokens.css`, converted by
//! `cargo xtask themes` into `themes/doze.json`). gpui-component's own theme is set from the same file, so its inputs,
//! menus and dialogs wear doze's colors, and doze's own painting reads [`Tokens`] by the names the React app used.
//!
//! The mode follows the system unless the user picked one; [`Choice`] is what the user picked.

mod motion;

pub(crate) use motion::{Density, Motion};

use std::rc::Rc;

use anyhow::{Context as _, anyhow};
use gpui::{App, Global, Hsla, Rgba, WindowAppearance};
use gpui_component::{Theme, ThemeConfig, try_parse_color};
use serde::Deserialize;

/// The theme set `cargo xtask themes` writes.
const THEMES: &str = include_str!("../../themes/doze.json");

/// A color family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Palette {
    /// The default.
    #[default]
    Violet,
    Teal,
    Burgundy,
    Amber,
    Slate,
}

impl Palette {
    /// Every palette, in the Settings order.
    pub(crate) const ALL: [Palette; 5] = [
        Palette::Violet,
        Palette::Teal,
        Palette::Burgundy,
        Palette::Amber,
        Palette::Slate,
    ];

    /// Its name in the theme file and in prefs.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Palette::Violet => "violet",
            Palette::Teal => "teal",
            Palette::Burgundy => "burgundy",
            Palette::Amber => "amber",
            Palette::Slate => "slate",
        }
    }

    /// The palette called `name`.
    pub(crate) fn parse(name: &str) -> Option<Palette> {
        Palette::ALL.into_iter().find(|p| p.name() == name)
    }
}

/// Dark, light, or whatever the system is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Mode {
    /// Follow the system, live.
    #[default]
    System,
    Dark,
    Light,
}

impl Mode {
    /// `dark`, `light` or `system`.
    pub(crate) fn parse(word: &str) -> Option<Mode> {
        match word {
            "dark" => Some(Mode::Dark),
            "light" => Some(Mode::Light),
            "system" | "" => Some(Mode::System),
            _ => None,
        }
    }

    /// Whether this mode is dark under the system's appearance.
    pub(crate) fn dark(self, system: WindowAppearance) -> bool {
        match self {
            Mode::Dark => true,
            Mode::Light => false,
            Mode::System => matches!(
                system,
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            ),
        }
    }
}

/// What the user picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Choice {
    pub(crate) palette: Palette,
    pub(crate) mode: Mode,
}

impl Global for Choice {}

/// doze's tokens, by the names of tokens.css.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Tokens {
    pub(crate) bg: Hsla,
    pub(crate) side: Hsla,
    pub(crate) panel: Hsla,
    pub(crate) line: Hsla,
    pub(crate) line2: Hsla,
    pub(crate) well: Hsla,
    pub(crate) text: Hsla,
    pub(crate) dim: Hsla,
    pub(crate) bright: Hsla,
    pub(crate) accent: Hsla,
    pub(crate) good: Hsla,
    pub(crate) warn: Hsla,
    pub(crate) cool: Hsla,
    pub(crate) bad: Hsla,
    pub(crate) extra: Hsla,
    pub(crate) sel: Hsla,
    pub(crate) pick: Hsla,
    pub(crate) track: Hsla,
    pub(crate) ink: Hsla,
    pub(crate) on_accent: Hsla,
    /// The theme is dark.
    pub(crate) dark: bool,
}

impl Global for Tokens {}

#[derive(Deserialize)]
struct Set {
    themes: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct Raw {
    bg: String,
    side: String,
    panel: String,
    line: String,
    line2: String,
    well: String,
    text: String,
    dim: String,
    bright: String,
    accent: String,
    good: String,
    warn: String,
    cool: String,
    bad: String,
    extra: String,
    sel: String,
    pick: String,
    track: String,
    ink: String,
    on_accent: String,
}

fn color(s: &str) -> anyhow::Result<Hsla> {
    try_parse_color(s).with_context(|| format!("color {s}"))
}

impl Tokens {
    fn from_raw(r: &Raw, dark: bool) -> anyhow::Result<Tokens> {
        Ok(Tokens {
            bg: color(&r.bg)?,
            side: color(&r.side)?,
            panel: color(&r.panel)?,
            line: color(&r.line)?,
            line2: color(&r.line2)?,
            well: color(&r.well)?,
            text: color(&r.text)?,
            dim: color(&r.dim)?,
            bright: color(&r.bright)?,
            accent: color(&r.accent)?,
            good: color(&r.good)?,
            warn: color(&r.warn)?,
            cool: color(&r.cool)?,
            bad: color(&r.bad)?,
            extra: color(&r.extra)?,
            sel: color(&r.sel)?,
            pick: color(&r.pick)?,
            track: color(&r.track)?,
            ink: color(&r.ink)?,
            on_accent: color(&r.on_accent)?,
            dark,
        })
    }

    /// The scrim under an overlay: the palette's darkest ink, a third opaque in the dark (`.scrim.top`), lighter in
    /// the light.
    pub(crate) fn scrim(&self) -> Hsla {
        if self.dark {
            alpha(mix(self.well, gpui::black(), 50.), 0.42)
        } else {
            alpha(self.bright, 0.28)
        }
    }

    /// A tone of the words as a color.
    pub(crate) fn tone(&self, tone: doze_display::Tone) -> Hsla {
        use doze_display::Tone;
        match tone {
            Tone::Good => self.good,
            Tone::Warn => self.warn,
            Tone::Bad => self.bad,
            Tone::Cool => self.cool,
            _ => self.dim,
        }
    }
}

/// One theme of the file: gpui-component's config and doze's tokens.
fn load(palette: Palette, dark: bool) -> anyhow::Result<(ThemeConfig, Tokens)> {
    let set: Set = serde_json::from_str(THEMES).context("themes/doze.json")?;
    let name = format!(
        "doze {} {}",
        palette.name(),
        if dark { "dark" } else { "light" }
    );
    let v = set
        .themes
        .into_iter()
        .find(|t| t.get("name").and_then(|n| n.as_str()) == Some(name.as_str()))
        .ok_or_else(|| anyhow!("no theme {name}"))?;
    let raw: Raw = serde_json::from_value(v.get("doze").cloned().unwrap_or_default())
        .context("doze tokens")?;
    let tokens = Tokens::from_raw(&raw, dark)?;
    let config: ThemeConfig = serde_json::from_value(v).context("component theme")?;
    Ok((config, tokens))
}

/// Puts the palette on: doze's tokens and gpui-component's theme, both modes registered so a system switch lands on
/// doze's other mode rather than the component default.
pub(crate) fn apply(cx: &mut App, choice: Choice, system: WindowAppearance) -> anyhow::Result<()> {
    let dark = choice.mode.dark(system);
    let (other, _) = load(choice.palette, !dark)?;
    let (config, tokens) = load(choice.palette, dark)?;
    cx.set_global(choice);
    cx.set_global(tokens);
    let (other, config) = (Rc::new(other), Rc::new(config));
    Theme::update(cx, |t| {
        t.apply_config(&other);
        t.apply_config(&config);
    });
    Ok(())
}

/// A palette's tokens in a mode, for Settings' swatches; read once from the theme file.
pub(crate) fn swatch(palette: Palette, dark: bool) -> Option<Tokens> {
    static ALL: std::sync::OnceLock<Vec<(Palette, bool, Tokens)>> = std::sync::OnceLock::new();
    let all = ALL.get_or_init(|| {
        Palette::ALL
            .into_iter()
            .flat_map(|p| [true, false].map(move |d| (p, d)))
            .filter_map(|(p, d)| load(p, d).ok().map(|(_, t)| (p, d, t)))
            .collect()
    });
    all.iter()
        .find(|(p, d, _)| *p == palette && *d == dark)
        .map(|(_, _, t)| *t)
}

/// The tokens in force.
pub(crate) fn tokens(cx: &App) -> Tokens {
    *cx.global::<Tokens>()
}

/// CSS `color-mix(in srgb, a pct%, b)`.
pub(crate) fn mix(a: Hsla, b: Hsla, pct: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let p = pct / 100.0;
    Rgba {
        r: a.r * p + b.r * (1.0 - p),
        g: a.g * p + b.g * (1.0 - p),
        b: a.b * p + b.b * (1.0 - p),
        a: 1.0,
    }
    .into()
}

/// The color at opacity `a`.
pub(crate) fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

#[cfg(test)]
mod tests {
    use super::{Palette, load};

    #[test]
    fn every_palette_loads_in_both_modes() {
        for p in Palette::ALL {
            for dark in [true, false] {
                let (config, t) = load(p, dark).unwrap();
                assert_eq!(config.mode.is_dark(), dark, "{}", p.name());
                assert!(t.dark == dark);
                // Text reads on the background: well apart in lightness.
                assert!((t.text.l - t.bg.l).abs() > 0.4, "{} {dark}", p.name());
            }
        }
    }
}
