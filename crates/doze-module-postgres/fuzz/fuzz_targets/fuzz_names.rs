#![no_main]
//! Any text, read as a grant's object name, without a panic; and any role and schema names, rendered into every kind
//! of grant name, read back as exactly that role and target (so the names prune is given always parse, whatever
//! quotes and words they hold).

#[path = "../../src/names.rs"]
#[allow(dead_code)]
mod names;

use libfuzzer_sys::fuzz_target;
use names::On;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else { return };
    let _ = names::grant(s);
    let (role, target) = s.split_once('\u{1}').unwrap_or((s, s));
    for on in [
        On::Database(target.to_string()),
        On::Schema(target.to_string()),
        On::All("TABLES", target.to_string()),
        On::All("SEQUENCES", target.to_string()),
        On::All("FUNCTIONS", target.to_string()),
    ] {
        let name = on.name(role);
        assert_eq!(names::grant(&name), Some((role.to_string(), on)), "{name}");
    }
});
