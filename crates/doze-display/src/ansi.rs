//! Escape sequences taken out of text, byte for byte as the Go surfaces take them out (charmbracelet's `ansi.Strip`
//! over its DEC ANSI parser table, vt100.net/emu/dec_ansi_parser with UTF-8 added). What is kept is what that parser
//! prints or executes; everything inside a sequence goes.

use std::sync::LazyLock;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum S {
    Ground,
    CsiEntry,
    CsiIntermediate,
    CsiParam,
    DcsEntry,
    DcsIntermediate,
    DcsParam,
    DcsString,
    Escape,
    EscapeIntermediate,
    OscString,
    SosString,
    PmString,
    ApcString,
    Utf8,
}

const STATES: [S; 15] = [
    S::Ground,
    S::CsiEntry,
    S::CsiIntermediate,
    S::CsiParam,
    S::DcsEntry,
    S::DcsIntermediate,
    S::DcsParam,
    S::DcsString,
    S::Escape,
    S::EscapeIntermediate,
    S::OscString,
    S::SosString,
    S::PmString,
    S::ApcString,
    S::Utf8,
];

/// What the parser does with a byte, as far as stripping cares: keep it, start a UTF-8 rune, or drop it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Act {
    Drop,
    Keep,
    Rune,
}

static TABLE: LazyLock<Table> = LazyLock::new(Table::build);

/// One cell: the next state and the action.
type Cell = (S, Act);

/// The transition table, built as the Go one is: a default, then rules, a later rule overwriting an earlier one. The
/// rules are grouped by state here; each group keeps the order Go applies them in.
struct Table([[Cell; 256]; 15]);

impl Table {
    fn set(&mut self, state: S, lo: u8, hi: u8, act: Act, next: S) {
        if let Some(row) = self.0.get_mut(state as usize) {
            for cell in row
                .iter_mut()
                .skip(usize::from(lo))
                .take(usize::from(hi - lo) + 1)
            {
                *cell = (next, act);
            }
        }
    }

    fn get(&self, state: S, byte: u8) -> Cell {
        self.0
            .get(state as usize)
            .and_then(|row| row.get(usize::from(byte)))
            .copied()
            .unwrap_or((S::Ground, Act::Drop))
    }

    fn build() -> Self {
        let mut t = Self([[(S::Ground, Act::Drop); 256]; 15]);
        for s in STATES {
            t.anywhere(s);
        }
        t.ground();
        t.escapes();
        t.strings();
        t.dcs();
        t.csi();
        t
    }

    /// Transitions from every state. (Go: "Anywhere".)
    fn anywhere(&mut self, s: S) {
        use Act::{Drop, Keep, Rune};
        for b in [0x18, 0x1a, 0x99, 0x9a] {
            self.set(s, b, b, Keep, S::Ground);
        }
        self.set(s, 0x80, 0x8F, Keep, S::Ground);
        self.set(s, 0x90, 0x97, Keep, S::Ground);
        self.set(s, 0x9C, 0x9C, Keep, S::Ground);
        self.set(s, 0x1B, 0x1B, Drop, S::Escape);
        self.set(s, 0x98, 0x98, Drop, S::SosString);
        self.set(s, 0x9E, 0x9E, Drop, S::PmString);
        self.set(s, 0x9F, 0x9F, Drop, S::ApcString);
        self.set(s, 0x9B, 0x9B, Drop, S::CsiEntry);
        self.set(s, 0x90, 0x90, Drop, S::DcsEntry);
        self.set(s, 0x9D, 0x9D, Drop, S::OscString);
        self.set(s, 0xC2, 0xF4, Rune, S::Utf8);
    }

    /// Executes and prints in the ground state, and in the escape states' C0 controls.
    fn ground(&mut self) {
        for s in [
            S::Ground,
            S::EscapeIntermediate,
            S::Escape,
            S::CsiParam,
            S::CsiIntermediate,
            S::CsiEntry,
        ] {
            self.set(s, 0x00, 0x17, Act::Keep, s);
            self.set(s, 0x19, 0x19, Act::Keep, s);
            self.set(s, 0x1C, 0x1F, Act::Keep, s);
            self.set(s, 0x7F, 0x7F, Act::Drop, s);
        }
        self.set(S::Ground, 0x20, 0x7E, Act::Keep, S::Ground);
        self.set(S::Ground, 0x7F, 0x7F, Act::Keep, S::Ground);
    }

    fn escapes(&mut self) {
        let (e, ei) = (S::Escape, S::EscapeIntermediate);
        self.set(ei, 0x20, 0x2F, Act::Drop, ei);
        self.set(ei, 0x30, 0x7E, Act::Drop, S::Ground);
        self.set(e, 0x30, 0x7E, Act::Drop, S::Ground);
        self.set(e, 0x20, 0x2F, Act::Drop, ei);
        // P, X, [, ], ^ and _ open a sequence rather than end one (Q and others in 0x50-0x5F that the Go table
        // does not name fall to the default: dropped, back to ground).
        self.set(e, 0x50, 0x5F, Act::Drop, S::Ground);
        for (b, next) in [
            (b'X', S::SosString),
            (b'^', S::PmString),
            (b'_', S::ApcString),
            (b'P', S::DcsEntry),
            (b'[', S::CsiEntry),
            (b']', S::OscString),
        ] {
            self.set(e, b, b, Act::Drop, next);
        }
    }

    /// SOS, PM, APC and OSC strings: everything inside them goes.
    fn strings(&mut self) {
        for s in [S::SosString, S::PmString, S::ApcString] {
            self.set(s, 0x00, 0x7F, Act::Drop, s);
            self.set(s, 0x18, 0x18, Act::Drop, S::Ground);
            self.set(s, 0x1A, 0x1A, Act::Drop, S::Ground);
            self.set(s, 0x1B, 0x1B, Act::Drop, S::Escape);
            self.set(s, 0x9C, 0x9C, Act::Drop, S::Ground);
        }
        let o = S::OscString;
        self.set(o, 0x00, 0xFF, Act::Drop, o);
        self.set(o, 0x07, 0x07, Act::Drop, S::Ground);
        self.set(o, 0x18, 0x18, Act::Drop, S::Ground);
        self.set(o, 0x1A, 0x1A, Act::Drop, S::Ground);
        self.set(o, 0x1B, 0x1B, Act::Drop, S::Escape);
        self.set(o, 0x9C, 0x9C, Act::Drop, S::Ground);
    }

    fn dcs(&mut self) {
        let (entry, inter, param, string) =
            (S::DcsEntry, S::DcsIntermediate, S::DcsParam, S::DcsString);
        for s in [entry, inter, param] {
            self.set(s, 0x00, 0x17, Act::Drop, s);
            self.set(s, 0x19, 0x19, Act::Drop, s);
            self.set(s, 0x1C, 0x1F, Act::Drop, s);
            self.set(s, 0x7F, 0x7F, Act::Drop, s);
            self.set(s, 0x40, 0x7E, Act::Drop, string);
        }
        self.set(entry, 0x20, 0x2F, Act::Drop, inter);
        self.set(entry, 0x30, 0x3F, Act::Drop, param);
        self.set(entry, 0x08, 0x0D, Act::Drop, string);
        self.set(entry, 0x1B, 0x1B, Act::Drop, string);
        self.set(inter, 0x20, 0x2F, Act::Drop, inter);
        self.set(inter, 0x30, 0x3F, Act::Drop, string);
        self.set(param, 0x30, 0x3F, Act::Drop, param);
        self.set(param, 0x20, 0x2F, Act::Drop, inter);
        self.set(string, 0x00, 0xFF, Act::Drop, string);
        self.set(string, 0x18, 0x18, Act::Drop, S::Ground);
        self.set(string, 0x1A, 0x1A, Act::Drop, S::Ground);
        self.set(string, 0x1B, 0x1B, Act::Drop, S::Escape);
        self.set(string, 0x9C, 0x9C, Act::Drop, S::Ground);
    }

    fn csi(&mut self) {
        let (entry, inter, param) = (S::CsiEntry, S::CsiIntermediate, S::CsiParam);
        for s in [entry, inter, param] {
            self.set(s, 0x40, 0x7E, Act::Drop, S::Ground);
            self.set(s, 0x20, 0x2F, Act::Drop, inter);
        }
        self.set(param, 0x30, 0x3F, Act::Drop, param);
        self.set(inter, 0x30, 0x3F, Act::Drop, S::Ground);
        self.set(entry, 0x30, 0x3F, Act::Drop, param);
    }
}

/// The length of the UTF-8 sequence a lead byte starts. (Only lead bytes 0xC2 to 0xF4 reach it.)
fn rune_len(lead: u8) -> usize {
    match lead {
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        _ => 4,
    }
}

/// `s` with its escape sequences taken out. The bytes of a rune the parser starts are kept whole, whatever they are,
/// as the Go parser keeps them; what is not valid UTF-8 is left for the caller to replace.
pub(crate) fn strip(s: &[u8]) -> Vec<u8> {
    let table = &*TABLE;
    let mut out = Vec::with_capacity(s.len());
    let (mut state, mut rune_left) = (S::Ground, 0_usize);
    for &b in s {
        if rune_left > 0 {
            out.push(b);
            rune_left -= 1;
            continue;
        }
        let (next, act) = table.get(state, b);
        match act {
            Act::Keep => out.push(b),
            Act::Rune => {
                out.push(b);
                rune_left = rune_len(b) - 1;
            }
            Act::Drop => {}
        }
        state = if next == S::Utf8 { S::Ground } else { next };
    }
    out
}
