//! The look: Graphite, one look in dark and light, from doze's own design tokens (`themes/tokens.css`, converted by
//! `cargo xtask themes` into `themes/doze.json`). gpui-component's own theme is set from the same file, so its inputs,
//! menus and dialogs wear doze's colors, and doze's own painting reads [`Tokens`] by the names the React app used.
//!
//! The Go app had five palettes; the owner dropped them on 2026-11-19 (docs/port/app/PLAN.md, the theme rows of the
//! parity checklist). A saved palette name is read and ignored: every one of them is the one look now.
//!
//! The mode follows the system unless the user picked one; [`Choice`] is what the user picked.

mod motion;
pub(crate) mod ty;

pub(crate) use motion::{Density, Motion};

use std::rc::Rc;

use anyhow::{Context as _, anyhow};
use gpui::{App, Global, Hsla, Rgba, WindowAppearance};
use gpui_component::{Theme, ThemeConfig, try_parse_color};
use serde::Deserialize;

/// The theme set `cargo xtask themes` writes.
const THEMES: &str = include_str!("../../themes/doze.json");

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
    pub(crate) sel: Hsla,
    pub(crate) pick: Hsla,
    pub(crate) track: Hsla,
    pub(crate) ink: Hsla,
    pub(crate) on_accent: Hsla,
    /// A switch's fill and knob when on: neutral, so the accent is not "on".
    pub(crate) on: Hsla,
    pub(crate) on_knob: Hsla,
    /// The categorical ramp (teal, violet, magenta, stone): log service names, the memory strip, chart lines. No
    /// status hue is in it, so a category never reads as awake, dozing or broken.
    pub(crate) cat: [Hsla; 4],
    /// How strongly cards lift off the page with a soft shadow.
    pub(crate) lift: f32,
    /// The scrim's opacity under ⌘K and the bell.
    pub(crate) scrim_top: f32,
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
    sel: String,
    pick: String,
    track: String,
    ink: String,
    on_accent: String,
    on: String,
    on_knob: String,
    cat_1: String,
    cat_2: String,
    cat_3: String,
    cat_4: String,
    lift: f32,
    scrim: f32,
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
            sel: color(&r.sel)?,
            pick: color(&r.pick)?,
            track: color(&r.track)?,
            ink: color(&r.ink)?,
            on_accent: color(&r.on_accent)?,
            on: color(&r.on)?,
            on_knob: color(&r.on_knob)?,
            cat: [
                color(&r.cat_1)?,
                color(&r.cat_2)?,
                color(&r.cat_3)?,
                color(&r.cat_4)?,
            ],
            lift: r.lift,
            scrim_top: r.scrim,
            dark,
        })
    }

    /// The scrim under ⌘K and the bell: Go's ink, rgba(9, 8, 14), at the theme's `scrim` (Go's `.scrim.top` was .35;
    /// Graphite's is .5 dark, .3 light, so a popover parts from what is behind it).
    pub(crate) fn scrim(&self) -> Hsla {
        scrim(self.scrim_top)
    }

    /// The soft shadow an awake card casts: it lifts off the page.
    pub(crate) fn lift(&self) -> Vec<gpui::BoxShadow> {
        self.lift_at(1.0)
    }

    /// The card shadow at `k` of its strength, 0 flat to 1 lifted: a tile eases between them as it sleeps or wakes.
    pub(crate) fn lift_at(&self, k: f32) -> Vec<gpui::BoxShadow> {
        let a = self.lift * k.clamp(0.0, 1.0);
        if a <= 0.0 {
            return Vec::new();
        }
        let shade = |a: f32, y: f32, blur: f32| gpui::BoxShadow {
            color: alpha(gpui::black(), a),
            offset: gpui::point(gpui::px(0.), gpui::px(y)),
            blur_radius: gpui::px(blur),
            spread_radius: gpui::px(0.),
            inset: false,
        };
        vec![shade(a, 1.0, 2.0), shade(a * 0.7, 6.0, 18.0)]
    }

    /// Category `i` of the ramp, cycling.
    pub(crate) fn cat(&self, i: usize) -> Hsla {
        self.cat[i % self.cat.len()]
    }

    /// The scrim under a dialog: Go `.scrim`, rgba(9, 8, 14, .62), the same in every theme and mode.
    pub(crate) fn dialog_scrim(&self) -> Hsla {
        scrim(0.62)
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

/// The theme of a mode: gpui-component's config and doze's tokens.
pub(crate) fn load(dark: bool) -> anyhow::Result<(ThemeConfig, Tokens)> {
    let set: Set = serde_json::from_str(THEMES).context("themes/doze.json")?;
    let name = format!("doze {}", if dark { "dark" } else { "light" });
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

/// Puts the look on in the chosen mode: doze's tokens and gpui-component's theme, both modes registered so a system switch lands on
/// doze's other mode rather than the component default.
pub(crate) fn apply(cx: &mut App, choice: Choice, system: WindowAppearance) -> anyhow::Result<()> {
    let dark = choice.mode.dark(system);
    let (other, _) = load(!dark)?;
    let (config, tokens) = load(dark)?;
    cx.set_global(choice);
    cx.set_global(tokens);
    let (other, config) = (Rc::new(other), Rc::new(config));
    Theme::update(cx, |t| {
        t.apply_config(&other);
        t.apply_config(&config);
    });
    Ok(())
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

/// Go's scrim ink, rgba(9, 8, 14), at opacity `a`.
fn scrim(a: f32) -> Hsla {
    Rgba {
        r: 9.0 / 255.0,
        g: 8.0 / 255.0,
        b: 14.0 / 255.0,
        a,
    }
    .into()
}

/// The color at opacity `a`.
pub(crate) fn alpha(c: Hsla, a: f32) -> Hsla {
    Hsla { a, ..c }
}

#[cfg(test)]
mod tests {
    use super::load;

    #[test]
    fn the_scrims_are_gos_one_ink_in_both_modes() {
        for dark in [true, false] {
            let (_, t) = load(dark).unwrap();
            let (s, d) = (
                gpui::Rgba::from(t.scrim()),
                gpui::Rgba::from(t.dialog_scrim()),
            );
            assert_eq!(
                (
                    (s.r * 255.).round(),
                    (s.g * 255.).round(),
                    (s.b * 255.).round()
                ),
                (9., 8., 14.)
            );
            let top = if dark { 0.5 } else { 0.3 };
            assert!((s.a - top).abs() < 1e-3 && (d.a - 0.62).abs() < 1e-3);
            assert_eq!(((d.r * 255.).round(), (d.b * 255.).round()), (9., 14.));
        }
    }

    #[test]
    fn the_look_loads_in_both_modes() {
        for dark in [true, false] {
            let (config, t) = load(dark).unwrap();
            assert_eq!(config.mode.is_dark(), dark);
            assert!(t.dark == dark);
            // Text reads on the background: well apart in lightness.
            assert!((t.text.l - t.bg.l).abs() > 0.4, "{dark}");
            // Awake lifts, asleep lies flat, and the ease runs between them.
            assert_eq!(t.lift().len(), 2);
            assert!(t.lift_at(0.0).is_empty());
            assert!(t.lift_at(0.5)[0].color.a < t.lift()[0].color.a);
        }
    }

    /// The categorical ramp keeps clear of the status set: no category is within 60 RGB units of a status colour or
    /// the accent (the design pass's "hard to tell apart at dot size"), and each reads at 3:1 on every surface.
    #[test]
    fn the_ramp_is_not_a_status_colour() {
        fn rgb(c: gpui::Hsla) -> [f32; 3] {
            let c = gpui::Rgba::from(c);
            [c.r * 255., c.g * 255., c.b * 255.]
        }
        fn lum(c: gpui::Hsla) -> f32 {
            let l = |v: f32| {
                let v = v / 255.;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            let [r, g, b] = rgb(c);
            0.2126 * l(r) + 0.7152 * l(g) + 0.0722 * l(b)
        }
        for dark in [true, false] {
            let (_, t) = load(dark).unwrap();
            for c in t.cat {
                for s in [t.good, t.warn, t.cool, t.bad, t.accent] {
                    let (a, b) = (rgb(c), rgb(s));
                    let d = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2))
                        .sqrt();
                    assert!(d > 60., "{dark}: a category {d:.0} from a status colour");
                }
                for s in [t.bg, t.side, t.panel, t.well, t.sel] {
                    let (x, y) = (lum(c), lum(s));
                    let ratio = (x.max(y) + 0.05) / (x.min(y) + 0.05);
                    assert!(
                        ratio >= 3.0,
                        "{dark}: a category at {ratio:.2} on a surface"
                    );
                }
            }
        }
    }
}
