use std::path::{Path, PathBuf};

use super::*;

fn web_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web")
}

/// `web/scripts/source-hash.ts`, the same walk and the same hash: FNV-1a 64 over path, 0, content, 0, sorted by path.
fn source_hash(root: &Path) -> String {
    const SKIP_DIRS: [&str; 6] = [
        "node_modules",
        "e2e",
        "dist",
        "test-results",
        "playwright-report",
        ".tmp",
    ];
    fn skip_file(name: &str) -> bool {
        name == "README.md"
            || name == "eslint.config.js"
            || name.ends_with(".test.ts")
            || name == ".DS_Store"
    }
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read web/").flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) {
                    walk(root, &path, out);
                }
            } else if !skip_file(&name) {
                let rel = path.strip_prefix(root).expect("under web/");
                let parts: Vec<_> = rel
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
    let mut files = Vec::new();
    walk(root, root, &mut files);
    files.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for rel in &files {
        eat(rel.as_bytes());
        eat(&[0]);
        eat(&std::fs::read(root.join(rel)).expect("read a source"));
        eat(&[0]);
    }
    format!("{h:016x}")
}

#[test]
fn the_embedded_bundle_was_built_from_the_current_web_sources() {
    let want = source_hash(&web_dir());
    let have = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web-dist/source-hash.txt"),
    )
    .unwrap_or_default();
    assert_eq!(
        have.trim(),
        want,
        "crates/kafka-console/web-dist is stale: run `bun run build` in web/ and commit web-dist"
    );
}

#[test]
fn every_text_file_of_the_bundle_inflates_to_its_source() {
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web-dist");
    for (path, f) in FILES {
        let plain = std::fs::read(dist.join(path)).expect("the plain file beside its .gz");
        assert_eq!(
            f.inflated().expect("inflates").as_ref(),
            plain.as_slice(),
            "{path}"
        );
    }
    assert!(FILES.iter().any(|(p, _)| *p == "index.html"));
}

#[test]
fn the_bundle_has_no_inline_script_and_evaluates_no_string() {
    for (path, f) in FILES {
        let text = String::from_utf8_lossy(&f.inflated().expect("inflates")).into_owned();
        if path.ends_with(".html") {
            for tag in text.split("<script").skip(1) {
                let head = tag.split('>').next().unwrap_or("");
                assert!(head.contains("src="), "{path}: an inline script");
            }
        }
        if path.ends_with(".js") {
            assert!(!text.contains("eval("), "{path}: eval");
            assert!(!text.contains("new Function("), "{path}: new Function");
        }
    }
}

#[test]
fn index_html_is_rewritten_for_the_root_and_the_prefix() {
    let html = "<head>\n<script type=\"module\" src=\"./assets/a.js\"></script><link href=\"./favicon.svg\"></head>";
    let root = rewrite_index(html, &Base::root());
    assert!(root.contains("src=\"/assets/a.js\""), "{root}");
    assert!(root.contains("href=\"/favicon.svg\""), "{root}");
    assert!(
        root.contains("<meta name=\"doze-base\" content=\"\">"),
        "{root}"
    );
    let kafka = rewrite_index(html, &Base::new("/kafka"));
    assert!(kafka.contains("src=\"/kafka/assets/a.js\""), "{kafka}");
    assert!(kafka.contains("content=\"/kafka\""), "{kafka}");
    let hostile = rewrite_index(html, &Base::new("/a\"b<c"));
    assert!(!hostile.contains("a\"b"), "{hostile}");
}

#[test]
fn the_built_index_points_at_files_the_bundle_has() {
    let b = Bundle::build(&Base::root());
    let html = String::from_utf8_lossy(b.index_html()).into_owned();
    let mut seen = 0;
    for part in html.split("\"/assets/").skip(1) {
        let name = part.split('"').next().unwrap_or("");
        assert!(
            b.paths().any(|p| p == format!("assets/{name}")),
            "index.html names assets/{name}"
        );
        seen += 1;
    }
    assert!(seen >= 2, "a script and a stylesheet: {html}");
}

/// SEC-246: the page layer writes every value as text. No source of ours hands React HTML to inject (`dangerouslySetInnerHTML`)
/// or writes markup into the DOM itself (`innerHTML`, `outerHTML`, `insertAdjacentHTML`, `document.write`).
#[test]
fn no_source_of_the_page_layer_injects_html() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("read web/src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "ts" || e == "tsx") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&web_dir().join("src"), &mut files);
    assert!(files.len() > 50, "{}", files.len());
    for path in files {
        let text = std::fs::read_to_string(&path).expect("read a source");
        for word in [
            "dangerouslySetInnerHTML",
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "document.write",
        ] {
            assert!(!text.contains(word), "{}: {word}", path.display());
        }
    }
}
