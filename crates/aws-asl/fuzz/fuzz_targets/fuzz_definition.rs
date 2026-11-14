#![no_main]
//! Any text as a state machine definition: no panic, and a definition that is accepted runs (bounded) on an empty input.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let Ok(def) = aws_asl::parse(text) else { return };
    let (mut ex, _) = aws_asl::Exec::start(&def, aws_asl::Start { input: "{}".into(), ..aws_asl::Start::default() }, 0);
    let _ = ex.run(&def, &aws_asl::Env::new(0, 1));
});
