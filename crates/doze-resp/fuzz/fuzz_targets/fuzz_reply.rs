#![no_main]
//! Any bytes, fed to the reply parser whole and cut short, without a panic: a reply never claims more bytes than it
//! was given, and a prefix of a complete reply is either incomplete or the same reply.

use doze_resp::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(Some((reply, used))) = parse(data) else {
        let _ = parse(&data[..data.len() / 2]);
        return;
    };
    assert!(used <= data.len(), "used {used} of {}", data.len());
    assert_eq!(parse(&data[..used]), Ok(Some((reply, used))));
    if used > 0 {
        assert_eq!(parse(&data[..used - 1]), Ok(None), "a cut reply must wait for more");
    }
});
