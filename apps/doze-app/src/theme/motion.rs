//! Motion and density. Motion is short and gentle (the owner's bar: 120–280 ms, opacity and a few pixels of travel),
//! from gpui-component's motion tokens, and none at all when the system asks to reduce motion: a reduced overlay
//! simply appears, so no animation frame is ever scheduled for it.

use std::time::Duration;

use gpui::App;
use gpui_component::Theme;

/// How long things move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Motion {
    /// A hover, a chip: 120 ms.
    pub(crate) fast: Duration,
    /// An overlay entering, a card lighting: 180 ms.
    pub(crate) normal: Duration,
    /// A camera move: 280 ms.
    pub(crate) slow: Duration,
    /// How far an overlay travels as it enters, in pixels.
    pub(crate) travel: f32,
}

impl Motion {
    /// The motion in force: the theme's tokens, or none when the system reduces motion.
    pub(crate) fn of(cx: &App) -> Motion {
        if cx.reduce_motion() {
            return Motion::none();
        }
        let m = &Theme::global(cx).motion;
        Motion {
            fast: m.duration_fast,
            normal: m.duration_normal,
            slow: m.duration_slow,
            travel: 6.0,
        }
    }

    /// Nothing moves.
    pub(crate) fn none() -> Motion {
        Motion {
            fast: Duration::ZERO,
            normal: Duration::ZERO,
            slow: Duration::ZERO,
            travel: 0.0,
        }
    }

    /// Whether anything moves.
    pub(crate) fn moves(&self) -> bool {
        !self.normal.is_zero()
    }
}

/// How much room rows get (the List view's Compact / Cosy / Roomy).
#[allow(dead_code)] // WHY: the token is the theme's (step 0.7); the List view (M1 step 1.2) is its first reader
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Density {
    Compact,
    #[default]
    Cosy,
    Roomy,
}

#[allow(dead_code)] // WHY: as above
impl Density {
    /// A row's height in pixels.
    pub(crate) fn row(self) -> f32 {
        match self {
            Density::Compact => 36.0,
            Density::Cosy => 48.0,
            Density::Roomy => 60.0,
        }
    }

    /// `compact`, `cosy`, `roomy`.
    pub(crate) fn parse(word: &str) -> Option<Density> {
        match word {
            "compact" => Some(Density::Compact),
            "cosy" | "cozy" => Some(Density::Cosy),
            "roomy" => Some(Density::Roomy),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Density, Motion};

    #[test]
    fn reduced_motion_moves_nothing() {
        assert!(!Motion::none().moves());
        assert!(Density::Compact.row() < Density::Cosy.row());
        assert_eq!(Density::parse("roomy"), Some(Density::Roomy));
    }
}
