//! One value in full, as the value viewer shows it (module protocol minor 2): the parts read so far put together,
//! what they are (JSON, text, bytes, or key–value pairs beside a body), how big the whole is, and the body laid out
//! as JSON lines, hex rows or text. The daemon's refusals are said as the daemon said them, with what a person can
//! do about each: an asleep instance opens with a press that wakes it; one that is off, failed or starting is not
//! woken.

use super::json::{self, JsonLine};
use doze_control::{ValueView, fault};

/// What the body is shown as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    Json,
    Text,
    Bytes,
}

/// The value read so far.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ValueDoc {
    /// What the module says it is (`json`, `text`, `bytes`, `pairs`; another is shown as bytes).
    pub media: String,
    pub body: Vec<u8>,
    pub pairs: Vec<(String, String)>,
    /// The whole value's size in bytes.
    pub size: u64,
    /// More of the body is there to load.
    pub more: bool,
    /// The body laid out as JSON, when it is all JSON.
    pub json: Option<Vec<JsonLine>>,
}

impl ValueDoc {
    /// The first part.
    pub fn first(v: &ValueView) -> ValueDoc {
        let mut d = ValueDoc {
            media: v.media.clone(),
            pairs: v
                .pairs
                .iter()
                .map(|p| (p.key.clone(), p.value.clone()))
                .collect(),
            ..ValueDoc::default()
        };
        d.add(v);
        d
    }

    /// The next part ("Load the rest"): appended when it starts where the body ends; a part from elsewhere (the value
    /// changed under it) is ignored.
    pub fn add(&mut self, v: &ValueView) {
        if v.offset != self.body.len() as u64 {
            return;
        }
        if let Some(part) = v.part() {
            self.body.extend_from_slice(&part);
        }
        self.size = v.size.max(self.body.len() as u64);
        self.more = v.more;
        self.json = (!self.more && self.shown() == Shown::Json)
            .then(|| json::lines(&self.body))
            .flatten();
    }

    /// Where "Load the rest" asks from.
    pub fn next_offset(&self) -> u64 {
        self.body.len() as u64
    }

    /// What the body is shown as: as the module says, else (pairs beside a body) as what it reads as.
    pub fn shown(&self) -> Shown {
        match self.media.as_str() {
            "json" | "text" | "pairs" => {}
            _ => return Shown::Bytes,
        }
        let text = std::str::from_utf8(&self.body).is_ok()
            // A part cut inside a character is still text.
            || (self.more && self.body.get(..self.body.len().saturating_sub(3)).is_some_and(|b| std::str::from_utf8(b).is_ok()));
        match self.media.as_str() {
            "json" if text => Shown::Json,
            "pairs" if text && json::lines(&self.body).is_some() => Shown::Json,
            _ if text => Shown::Text,
            _ => Shown::Bytes,
        }
    }

    /// The body as text (a character cut at a part's end is left out).
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body)
            .trim_end_matches('\u{fffd}')
            .to_owned()
    }

    /// The header's words: what it is and how big, and how much of it is here when not all is.
    pub fn summary(&self) -> String {
        let what = match (self.media.as_str(), self.shown()) {
            ("pairs", _) if self.body.is_empty() => {
                let n = self.pairs.len();
                return format!("{n} {}", if n == 1 { "pair" } else { "pairs" });
            }
            (_, Shown::Json) => "JSON",
            (_, Shown::Text) => "Text",
            (_, Shown::Bytes) => "Bytes",
        };
        if self.more {
            format!(
                "{what} · {} of {}",
                size(self.body.len() as u64),
                size(self.size)
            )
        } else {
            format!("{what} · {}", size(self.size))
        }
    }

    /// Why the body is shown as text though the module said JSON: it is not all here, or it does not read.
    pub fn json_note(&self) -> Option<&'static str> {
        (self.shown() == Shown::Json && self.json.is_none()).then_some(if self.more {
            "Part of the JSON: load the rest to fold it."
        } else {
            "This does not read as JSON, so it is shown as text."
        })
    }

    /// Everything here, for the clipboard: the pairs (`k: v` lines) then the body.
    pub fn copy_text(&self) -> String {
        let mut out: Vec<String> = self
            .pairs
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        if !self.body.is_empty() {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push(match self.shown() {
                Shown::Bytes => hex(&self.body).join("\n"),
                _ => self.text(),
            });
        }
        out.join("\n")
    }
}

/// The body as hex rows: offset, sixteen bytes in two groups of eight, and the bytes as text (`.` where none prints).
pub fn hex(body: &[u8]) -> Vec<String> {
    body.chunks(16)
        .enumerate()
        .map(|(i, c)| {
            let mut h = String::with_capacity(50);
            for (j, b) in c.iter().enumerate() {
                if j == 8 {
                    h.push(' ');
                }
                h.push_str(&format!("{b:02x} "));
            }
            let ascii: String = c
                .iter()
                .map(|&b| {
                    if (0x20..0x7f).contains(&b) {
                        b as char
                    } else {
                        '.'
                    }
                })
                .collect();
            format!("{:08x}  {h:<49} {ascii}", i * 16)
        })
        .collect()
}

/// A size in the unit it reads in: `512 B`, `1.2 KB`, `3.4 MB`, `2.0 GB`.
pub fn size(n: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if n < 1024 {
        return format!("{n} B");
    }
    #[allow(clippy::cast_precision_loss)] // WHY: a size to one decimal place
    let mut v = n as f64 / 1024.;
    let mut u = 0;
    while v >= 1024. && u + 1 < UNITS.len() {
        v /= 1024.;
        u += 1;
    }
    if v < 10. {
        format!("{v:.1} {}", UNITS.get(u).unwrap_or(&"TB"))
    } else {
        format!("{v:.0} {}", UNITS.get(u).unwrap_or(&"TB"))
    }
}

/// Why a value (or a folder) could not be read, as the daemon said it, and what a person can do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub message: String,
    pub code: String,
}

impl Refusal {
    /// From the daemon's error.
    pub fn of(e: &crate::Error) -> Refusal {
        match e {
            crate::Error::Control(doze_control::Error::Daemon { message, code }) => Refusal {
                message: message.clone(),
                code: code.clone(),
            },
            other => Refusal {
                message: doze_display::humanise(&other.to_string()).to_owned(),
                code: String::new(),
            },
        }
    }

    /// The instance sleeps: opening the value (a press) wakes it.
    pub fn asleep(&self) -> bool {
        self.code == fault::ASLEEP
    }

    /// The instance is starting: asked again in a moment, it answers.
    pub fn starting(&self) -> bool {
        self.code == fault::STARTING
    }

    /// The daemon's code for the state the instance is in, when that is why (`fault::ASLEEP`, `OFF`, `FAILED`,
    /// `STARTING`); `None` for a refusal of another kind (the module's own).
    pub fn state(&self) -> Option<&str> {
        [fault::ASLEEP, fault::OFF, fault::FAILED, fault::STARTING]
            .contains(&self.code.as_str())
            .then_some(self.code.as_str())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn part(media: &str, body: &[u8], offset: u64, size: u64, more: bool) -> ValueView {
        let mut v = ValueView {
            media: media.into(),
            size,
            offset,
            more,
            ..ValueView::default()
        };
        v.set_body(body);
        v
    }

    #[test]
    fn parts_add_up_and_json_folds_once_whole() {
        let mut d = ValueDoc::first(&part("json", br#"{"a":[1,"#, 0, 12, true));
        assert!(d.json.is_none());
        assert_eq!(
            d.json_note(),
            Some("Part of the JSON: load the rest to fold it.")
        );
        assert_eq!(d.summary(), "JSON · 8 B of 12 B");
        d.add(&part("json", b"2,3]}", 3, 12, false));
        assert_eq!(d.body.len(), 8, "a part from elsewhere is ignored");
        d.add(&part("json", b"2]}x", 8, 12, false));
        assert!(d.json.is_none(), "not JSON");
        let mut d = ValueDoc::first(&part("json", br#"{"a":[1,"#, 0, 11, true));
        d.add(&part("json", b"2]}", 8, 11, false));
        assert_eq!(d.json.as_ref().map(Vec::len), Some(6));
        assert_eq!(d.summary(), "JSON · 11 B");
    }

    #[test]
    fn what_a_body_is_shown_as() {
        let shown =
            |m: &str, b: &[u8]| ValueDoc::first(&part(m, b, 0, b.len() as u64, false)).shown();
        assert_eq!(shown("json", b"{}"), Shown::Json);
        assert_eq!(shown("text", b"hi"), Shown::Text);
        assert_eq!(shown("text", &[0xff, 0]), Shown::Bytes);
        assert_eq!(shown("pairs", b"{\"a\":1}"), Shown::Json);
        assert_eq!(shown("pairs", b"plain"), Shown::Text);
        assert_eq!(shown("bytes", b"hi"), Shown::Bytes);
        assert_eq!(
            shown("parquet", b"hi"),
            Shown::Bytes,
            "a media this doze does not know"
        );
    }

    #[test]
    fn hex_rows_read_like_a_dump() {
        let rows = hex(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\xff");
        assert_eq!(
            rows,
            [
                "00000000  89 50 4e 47 0d 0a 1a 0a  00 00 00 0d 49 48 44 52  .PNG........IHDR",
                "00000010  ff                                                .",
            ]
        );
    }

    #[test]
    fn sizes_and_pairs() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1234), "1.2 KB");
        assert_eq!(size(2_400_000_000), "2.2 GB");
        let mut v = part("pairs", b"", 0, 0, false);
        v.pairs = vec![doze_control::PairView {
            key: "source".into(),
            value: "api".into(),
        }];
        let d = ValueDoc::first(&v);
        assert_eq!(d.summary(), "1 pair");
        assert_eq!(d.copy_text(), "source: api");
    }

    #[test]
    fn a_refusal_names_the_state() {
        let r = Refusal::of(&crate::Error::Control(doze_control::Error::Daemon {
            message: "db is asleep. Opening a value wakes it.".into(),
            code: fault::ASLEEP.into(),
        }));
        assert!(r.asleep());
        assert_eq!(r.state(), Some(fault::ASLEEP));
    }
}
