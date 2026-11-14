//! What a card's text needs: its width for the name and badge, and which chips fit. Widths are
//! the Go map's estimates per character (the board's fonts), not measured glyphs, so the layout
//! needs no font system.

/// A card's usual width.
pub(crate) const CARD_W: i64 = 214;
/// A card grows by this for a long name.
const WIDE_STEP: i64 = 36;
/// The fold mark (✓ or ▸) and the gap before it.
const FOLD_W: i64 = 12 + 5;

/// The language badge, read from the run command: what runs it says what it is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Lang {
    /// `go run …`
    Go,
    /// `uv`, `poetry`, `python3`, `uvicorn` …
    Py,
    /// `bun`, `deno`, or a node runner with a `.ts` file.
    Ts,
    /// `node`, `npm`, `pnpm`, `yarn`, `npx`.
    Js,
}

impl Lang {
    /// The badge's text: `go`, `py`, `ts`, `js`.
    #[must_use]
    pub fn badge(self) -> &'static str {
        match self {
            Lang::Go => "go",
            Lang::Py => "py",
            Lang::Ts => "ts",
            Lang::Js => "js",
        }
    }

    /// The language a run command is in, when the command says.
    #[must_use]
    pub fn of(run: &str) -> Option<Lang> {
        let first = run.split_whitespace().find(|f| !f.contains('='))?;
        let first = first.rsplit('/').next().unwrap_or(first);
        let ts = run.contains(".ts") || run.contains("tsx") || run.contains("ts-node");
        match first {
            "go" => Some(Lang::Go),
            "uv" | "poetry" | "pipenv" | "uvicorn" | "gunicorn" => Some(Lang::Py),
            f if f.starts_with("python") => Some(Lang::Py),
            "bun" | "deno" => Some(Lang::Ts),
            "npm" | "pnpm" | "node" | "yarn" | "npx" if ts => Some(Lang::Ts),
            "npm" | "pnpm" | "node" | "yarn" | "npx" => Some(Lang::Js),
            _ => None,
        }
    }
}

/// Text's width at `per` pixels a character.
pub(crate) fn text_w(s: &str, per: f64) -> i64 {
    #[allow(clippy::cast_precision_loss)]
    let w = s.chars().count() as f64 * per + 0.5;
    // Small and non-negative: the cast is exact.
    #[allow(clippy::cast_possible_truncation)]
    let w = w as i64;
    w
}

/// A chip's width at the board's 11.5px.
pub(crate) fn chip_w(s: &str) -> i64 {
    text_w(s, 6.4) + 14
}

/// How wide a card must be for its name and badge: 214, or wider in steps of 36.
pub(crate) fn card_width(name: &str, lang: Option<Lang>) -> i64 {
    // dot, gaps, memory, padding
    let mut need = text_w(name, 7.4) + 8 + 7 + 7 + 44 + 26;
    if lang.is_some() {
        need += 22 + 7;
    }
    let mut w = CARD_W;
    while w < need {
        w += WIDE_STEP;
    }
    w
}

/// The chips that fit on a card `width` wide, in order, and how many did not.
pub(crate) fn fit_chips<'a>(width: i64, fold: bool, uses: &[&'a str]) -> (Vec<&'a str>, u32) {
    let mut room = width - 26;
    if fold {
        room -= FOLD_W;
    }
    let plus = chip_w("+9") + 5;
    let mut chips = Vec::new();
    for (i, s) in uses.iter().enumerate() {
        let w = chip_w(s);
        // The last chip needs only its own room; one before it, room for a "+n" after it too.
        let fits = if i + 1 < uses.len() {
            w + plus <= room
        } else {
            w <= room
        };
        if !fits {
            let more = u32::try_from(uses.len() - i).unwrap_or(u32::MAX);
            return (chips, more);
        }
        chips.push(*s);
        room -= w + 5;
    }
    (chips, 0)
}

#[cfg(test)]
mod tests {
    use super::Lang;

    /// TestLang in flow_test.go: the badge for each kind of run command.
    #[test]
    fn lang_of_run_commands() {
        for (run, want) in [
            ("go run ./services/orders", Some("go")),
            ("uv run --frozen python -m fulfilment", Some("py")),
            ("python3 app.py", Some("py")),
            ("poetry run serve", Some("py")),
            ("bun src/index.ts", Some("ts")),
            ("bun run dev", Some("ts")),
            ("pnpm dev", Some("js")),
            ("npm run dev -- --config vite.config.ts", Some("ts")),
            ("node server.js", Some("js")),
            ("deno run -A main.ts", Some("ts")),
            ("PORT=1 go run .", Some("go")),
            ("./bin/svc --port $PORT", None),
            ("docker run --rm -p 7700:7700 meilisearch", None),
            ("", None),
        ] {
            assert_eq!(Lang::of(run).map(Lang::badge), want, "{run}");
        }
    }
}
