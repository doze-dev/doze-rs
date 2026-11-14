//! An extension bundle archive (`.tar.gz`) read into its files, with nothing else of the crate in it so the fuzz target
//! can take it whole. Only what a bundle holds is kept: `share/`, `lib/`, `bin/` and `bundle.json`, under one optional
//! top folder (doze-binaries packs `<name>-<version>-<triple>/`; Go's `source` bundles had none). Go unpacked every
//! regular file it found, wherever its name pointed; here a name that climbs out (`..`), is absolute, or a link that
//! points anywhere but beside itself refuses the whole archive.

// fuzz:parser fuzz_bundle

use std::io::Read;

/// The most an archive may unpack to, so a small file cannot fill the disk or the memory.
pub(crate) const MAX_BYTES: u64 = 256 << 20;
/// The most entries it may hold.
pub(crate) const MAX_ENTRIES: usize = 20_000;

/// One file of a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Item {
    /// Relative to the bundle's root, `/`-separated: `share/extension/x.control`, `lib/x.so`, `bundle.json`.
    pub path: String,
    pub body: Body,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Body {
    File {
        data: Vec<u8>,
        exec: bool,
    },
    /// A link to a file in the same folder (a library's version name: `libx.so.1` to `libx.so.1.2`).
    Link(String),
}

/// The files of a gzip-compressed tar archive, or why it is not a bundle.
pub(crate) fn read(gz: &[u8]) -> Result<Vec<Item>, String> {
    let mut raw = Vec::new();
    flate2::read::GzDecoder::new(gz)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("it is not a gzip file ({e})"))?;
    if u64::try_from(raw.len()).map_or(true, |n| n > MAX_BYTES) {
        return Err(format!("it unpacks to more than {} MiB", MAX_BYTES >> 20));
    }
    read_tar(&raw)
}

/// The files of an uncompressed tar archive.
pub(crate) fn read_tar(raw: &[u8]) -> Result<Vec<Item>, String> {
    place(entries(raw)?)
}

/// One raw entry: its name as written, its mode, and its data (a link: `Err` with its target).
type Raw = (String, u32, Result<Vec<u8>, String>);

/// What one 512-byte header says.
struct Header {
    name: String,
    kind: u8,
    size: usize,
    mode: u32,
    link: String,
}

fn field(h: &[u8], from: usize, len: usize) -> &[u8] {
    let f = h.get(from..from + len).unwrap_or_default();
    let end = f.iter().position(|&b| b == 0).unwrap_or(f.len());
    f.get(..end).unwrap_or_default()
}

fn octal(f: &[u8]) -> Result<u64, String> {
    let s = String::from_utf8_lossy(f);
    let t = s.trim_matches(|c: char| c == ' ' || c == '\0');
    if t.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(t, 8).map_err(|_| format!("a header holds {t:?} where a number belongs"))
}

fn text(f: &[u8]) -> Result<String, String> {
    String::from_utf8(f.to_vec()).map_err(|_| "a name is not UTF-8".to_string())
}

fn header(block: &[u8]) -> Result<Header, String> {
    let name = text(field(block, 0, 100))?;
    let prefix = text(field(block, 345, 155))?;
    let ustar = field(block, 257, 5) == b"ustar";
    let name = if ustar && !prefix.is_empty() {
        format!("{prefix}/{name}")
    } else {
        name
    };
    let size =
        usize::try_from(octal(field(block, 124, 12))?).map_err(|_| "an entry is too large")?;
    let mode = u32::try_from(octal(field(block, 100, 8))? & 0o7777).unwrap_or(0);
    Ok(Header {
        name,
        kind: block.get(156).copied().unwrap_or(0),
        size,
        mode,
        link: text(field(block, 157, 100))?,
    })
}

/// A pax extended header's `path` and `linkpath`.
fn pax(data: &[u8]) -> (Option<String>, Option<String>) {
    let (mut path, mut link) = (None, None);
    let text = String::from_utf8_lossy(data);
    let mut rest: &str = &text;
    while let Some((len, _)) = rest.split_once(' ') {
        // A record counts its own length digits and the space: anything shorter cannot move the reader on.
        let Some(n) = len.parse::<usize>().ok().filter(|n| *n > len.len()) else {
            break;
        };
        let Some(record) = rest.get(..n) else { break };
        rest = rest.get(n..).unwrap_or_default();
        let kv = record.split_once(' ').map_or("", |(_, kv)| kv);
        let kv = kv.strip_suffix('\n').unwrap_or(kv);
        match kv.split_once('=') {
            Some(("path", v)) => path = Some(v.to_string()),
            Some(("linkpath", v)) => link = Some(v.to_string()),
            _ => {}
        }
    }
    (path, link)
}

/// The raw entries: regular files and links with their names as written (long names and pax names applied).
fn entries(raw: &[u8]) -> Result<Vec<Raw>, String> {
    let mut out = Vec::new();
    let (mut at, mut long, mut long_link) = (0usize, None::<String>, None::<String>);
    while let Some(block) = raw.get(at..at + 512) {
        if block.iter().all(|&b| b == 0) {
            break;
        }
        let h = header(block)?;
        let start = at + 512;
        let data = raw
            .get(start..start + h.size)
            .ok_or("the archive ends inside an entry")?;
        at = start + h.size.div_ceil(512) * 512;
        match h.kind {
            b'L' => long = Some(text(field(data, 0, data.len()))?),
            b'K' => long_link = Some(text(field(data, 0, data.len()))?),
            b'x' => {
                let (p, l) = pax(data);
                long = p.or(long);
                long_link = l.or(long_link);
            }
            b'0' | 0 | b'2' => {
                let name = long.take().unwrap_or(h.name);
                let link = long_link.take().unwrap_or(h.link);
                // An old archiver's folder: a plain entry whose name ends in `/`.
                if name.ends_with('/') && h.kind != b'2' {
                    continue;
                }
                let body = if h.kind == b'2' {
                    Err(link)
                } else {
                    Ok(data.to_vec())
                };
                out.push((name, h.mode, body));
            }
            // Folders, global headers, and hard links (GNU tar writes one for a second copy of a library; as Go did,
            // it is left out, and a bundle that needs it fails its own checks).
            b'5' | b'g' | b'1' => {
                long = None;
                long_link = None;
            }
            k => {
                return Err(format!(
                    "entry {:?} is of a kind a bundle has no use for ({})",
                    h.name,
                    char::from(k)
                ));
            }
        }
        if out.len() > MAX_ENTRIES {
            return Err(format!("it holds more than {MAX_ENTRIES} files"));
        }
    }
    Ok(out)
}

/// A name as a path inside the bundle: relative, no `.`/`..`/empty part, no backslash or NUL.
fn clean(name: &str) -> Option<Vec<&str>> {
    let name = name.strip_prefix("./").unwrap_or(name);
    let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
    let bad = name.starts_with('/')
        || parts.is_empty()
        || parts
            .iter()
            .any(|p| *p == "." || *p == ".." || p.contains('\\') || p.contains('\0'));
    (!bad).then_some(parts)
}

/// Whether a path (without a top folder) is part of a bundle.
/// macOS tar adds an AppleDouble `._<name>` beside a file with extended attributes; copied into a toolchain, a
/// `._x.control` would read as an extension, so those are left out.
fn kept(parts: &[&str]) -> bool {
    if parts.last().is_some_and(|p| p.starts_with("._")) {
        return false;
    }
    match parts {
        ["bundle.json"] => true,
        [top, _, ..] => matches!(*top, "share" | "lib" | "bin"),
        _ => false,
    }
}

/// The entries as bundle files: one common top folder taken off when the names are not already at the root.
fn place(raw: Vec<Raw>) -> Result<Vec<Item>, String> {
    let mut named = Vec::new();
    for (name, mode, body) in raw {
        // macOS's zip-and-tar habit: resource forks under `__MACOSX/` are never part of a bundle.
        if name.trim_start_matches("./").starts_with("__MACOSX/") {
            continue;
        }
        let parts =
            clean(&name).ok_or_else(|| format!("entry {name:?} points outside the bundle"))?;
        named.push((
            parts.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            mode,
            body,
        ));
    }
    let top = named.first().and_then(|(p, ..)| p.first().cloned());
    let strip = top.as_ref().is_some_and(|t| {
        !matches!(t.as_str(), "share" | "lib" | "bin" | "bundle.json")
            && named
                .iter()
                .all(|(p, ..)| p.first() == Some(t) && p.len() > 1)
    });
    let mut out = Vec::new();
    for (parts, mode, body) in named {
        let parts: Vec<&str> = parts
            .iter()
            .skip(usize::from(strip))
            .map(String::as_str)
            .collect();
        if !kept(&parts) {
            continue;
        }
        let path = parts.join("/");
        let body = match body {
            Ok(data) => Body::File {
                data,
                exec: mode & 0o111 != 0,
            },
            Err(link) if !link.is_empty() && !link.contains('/') && link != "." && link != ".." => {
                Body::Link(link)
            }
            Err(link) => return Err(format!("{path} links to {link:?}, outside its folder")),
        };
        out.push(Item { path, body });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(name: &str, kind: u8, size: usize, link: &str) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        for (i, b) in name.bytes().enumerate() {
            h[i] = b;
        }
        for (i, b) in format!("{:07o}\0", 0o644).bytes().enumerate() {
            h[100 + i] = b;
        }
        for (i, b) in format!("{size:011o}\0").bytes().enumerate() {
            h[124 + i] = b;
        }
        h[156] = kind;
        for (i, b) in link.bytes().enumerate() {
            h[157 + i] = b;
        }
        h
    }

    fn tar(entries: &[(&str, u8, &[u8], &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, kind, data, link) in entries {
            out.extend(block(name, *kind, data.len(), link));
            out.extend(*data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend([0u8; 1024]);
        out
    }

    #[test]
    fn a_top_folder_is_taken_off_and_only_the_bundle_is_kept() {
        let raw = tar(&[
            (
                "b-1/share/extension/x.control",
                b'0',
                b"default_version = '1'",
                "",
            ),
            ("b-1/lib/libx.so", b'2', b"", "libx.so.1"),
            ("b-1/README", b'0', b"hi", ""),
            ("b-1/share/extension/._x.control", b'0', b"junk", ""),
        ]);
        let items = place(entries(&raw).unwrap()).unwrap();
        let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["share/extension/x.control", "lib/libx.so"]);
        assert_eq!(items[1].body, Body::Link("libx.so.1".into()));
    }

    #[test]
    fn names_and_links_that_climb_out_are_refused() {
        let up = tar(&[("share/../../etc/x", b'0', b"x", "")]);
        assert!(place(entries(&up).unwrap()).is_err());
        let abs = tar(&[("/share/x", b'0', b"x", "")]);
        assert!(place(entries(&abs).unwrap()).is_err());
        let link = tar(&[("lib/x.so", b'2', b"", "../../x")]);
        assert!(place(entries(&link).unwrap()).is_err());
        let dev = tar(&[("lib/x", b'3', b"", "")]);
        assert!(entries(&dev).is_err());
        let short = tar(&[("share/x", b'0', &[1u8; 600], "")]);
        assert!(entries(short.get(..700).unwrap()).is_err());
    }

    #[test]
    fn a_pax_record_that_counts_nothing_ends_the_header() {
        assert_eq!(pax(b"0 path=x\n"), (None, None));
        assert_eq!(pax(b"1 x"), (None, None));
    }

    #[test]
    fn hard_links_old_folders_and_macos_forks_are_left_out() {
        let raw = tar(&[
            ("b/share/", b'0', b"", ""),
            ("b/share/extension/x.control", b'0', b"1", ""),
            ("b/lib/libx.so.1", b'1', b"", "b/lib/libx.so"),
            ("__MACOSX/b/._x", b'0', b"junk", ""),
        ]);
        let items = place(entries(&raw).unwrap()).unwrap();
        let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["share/extension/x.control"]);
    }

    #[test]
    fn long_and_pax_names() {
        let long = "share/extension/".to_string() + &"a".repeat(120) + ".sql";
        let pax_rec = format!("{} path={long}\n", long.len() + 10);
        let raw = tar(&[
            ("././@LongLink", b'L', long.as_bytes(), ""),
            ("share/extension/short", b'0', b"1", ""),
            ("pax", b'x', pax_rec.as_bytes(), ""),
            ("share/extension/other", b'0', b"2", ""),
        ]);
        let items = place(entries(&raw).unwrap()).unwrap();
        assert!(items.iter().all(|i| i.path == long), "{items:?}");
    }
}
