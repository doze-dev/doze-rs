//! A debug build's slow start, for the tests of `doze up` (compiled out of a release build).

/// The variable a debug build's daemon reads to start slowly, as on a fresh doze home where it fetches each module
/// before it serves: `<seconds>` waits saying nothing (a stuck start), `<seconds>,download` waits between the lines a
/// `__fetch` child writes around a download. Tests of `doze up` use it, because no module they can build is in a
/// registry they can serve. A release build has no such variable: this is compiled out.
pub const SLOW_START_VAR: &str = "DOZE_TEST_SLOW_START";

/// Waits as `SLOW_START_VAR` says, if it is set.
#[allow(clippy::print_stderr)] // the daemon's stderr is its log file
pub(super) async fn slow_start_for_tests() {
    let Ok(v) = std::env::var(SLOW_START_VAR) else {
        return;
    };
    let (secs, download) = v
        .split_once(',')
        .map_or((v.as_str(), false), |(s, w)| (s, w == "download"));
    let Ok(secs) = secs.trim().parse::<u64>() else {
        return;
    };
    if download {
        eprintln!("doze: fetching module doze/slow from a test registry…");
    }
    tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    if download {
        eprintln!("doze: download done in {secs}.0s");
    }
}
