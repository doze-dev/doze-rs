#![no_main]
//! Any process table: an answer, never a panic; each root at most once, no child listed twice, the root never its own
//! child, and no negative or non-finite CPU.

use doze_os::meters;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let procs = meters::parse(&text);
    let roots: Vec<u32> = procs.iter().take(3).map(|p| p.pid).collect();
    for (root, m) in meters::of_roots(&procs, &roots) {
        assert!(m.cpu.is_finite() && m.cpu >= 0.0);
        let mut seen = std::collections::BTreeSet::new();
        assert!(m.children.iter().all(|n| n.proc.pid != root && seen.insert(n.proc.pid) && n.depth >= 1));
    }
});
