//! `doze fmt` against Go's `hclwrite.Format` (owner decision O11): every source Go accepts,
//! formatted byte for byte as Go formats it (`oracle-dump fmt` wrote `testdata/fmt/`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use doze_hcl::fmt::format;

#[test]
fn formats_every_source_as_go_does() {
    let root = env!("CARGO_MANIFEST_DIR");
    let mut names: Vec<_> = std::fs::read_dir(format!("{root}/testdata/fmt"))
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| n.ends_with(".hcl"))
        .collect();
    names.sort();
    assert!(names.len() >= 170, "{} formatted sources", names.len());
    let mut diffs = Vec::new();
    for n in &names {
        let src = std::fs::read(format!("{root}/testdata/sources/{n}")).unwrap();
        let want = std::fs::read(format!("{root}/testdata/fmt/{n}")).unwrap();
        let got = format(&src);
        if got != want {
            let (g, w) = (
                String::from_utf8_lossy(&got),
                String::from_utf8_lossy(&want),
            );
            let line = g
                .lines()
                .zip(w.lines())
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            diffs.push(format!(
                "{n} line {}:\n   go:   {:?}\n   rust: {:?}",
                line + 1,
                w.lines().nth(line),
                g.lines().nth(line)
            ));
        }
        // Formatting is idempotent.
        assert_eq!(
            format(&want),
            want,
            "{n}: formatting Go's output changed it"
        );
    }
    eprintln!(
        "fmt: {} of {} identical",
        names.len() - diffs.len(),
        names.len()
    );
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// Every syntax case (random fragments, mutants, refused files): Go formats tokens, accepted or
/// not, and so does doze. Go's output is kept as an FNV-1a hash (testdata/tools/fmthash.ts).
#[test]
fn formats_broken_and_random_input_as_go_does() {
    let root = env!("CARGO_MANIFEST_DIR");
    let cases: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(format!("{root}/testdata/syntax/cases.json")).unwrap(),
    )
    .unwrap();
    let go: Vec<serde_json::Value> =
        std::fs::read_to_string(format!("{root}/testdata/fmt/go-cases.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
    assert_eq!(cases.len(), go.len());
    let mut diffs = Vec::new();
    for (c, g) in cases.iter().zip(&go) {
        assert_eq!(c["id"], g["id"]);
        let got = format(c["src"].as_str().unwrap().as_bytes());
        if fnv(&got) != g["h"].as_str().unwrap() {
            diffs.push(format!(
                "{}: {:?}\n   rust: {:?}",
                c["id"],
                c["src"],
                String::from_utf8_lossy(&got)
            ));
        }
    }
    eprintln!(
        "fmt cases: {} of {} identical",
        cases.len() - diffs.len(),
        cases.len()
    );
    assert!(
        diffs.is_empty(),
        "{} differ:\n{}",
        diffs.len(),
        diffs
            .iter()
            .take(15)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn fnv(b: &[u8]) -> String {
    let h = b.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, x| {
        (h ^ u64::from(*x)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{h:016x}")
}

/// The H3 audit's cases that once differed: Indic conjuncts (Go's textseg is Unicode 15.0, so
/// grapheme clusters are counted without rule GB9c) in names, strings and comments being aligned.
#[test]
fn formats_the_audit_cases_as_go_does() {
    let root = env!("CARGO_MANIFEST_DIR");
    let cases: Vec<serde_json::Value> = serde_json::from_str(
        &std::fs::read_to_string(format!("{root}/testdata/fmt/cases-audit.json")).unwrap(),
    )
    .unwrap();
    let go: Vec<serde_json::Value> =
        std::fs::read_to_string(format!("{root}/testdata/fmt/go-audit.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
    assert_eq!(cases.len(), go.len());
    let bad: Vec<_> = cases
        .iter()
        .zip(&go)
        .filter(|(c, g)| {
            fnv(&format(c["src"].as_str().unwrap().as_bytes())) != g["h"].as_str().unwrap()
        })
        .map(|(c, _)| c["id"].to_string())
        .collect();
    assert!(bad.is_empty(), "{} differ: {bad:?}", bad.len());
}

/// Bytes that are not UTF-8 (a surrogate, a code point past U+10FFFF, an overlong form) count as
/// one column each when aligning, as in Go.
#[test]
fn formats_invalid_utf8_as_go_does() {
    let root = env!("CARGO_MANIFEST_DIR");
    for e in std::fs::read_dir(format!("{root}/testdata/fmt-bytes/go")).unwrap() {
        let name = e.unwrap().file_name().into_string().unwrap();
        let src = std::fs::read(format!("{root}/testdata/fmt-bytes/src/{name}")).unwrap();
        let want = std::fs::read(format!("{root}/testdata/fmt-bytes/go/{name}")).unwrap();
        assert_eq!(format(&src), want, "{name}");
    }
}
