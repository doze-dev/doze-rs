use std::collections::BTreeSet;
use std::path::Path;

use super::*;

fn dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn every_file_of_assets_is_in_the_manifest_and_every_entry_has_its_file() {
    // A `x.gz` is the compressed form of `x` (the next test says it is current): it stands for `x`.
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir("assets"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .map(|n| n.strip_suffix(".gz").map_or(n.clone(), str::to_owned))
        .collect();
    let listed: BTreeSet<String> = MANIFEST.iter().map(|f| f.name.to_owned()).collect();
    let unlisted: Vec<_> = on_disk.difference(&listed).collect();
    let missing: Vec<_> = listed.difference(&on_disk).collect();
    assert!(
        unlisted.is_empty(),
        "files in assets/ that the manifest does not list: {unlisted:?}"
    );
    assert!(
        missing.is_empty(),
        "manifest entries with no file: {missing:?}"
    );
    assert_eq!(listed.len(), MANIFEST.len(), "a name is listed twice");
}

#[test]
fn a_licence_is_kept_for_every_third_party_file() {
    let names: BTreeSet<_> = MANIFEST.iter().map(|f| f.name).collect();
    for licence in [
        "inter.LICENSE.txt",
        "jetbrains-mono.LICENSE.txt",
        "htmx.LICENSE.txt",
        "idiomorph.LICENSE.txt",
        "alpine.LICENSE.txt",
    ] {
        assert!(names.contains(licence), "{licence}");
        assert!(
            std::fs::read_to_string(dir("assets").join(licence))
                .unwrap()
                .len()
                > 200
        );
    }
}

#[test]
fn the_stylesheet_is_the_kit_and_the_consoles_rules_joined() {
    let joined: String = [
        "design/kit/paper.css",
        "design/kit/kafka.css",
        "design/console.css",
    ]
    .iter()
    .map(|p| std::fs::read_to_string(dir(p)).unwrap())
    .collect();
    let path = dir("assets/console.css");
    if std::env::var_os("DOZE_UPDATE_DESIGN").is_some() {
        std::fs::write(&path, &joined).unwrap();
    }
    assert!(
        std::fs::read_to_string(&path).unwrap() == joined,
        "assets/console.css is not design/kit/{{paper,kafka}}.css + design/console.css: run the tests with DOZE_UPDATE_DESIGN=1"
    );
}

/// What `x.gz` must inflate to: the source, the stylesheet with its font placeholders replaced by the fonts' hashed names.
fn expected_plain(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(dir("assets").join(name)).unwrap();
    if name != "console.css" {
        return bytes;
    }
    let mut text = String::from_utf8(bytes).unwrap();
    for f in MANIFEST.iter().filter(|f| f.kind == Kind::Font) {
        let font = std::fs::read(dir("assets").join(f.name)).unwrap();
        text = text.replace(
            &format!("@asset:{}", f.name),
            &hashed(f.name, &fingerprint(&font)),
        );
    }
    text.into_bytes()
}

#[test]
fn every_compressed_file_is_current_and_text_is_embedded_only_compressed() {
    let mut packed = 0;
    for f in MANIFEST {
        let on_disk = dir("assets").join(format!("{}.gz", f.name));
        assert_eq!(
            f.packed,
            matches!(f.kind, Kind::Css | Kind::Js),
            "{}: the stylesheet and the scripts are embedded as gzip, nothing else is",
            f.name
        );
        assert_eq!(on_disk.exists(), f.packed, "{}: its .gz", f.name);
        if !f.packed {
            continue;
        }
        packed += 1;
        let embedded = f.bytes.unwrap();
        assert_eq!(embedded, std::fs::read(&on_disk).unwrap(), "{}", f.name);
        let mut inflated = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(embedded), &mut inflated)
            .unwrap();
        assert!(
            inflated == expected_plain(f.name),
            "{}.gz is stale: run `bun tools/assets-compress.ts` (after `DOZE_UPDATE_DESIGN=1 cargo test -p kafka-console` for the stylesheet)",
            f.name
        );
    }
    assert_eq!(packed, 9);
}

#[test]
fn a_name_changes_with_its_content_and_only_then() {
    assert_eq!(fingerprint(b"abc"), fingerprint(b"abc"));
    assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
    assert_ne!(fingerprint(b""), fingerprint(b"\0"));
    assert_eq!(fingerprint(b"").len(), 12);
    assert_eq!(hashed("console.css", "abc"), "console.abc.css");
    assert_eq!(hashed("console.min.js", "abc"), "console.min.abc.js");
}

#[test]
fn every_served_file_is_found_by_its_hashed_name_and_the_stylesheet_names_its_fonts() {
    let assets = Assets::build(&Base::new("/k"));
    assert!(assets.urls.css.starts_with("/k/static/console.") && assets.urls.css.ends_with(".css"));
    let css_name = assets.urls.css.rsplit('/').next().unwrap();
    let css = assets.get(css_name).unwrap().inflated().unwrap();
    let css = std::str::from_utf8(&css).unwrap();
    assert!(!css.contains("@asset:"), "an unresolved font name");
    for font in [
        "inter-latin-wght-normal",
        "jetbrains-mono-latin-400-normal",
        "jetbrains-mono-latin-500-normal",
    ] {
        let name = assets.names().find(|n| n.starts_with(font)).unwrap();
        assert!(css.contains(&format!("url({name})")), "{font}");
        assert_eq!(assets.get(name).unwrap().content_type, "font/woff2");
    }
    let urls = assets.urls.scripts.iter().chain([&assets.urls.favicon]);
    assert_eq!(assets.urls.scripts.len(), SCRIPT_ORDER.len());
    for url in urls {
        assert!(
            assets.get(url.rsplit('/').next().unwrap()).is_some(),
            "{url}"
        );
    }
    assert!(
        assets.get("console.css").is_none(),
        "an unhashed name is not served"
    );
}

#[test]
fn the_shipped_files_weigh_what_the_stack_decision_says() {
    // About 330 KB in all, fonts included (docs/port/console/client-stack.md): about 140 KB before the stack, 155 KB of vendored
    // script with it, and the console's own scripts. The text is embedded as gzip (96 KB for the 311 KB of it): what is in the
    // binary is about 190 KB, and the sources' own sizes are what the stack decision counted.
    let total = Assets::build(&Base::root()).total_bytes();
    assert!((150_000..260_000).contains(&total), "{total}");
    let sources: usize = MANIFEST
        .iter()
        .filter(|f| f.bytes.is_some())
        .map(|f| expected_plain(f.name).len())
        .sum();
    assert!((250_000..420_000).contains(&sources), "{sources}");
}

#[test]
fn the_vendored_script_stays_within_the_budget_of_the_stack_decision() {
    let vendored: usize = MANIFEST
        .iter()
        .filter(|f| f.kind == Kind::Js && f.name.ends_with(".min.js"))
        .map(|f| expected_plain(f.name).len())
        .sum();
    assert!(vendored < 160_000, "vendored script is {vendored} bytes");
    assert!(
        vendored > 100_000,
        "vendored script is {vendored} bytes: is a library missing?"
    );
}

#[test]
fn no_script_of_ours_evaluates_a_string() {
    for name in ["console.js", "components.js", "multiselect.js"] {
        let text = std::fs::read_to_string(dir("assets").join(name)).unwrap();
        for banned in ["eval(", "new Function", "setTimeout(\"", "document.write"] {
            assert!(!text.contains(banned), "{name} uses {banned}");
        }
    }
}
