#![no_main]
//! Any text, as each thing setup and trust read from the machine (resolv.conf, the hosts file, a systemd unit,
//! `resolvectl domain`, `security find-certificate -Z`, the NRPT rule listing, a PEM bundle), without a panic; and the
//! hosts block's two promises: writing it twice is writing it once, and it is there afterwards.

use doze_names::fuzzing::{
    detect_from, link_domains, netd_serves, nrpt_rules, pem_blocks, replace_block, sha256_lines,
};
use libfuzzer_sys::fuzz_target;

const BLOCK: &str = "# BEGIN doze — managed, safe to delete\n127.0.0.2\taws.doze\n# END doze\n";

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let _ = detect_from(&text);
    let _ = netd_serves(&text);
    let _ = link_domains(&text);
    let _ = nrpt_rules(&text);
    for fp in sha256_lines(&text) {
        assert!(fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit()));
    }
    for block in pem_blocks(&text) {
        assert!(block.starts_with("-----BEGIN ") && block.ends_with("-----"));
    }
    let once = replace_block(&text, BLOCK);
    assert!(once.contains(BLOCK));
    assert_eq!(replace_block(&once, BLOCK), once);
    let _ = replace_block(&text, "");
});
