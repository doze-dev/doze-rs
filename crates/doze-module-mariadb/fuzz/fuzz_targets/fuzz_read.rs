#![no_main]
//! Any text, read as a grant object's name and as the privilege query's output, without a panic; a name that splits
//! puts back together as itself with a target the module would write; and any database and table names, written as
//! `sql::target` writes them, read back as a whole target.

#[path = "../../src/read.rs"]
#[allow(dead_code)]
mod read;

use libfuzzer_sys::fuzz_target;

fn quote(s: &str) -> String {
    format!("`{}`", s.replace('`', "``"))
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    if let Some((user, host, on)) = read::grant_name(s, read::target_ok) {
        assert!(read::target_ok(on));
        assert_eq!(format!("{user}@{host} on {on}"), s);
    }
    let _ = read::privileges(s);
    let (db, table) = s.split_once('\u{1}').unwrap_or((s, s));
    for t in [quote(db) + ".*", format!("{}.{}", quote(db), quote(table)), "*.*".into()] {
        assert!(read::target_ok(&t), "{t}");
        let name = format!("a@b@% on {t}");
        assert_eq!(read::grant_name(&name, read::target_ok), Some(("a@b", "%", t.as_str())), "{name}");
    }
});
