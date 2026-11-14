//! What leaves the app: a URL opened in the browser (Go `API.Open`), a file opened in its app, text on the clipboard
//! (Go `API.Copy`). One place each, so a `~/` path is the person's home wherever it comes from, and so a scripted run
//! (`crate::driver`, the `fixtures` feature) writes them to its log instead of opening anything.
//!
//! The probes the driver presses are here too: [`Probe::probe`] leaves an element's place and words for a script to
//! find; in a build without the `fixtures` feature it adds nothing.

#[cfg(feature = "fixtures")]
use gpui::Styled as _;
use gpui::{App, ClipboardItem, ParentElement};

/// `file://~/x` with the person's home in place of `~` (Go: `Open`'s one rewrite).
pub(crate) fn expand(url: &str) -> String {
    match (url.strip_prefix("file://~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => {
            format!(
                "file://{}",
                std::path::Path::new(&home).join(rest).display()
            )
        }
        _ => url.to_owned(),
    }
}

/// Opens `url` in the browser, or a `file://` in its app: no reachability or domain check first (Go: `API.Open`; a
/// browser connection through the front door is what wakes a service).
pub(crate) fn open_url(cx: &App, url: &str) {
    let url = expand(url);
    #[cfg(feature = "fixtures")]
    if crate::driver::active() {
        crate::driver::note("open", &url);
        return;
    }
    match url.strip_prefix("file://") {
        Some(path) => cx.open_with_system(std::path::Path::new(path)),
        None => cx.open_url(&url),
    }
}

/// Puts `text` on the clipboard.
pub(crate) fn copy(cx: &mut App, text: &str) {
    #[cfg(feature = "fixtures")]
    if crate::driver::active() {
        crate::driver::note("copy", text);
        return;
    }
    cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
}

/// Leaves an element's place and words for a scripted run to find.
pub(crate) trait Probe: ParentElement + Sized {
    /// `id` names it; `words` is what it says (a step may name either).
    #[cfg_attr(not(feature = "fixtures"), allow(unused_variables))]
    fn probe(self, id: impl Into<String>, words: impl Into<String>) -> Self {
        #[cfg(feature = "fixtures")]
        if crate::driver::active() {
            let (id, words) = (id.into(), words.into());
            return self.child(
                gpui::canvas(
                    move |b, _, _| crate::driver::record(&id, b, &words),
                    |_, _, _, _| {},
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            );
        }
        self
    }
}

impl<T: ParentElement + Sized> Probe for T {}

/// Opens a file (or a folder) in its app.
pub(crate) fn open_path(cx: &App, path: &std::path::Path) {
    #[cfg(feature = "fixtures")]
    if crate::driver::active() {
        crate::driver::note("open", &format!("file://{}", path.display()));
        return;
    }
    cx.open_with_system(path);
}

/// Shows a file or folder in Finder.
pub(crate) fn reveal(cx: &App, path: &std::path::Path) {
    #[cfg(feature = "fixtures")]
    if crate::driver::active() {
        crate::driver::note("reveal", &path.display().to_string());
        return;
    }
    cx.reveal_path(path);
}

/// A line a privileged job printed, written to a scripted run's log (`out …`) so a script can follow it.
#[cfg_attr(not(feature = "fixtures"), allow(unused_variables))]
pub(crate) fn note_line(said: &doze_desk::settings::Said) {
    #[cfg(feature = "fixtures")]
    if crate::driver::active() {
        use doze_desk::settings::Said;
        let (kind, text) = match said {
            Said::Run(s) => ("run", s),
            Said::Line(s) => ("out", s),
            Said::Note(s) => ("note", s),
        };
        crate::driver::note(kind, text);
    }
}
