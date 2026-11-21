//! Setting Go's files aside; the replay itself runs against the real server in tests/module_boot.rs.

use super::{KEPT, PARTIAL, set_aside, xml_all};
use std::path::PathBuf;

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doze-aws-godata-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn go_service_folders_move_aside_whole_and_once() {
    let dir = folder("aside");
    for (path, body) in [
        ("s3/meta.bolt", "x"),
        ("s3/blobs/b/00/0001", "object"),
        ("sqs/sqs.bolt", "x"),
        ("lambda/lambda.bolt", "x"),
        ("lambda/code/f/main.py", "code"),
        (".doze-converged", "fp"),
    ] {
        std::fs::create_dir_all(dir.join(path).parent().unwrap()).unwrap();
        std::fs::write(dir.join(path), body).unwrap();
    }
    std::fs::create_dir_all(dir.join("iam")).unwrap(); // the Rust server's own, with no bbolt file: stays
    assert!(set_aside(&dir).unwrap());
    assert_eq!(
        std::fs::read(dir.join(KEPT).join("s3/blobs/b/00/0001")).unwrap(),
        b"object"
    );
    assert!(dir.join(KEPT).join("lambda/code/f/main.py").is_file());
    assert!(!dir.join("s3").exists() && !dir.join("lambda").exists() && dir.join("iam").is_dir());
    assert!(dir.join(".doze-converged").is_file());
    assert!(!set_aside(&dir).unwrap(), "once");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_set_aside_stopped_half_way_is_finished() {
    let dir = folder("half");
    std::fs::create_dir_all(dir.join(PARTIAL).join("s3")).unwrap();
    std::fs::write(dir.join(PARTIAL).join("s3/meta.bolt"), "x").unwrap();
    std::fs::create_dir_all(dir.join("sqs")).unwrap();
    std::fs::write(dir.join("sqs/sqs.bolt"), "x").unwrap();
    assert!(set_aside(&dir).unwrap());
    assert!(
        dir.join(KEPT).join("s3/meta.bolt").is_file()
            && dir.join(KEPT).join("sqs/sqs.bolt").is_file()
    );
    assert!(!dir.join(PARTIAL).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_rust_folder_is_left_alone() {
    let dir = folder("rust");
    std::fs::create_dir_all(dir.join("s3/blobs")).unwrap();
    std::fs::write(dir.join("s3/s3.redb"), "x").unwrap();
    std::fs::write(dir.join("s3/meta.bolt"), "x").unwrap();
    assert!(!set_aside(&dir).unwrap());
    assert!(dir.join("s3/s3.redb").is_file() && !dir.join(KEPT).exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn xml_values_are_found_in_order() {
    let xml = "<a><Name>one</Name><x/><Name>two</Name></a>";
    assert_eq!(xml_all(xml, "Name"), ["one", "two"]);
    assert!(xml_all("<Name>open", "Name").is_empty());
}
