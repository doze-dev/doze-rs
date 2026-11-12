//! The files the console ships: one manifest of `include_bytes!` lines, served under content-hashed names.
//!
//! Third-party files, with where they came from (npm registry tarballs, 2026-11-12). The client stack is decided in
//! `docs/port/console/client-stack.md`; the versions are pinned here and there:
//!
//! - `htmx-2.0.11.min.js`: `htmx.org` 2.0.11 `dist/htmx.min.js`, 0BSD (`htmx.LICENSE.txt`),
//!   `sha256 d6fdc75f204e6bdefa99b69bf1e6d4ac69b8a364f77929f45c13476b4000f717`.
//! - `idiomorph-ext-0.8.0.min.js`: `idiomorph` 0.8.0 `dist/idiomorph-ext.min.js`, 0BSD (`idiomorph.LICENSE.txt`),
//!   `sha256 5811b9a7eda14878b7dab4378bf60432e1bb6f11fcfb970cc26540642650181e`.
//! - `alpine-csp-3.17.5.min.js`: `@alpinejs/csp` 3.17.5 `dist/cdn.min.js` (the build that evaluates no string), MIT
//!   (`alpine.LICENSE.txt`), `sha256 48c170c92032e527cbae71b8ce75cf1f96ca06fa869a6120ef0fe438d8e92c36`.
//! - `alpine-anchor-3.17.5.min.js`: `@alpinejs/anchor` 3.17.5 `dist/cdn.min.js` (Floating UI inside), MIT,
//!   `sha256 ab491df26e53b7ff6e502e0fa403b1960ce742a1e4498f209d8247ecf3e9e486`.
//!
//! `console.js` (product logic: Live, the palette, copy) and `components.js` (the Alpine components) are ours. The fonts:
//!
//! - `inter-latin-wght-normal.woff2`: `@fontsource-variable/inter` 5.3.0, Latin subset, OFL 1.1 (`inter.LICENSE.txt`),
//!   `sha256 3100e775e8616cd2611beecfa23a4263d7037586789b43f035236a2e6fbd4c62`.
//! - `jetbrains-mono-latin-400-normal.woff2` and `-500-`: `@fontsource/jetbrains-mono` 5.3.0, Latin subset, OFL 1.1
//!   (`jetbrains-mono.LICENSE.txt`), `sha256 14425ba9c695763c1547f48a206b7aa60350a33ae23de09f0407877f3fcd89eb` and
//!   `cb182feeed4d798ff6961d3c79f7026279448fca0676438aaecb21f3fc39553a`.
//!
//! `console.css` is `design/kit/paper.css` + `design/kit/kafka.css` + `design/console.css`, joined; a test says so.
//! A test walks `assets/` and fails on a file that is not in the manifest and on an entry with no file.

use std::collections::HashMap;

use crate::base::Base;

/// What a file is, for its `Content-Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Css,
    Js,
    Font,
    Svg,
    /// A licence text: kept beside the file it covers, not shipped in the binary.
    Licence,
}

/// One file of `assets/`.
#[derive(Debug, Clone, Copy)]
pub struct File {
    pub name: &'static str,
    pub kind: Kind,
    /// `None` for a licence: it is in the source tree, not in the binary.
    pub bytes: Option<&'static [u8]>,
}

macro_rules! file {
    ($name:literal, $kind:ident) => {
        File {
            name: $name,
            kind: Kind::$kind,
            bytes: Some(include_bytes!(concat!("../assets/", $name))),
        }
    };
}

/// Every file of `assets/`.
pub const MANIFEST: &[File] = &[
    file!("console.css", Css),
    file!("console.js", Js),
    file!("components.js", Js),
    file!("multiselect.js", Js),
    file!("live.js", Js),
    file!("htmx-2.0.11.min.js", Js),
    file!("idiomorph-ext-0.8.0.min.js", Js),
    file!("alpine-anchor-3.17.5.min.js", Js),
    file!("alpine-csp-3.17.5.min.js", Js),
    file!("favicon.svg", Svg),
    file!("inter-latin-wght-normal.woff2", Font),
    file!("jetbrains-mono-latin-400-normal.woff2", Font),
    file!("jetbrains-mono-latin-500-normal.woff2", Font),
    File {
        name: "htmx.LICENSE.txt",
        kind: Kind::Licence,
        bytes: None,
    },
    File {
        name: "idiomorph.LICENSE.txt",
        kind: Kind::Licence,
        bytes: None,
    },
    File {
        name: "alpine.LICENSE.txt",
        kind: Kind::Licence,
        bytes: None,
    },
    File {
        name: "inter.LICENSE.txt",
        kind: Kind::Licence,
        bytes: None,
    },
    File {
        name: "jetbrains-mono.LICENSE.txt",
        kind: Kind::Licence,
        bytes: None,
    },
];

/// The scripts of a page, in load order.
pub const SCRIPT_ORDER: [&str; 8] = [
    "htmx-2.0.11.min.js",
    "idiomorph-ext-0.8.0.min.js",
    "console.js",
    "components.js",
    "multiselect.js",
    "live.js",
    "alpine-anchor-3.17.5.min.js",
    "alpine-csp-3.17.5.min.js",
];

/// A file ready to serve.
#[derive(Debug)]
pub struct Served {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Where the pages find the files they link to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetUrls {
    pub css: String,
    /// The scripts, in the order a page loads them (all deferred, so they run in this order): htmx and idiomorph, our
    /// three files (which register on `alpine:init`), then Alpine's plugin and Alpine itself, which starts when it runs.
    pub scripts: Vec<String>,
    pub favicon: String,
}

/// The served files by their hashed names, and the URLs of those the pages use.
#[derive(Debug)]
pub struct Assets {
    by_name: HashMap<String, Served>,
    pub urls: AssetUrls,
}

/// FNV-1a over the bytes, as twelve hex digits: a name that changes when the content does.
#[must_use]
pub fn fingerprint(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}

/// `console.css` and `abc123` make `console.abc123.css`.
fn hashed(name: &str, hash: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}.{hash}.{ext}"),
        None => format!("{name}.{hash}"),
    }
}

const fn content_type(kind: Kind) -> &'static str {
    match kind {
        Kind::Css => "text/css; charset=utf-8",
        Kind::Js => "text/javascript; charset=utf-8",
        Kind::Font => "font/woff2",
        Kind::Svg => "image/svg+xml",
        Kind::Licence => "text/plain; charset=utf-8",
    }
}

impl Assets {
    /// Hashes every file and links the stylesheet to the fonts' hashed names, which it names as `@asset:file`.
    #[must_use]
    pub fn build(base: &Base) -> Self {
        let served: Vec<_> = MANIFEST
            .iter()
            .filter_map(|f| f.bytes.map(|b| (f, b)))
            .collect();
        let mut names: HashMap<&str, String> = HashMap::new();
        for (f, bytes) in served.iter().filter(|(f, _)| f.kind != Kind::Css) {
            names.insert(f.name, hashed(f.name, &fingerprint(bytes)));
        }
        let mut by_name = HashMap::new();
        for (f, bytes) in &served {
            let mut data = bytes.to_vec();
            if f.kind == Kind::Css {
                let mut text = String::from_utf8_lossy(bytes).into_owned();
                for (plain, hashed_name) in &names {
                    text = text.replace(&format!("@asset:{plain}"), hashed_name);
                }
                data = text.into_bytes();
                names.insert(f.name, hashed(f.name, &fingerprint(&data)));
            }
            let name = names.get(f.name).cloned().unwrap_or_default();
            by_name.insert(
                name,
                Served {
                    content_type: content_type(f.kind),
                    bytes: data,
                },
            );
        }
        let url = |plain: &str| {
            base.url(&format!(
                "/static/{}",
                names.get(plain).cloned().unwrap_or_default()
            ))
        };
        let urls = AssetUrls {
            css: url("console.css"),
            scripts: SCRIPT_ORDER.iter().map(|n| url(n)).collect(),
            favicon: url("favicon.svg"),
        };
        Self { by_name, urls }
    }

    /// The file served under this hashed name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Served> {
        self.by_name.get(name)
    }

    /// The hashed names of every served file.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.by_name.keys().map(String::as_str)
    }

    /// Their total size in bytes.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.by_name.values().map(|s| s.bytes.len()).sum()
    }
}

#[cfg(test)]
mod tests;
