//! A dialog being answered (Go `components/dialog.tsx`): which button is highlighted, what was typed, and what each
//! key does. ⏎ presses the highlighted button when it is enabled; Esc cancels; ←→ move between the buttons, round;
//! `y` and `n` answer a restart. A reset waits for its name, and typing it moves the highlight from Cancel onto the
//! reset (and back, if the name goes wrong again); an action with input waits for a line.

use super::dialog::{Button, Dialog};

/// A dialog and its answer so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choosing {
    pub dialog: Dialog,
    /// The highlighted button.
    pub at: usize,
    /// What the box holds.
    pub typed: String,
}

/// What a key or a click did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    /// Nothing: the button is not enabled yet.
    Nothing,
    Cancel,
    /// Run this button, with the text typed (for an input).
    Run(Button, String),
}

impl Choosing {
    /// A dialog with its default button highlighted.
    pub fn new(dialog: Dialog) -> Choosing {
        let at = dialog.default.min(dialog.buttons.len().saturating_sub(1));
        Choosing {
            dialog,
            at,
            typed: String::new(),
        }
    }

    /// The name is typed, or a line is, as the dialog asks; or it asks for nothing.
    pub fn armed(&self) -> bool {
        let d = &self.dialog;
        if !d.type_name.is_empty() {
            self.typed == d.type_name
        } else if d.input {
            !self.typed.trim().is_empty()
        } else {
            true
        }
    }

    /// Whether button `i` can be pressed now.
    pub fn enabled(&self, i: usize) -> bool {
        self.dialog
            .buttons
            .get(i)
            .is_some_and(|b| b.cancel || self.armed() || (!b.danger && !self.dialog.input))
    }

    /// The box now holds `text`. Typing a reset's name moves the highlight onto the reset; a wrong name, back.
    pub fn type_text(&mut self, text: &str) {
        text.clone_into(&mut self.typed);
        if self.dialog.type_name.is_empty() {
            return;
        }
        self.at = if self.armed() {
            self.dialog
                .buttons
                .iter()
                .position(|b| !b.cancel)
                .unwrap_or(0)
        } else {
            self.dialog.default
        };
    }

    /// → (1) or ← (-1), round.
    pub fn step(&mut self, by: isize) {
        let n = self.dialog.buttons.len();
        if n == 0 {
            return;
        }
        self.at = (self.at + n).wrapping_add_signed(by) % n;
    }

    /// Highlights button `i` (the pointer is over it).
    pub fn hover(&mut self, i: usize) {
        if i < self.dialog.buttons.len() {
            self.at = i;
        }
    }

    /// Presses button `i`.
    pub fn press(&self, i: usize) -> Pressed {
        match self.dialog.buttons.get(i) {
            Some(b) if b.cancel => Pressed::Cancel,
            Some(b) if self.enabled(i) => {
                let text = if self.dialog.input {
                    self.typed.trim().to_owned()
                } else {
                    String::new()
                };
                Pressed::Run(b.clone(), text)
            }
            _ => Pressed::Nothing,
        }
    }

    /// ⏎: the highlighted button.
    pub fn enter(&self) -> Pressed {
        self.press(self.at)
    }

    /// `y`: a restart's first choice; nothing elsewhere.
    pub fn yes(&self) -> Pressed {
        if self.dialog.kind != "restart" || !self.dialog.type_name.is_empty() {
            return Pressed::Nothing;
        }
        self.dialog
            .buttons
            .iter()
            .position(|b| !b.cancel)
            .map_or(Pressed::Nothing, |i| self.press(i))
    }

    /// `n`: a restart's Cancel; nothing elsewhere.
    pub fn no(&self) -> Pressed {
        if self.dialog.kind == "restart" {
            Pressed::Cancel
        } else {
            Pressed::Nothing
        }
    }

    /// The dialog has a box to type in: a reset's name, or an action's input.
    pub fn has_box(&self) -> bool {
        self.dialog.input || !self.dialog.type_name.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::super::dialog::{Button, Dialog};
    use super::{Choosing, Pressed};

    fn reset() -> Dialog {
        Dialog {
            kind: "reset".into(),
            type_name: "db".into(),
            buttons: vec![
                Button {
                    danger: true,
                    ..Button::run("Reset db", "reset", &["db"])
                },
                Button::cancel(),
            ],
            default: 1,
            ..Dialog::default()
        }
    }

    #[test]
    fn a_reset_waits_for_the_name_and_the_default_moves_onto_it() {
        let mut c = Choosing::new(reset());
        assert_eq!(c.at, 1, "Cancel is the default");
        assert_eq!(c.enter(), Pressed::Cancel);
        assert!(!c.enabled(0));
        assert_eq!(c.press(0), Pressed::Nothing);
        c.type_text("d");
        assert_eq!(c.at, 1);
        c.type_text("db");
        assert_eq!(c.at, 0, "typing the name moves the default onto Reset");
        assert!(matches!(c.enter(), Pressed::Run(b, _) if b.verb == "reset"));
        c.type_text("dbx");
        assert_eq!(c.at, 1, "a wrong name puts it back on Cancel");
    }

    #[test]
    fn arrows_go_round_and_y_n_answer_a_restart() {
        let d = Dialog {
            kind: "restart".into(),
            buttons: vec![
                Button::run("Restart db", "restart", &["db"]).primary(true),
                Button::cancel(),
            ],
            ..Dialog::default()
        };
        let mut c = Choosing::new(d);
        c.step(1);
        assert_eq!(c.at, 1);
        c.step(1);
        assert_eq!(c.at, 0);
        c.step(-1);
        assert_eq!(c.at, 1);
        assert!(matches!(c.yes(), Pressed::Run(b, _) if b.label == "Restart db"));
        assert_eq!(c.no(), Pressed::Cancel);
        let mut sleep = c.clone();
        "sleep".clone_into(&mut sleep.dialog.kind);
        assert_eq!(sleep.yes(), Pressed::Nothing, "y answers a restart only");
    }

    #[test]
    fn an_input_waits_for_a_line_and_hands_it_over_trimmed() {
        let d = Dialog {
            kind: "action".into(),
            input: true,
            buttons: vec![
                Button::run("Do it", "produce", &["orders"]).primary(true),
                Button::cancel(),
            ],
            ..Dialog::default()
        };
        let mut c = Choosing::new(d);
        assert_eq!(c.enter(), Pressed::Nothing);
        c.type_text("   ");
        assert_eq!(c.enter(), Pressed::Nothing);
        c.type_text(" hello ");
        assert_eq!(
            c.enter(),
            Pressed::Run(
                Button::run("Do it", "produce", &["orders"]).primary(true),
                "hello".into()
            )
        );
        assert!(c.has_box());
    }
}
