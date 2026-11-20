use std::path::{Path, PathBuf};

use super::*;

/// The bun workspace the bundle is built from.
fn console_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../console")
}

fn dist() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("web-dist")
}

/// The hash the bundle's build script must write into `web-dist/source-hash.txt`: FNV-1a 64 over path, 0, content, 0 of every
/// source the doze-aws bundle is made from, sorted by path (bytewise), paths relative to `console/` with `/` between parts.
///
/// The sources are everything under `console/` except: the directories below, the files `skip_file` names, and every app
/// under `console/apps/` but `aws` (the gallery and doze-kafka's app do not go into this bundle). The same walk and hash as
/// doze-kafka's `web/scripts/source-hash.ts`, over the workspace.
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
            || name.ends_with(".test.tsx")
            || name == ".DS_Store"
    }
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).expect("read console/").flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let other_app = dir == root.join("apps") && name != "aws";
            if path.is_dir() {
                if !SKIP_DIRS.contains(&name.as_str()) && !other_app {
                    walk(root, &path, out);
                }
            } else if !skip_file(&name) {
                let rel = path.strip_prefix(root).expect("under console/");
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

/// The stale-bundle test. While `web-dist/` is the placeholder (its hash file says `placeholder`) there are no sources to be
/// stale against, and what is checked is that the placeholder is only that: `index.html`, saying so. From the first real build
/// on, the hash the build wrote must be the hash of the sources as they are now.
#[test]
fn the_embedded_bundle_was_built_from_the_current_sources() {
    let have = std::fs::read_to_string(dist().join("source-hash.txt")).unwrap_or_default();
    if have.trim() == "placeholder" {
        let paths: Vec<&str> = FILES.iter().map(|(p, _)| *p).collect();
        assert_eq!(paths, ["index.html"], "a placeholder is index.html alone");
        let html = String::from_utf8_lossy(Bundle::build().index_html()).into_owned();
        assert!(html.contains("is not built yet"), "{html}");
        return;
    }
    assert_eq!(
        have.trim(),
        source_hash(&console_dir()),
        "crates/aws-console/web-dist is stale: run `bun run build` in console/apps/aws and commit web-dist"
    );
}

#[test]
fn every_text_file_of_the_bundle_inflates_to_its_source() {
    for (path, f) in FILES {
        let plain = std::fs::read(dist().join(path)).expect("the plain file beside its .gz");
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
fn index_html_is_rewritten_for_deep_links() {
    let html = "<head>\n<script type=\"module\" src=\"./assets/a.js\"></script><link href=\"./favicon.svg\"></head>";
    let out = rewrite_index(html);
    assert!(out.contains("src=\"/assets/a.js\""), "{out}");
    assert!(out.contains("href=\"/favicon.svg\""), "{out}");
    assert!(
        out.contains("<meta name=\"doze-base\" content=\"\">"),
        "{out}"
    );
}

#[test]
fn the_built_index_points_at_files_the_bundle_has() {
    let b = Bundle::build();
    let html = String::from_utf8_lossy(b.index_html()).into_owned();
    for part in html.split("\"/assets/").skip(1) {
        let name = part.split('"').next().unwrap_or("");
        assert!(
            b.paths().any(|p| p == format!("assets/{name}")),
            "index.html names assets/{name}"
        );
    }
}

#[test]
fn the_hash_of_the_sources_is_sixteen_hex_digits_and_changes_with_a_file() {
    let dir = tempfile::tempdir().expect("a directory");
    let root = dir.path();
    std::fs::create_dir_all(root.join("apps/aws/src")).expect("mkdir");
    std::fs::create_dir_all(root.join("apps/gallery")).expect("mkdir");
    std::fs::create_dir_all(root.join("packages/ui/node_modules/x")).expect("mkdir");
    std::fs::write(root.join("apps/aws/src/main.tsx"), "a").expect("write");
    std::fs::write(root.join("packages/ui/button.tsx"), "b").expect("write");
    let first = source_hash(root);
    assert_eq!(first.len(), 16);
    // What does not go into this bundle does not change its hash.
    std::fs::write(root.join("apps/gallery/main.tsx"), "g").expect("write");
    std::fs::write(root.join("packages/ui/node_modules/x/i.js"), "n").expect("write");
    std::fs::write(root.join("apps/aws/src/main.test.ts"), "t").expect("write");
    assert_eq!(source_hash(root), first);
    std::fs::write(root.join("packages/ui/button.tsx"), "c").expect("write");
    assert_ne!(source_hash(root), first);
}
