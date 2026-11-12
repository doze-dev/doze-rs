use std::collections::BTreeSet;
use std::path::Path;

use super::*;

fn dir(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(name)
}

#[test]
fn every_file_of_assets_is_in_the_manifest_and_every_entry_has_its_file() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir("assets"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
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
    let css = std::str::from_utf8(&assets.get(css_name).unwrap().bytes).unwrap();
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
    // 337 KB in all, fonts included (docs/port/console/client-stack.md): about 140 KB before the stack, 155 KB of vendored
    // script with it, and the console's own two scripts.
    let total = Assets::build(&Base::root()).total_bytes();
    assert!((250_000..420_000).contains(&total), "{total}");
}

#[test]
fn the_vendored_script_stays_within_the_budget_of_the_stack_decision() {
    let vendored: usize = MANIFEST
        .iter()
        .filter(|f| f.kind == Kind::Js && f.name.ends_with(".min.js"))
        .map(|f| f.bytes.map_or(0, <[u8]>::len))
        .sum();
    assert!(vendored < 160_000, "vendored script is {vendored} bytes");
    assert!(vendored > 100_000, "vendored script is {vendored} bytes: is a library missing?");
}

#[test]
fn no_script_of_ours_evaluates_a_string() {
    for name in ["console.js", "components.js"] {
        let text = std::fs::read_to_string(dir("assets").join(name)).unwrap();
        for banned in ["eval(", "new Function", "setTimeout(\"", "innerHTML ="] {
            assert!(!text.contains(banned), "{name} uses {banned}");
        }
    }
}
