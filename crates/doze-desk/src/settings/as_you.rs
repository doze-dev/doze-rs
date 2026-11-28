//! What a script the app ran as the person came to: macOS's System trust settings, which macOS changes only through
//! its own dialog (as root under the app's administrator prompt it refuses: "no user interaction was possible").

use super::repair::Fail;

/// From its exit and what it printed, in the words of `security`: Security framework's own for a cancelled dialog
/// (errAuthorizationCanceled) and for one it could not show (errAuthorizationInteractionNotAllowed); else its last line.
pub(super) fn outcome(ok: bool, text: &str) -> Result<(), Fail> {
    if ok {
        return Ok(());
    }
    let low = text.to_ascii_lowercase();
    // The person's part runs first (a removal of trust), so a cancel there leaves everything as it was.
    if low.contains("canceled by the user") || low.contains("cancelled by the user") {
        return Err(Fail::Cancelled);
    }
    if low.contains("no user interaction was possible") {
        return Err(Fail::Failed(
            "macOS could not show its dialog here: run it in Terminal on this Mac".into(),
        ));
    }
    let last = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("it failed and said nothing");
    Err(Fail::Failed(last.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::{Fail, outcome};

    #[test]
    fn a_cancelled_or_missing_dialog_is_said_plainly() {
        assert_eq!(outcome(true, ""), Ok(()));
        let said = |t: &str| match outcome(false, t) {
            Err(Fail::Failed(why)) => why,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            outcome(
                false,
                "SecTrustSettingsRemoveTrustSettings: The authorization was canceled by the user."
            ),
            Err(Fail::Cancelled)
        );
        assert!(
            said("The authorization was denied since no user interaction was possible.")
                .contains("Terminal")
        );
        assert_eq!(said("one\nlast words\n\n"), "last words");
        assert_eq!(said(""), "it failed and said nothing");
    }
}
