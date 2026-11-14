//! Go's hover titles (`title=`) on the app's buttons, word for word, so the window's tooltips say what Go's did.
//! A tooltip cannot show in a shot, so the words live here, with a test.

/// The Library head's Open folder… (library.tsx).
pub const OPEN_FOLDER: &str = "Open a project folder (⌘O)";
/// The Library head's Settings (library.tsx).
pub const SETTINGS: &str = "Themes, the machine, about (⌘,)";
/// The machine line's "N left behind · Clean up…" (library.tsx Machine).
pub const LEFT_BEHIND: &str =
    "Processes, data and routes stacks left behind, out of step with doze";
/// The machine line's Set up (one password) (library.tsx Machine).
pub const SET_UP: &str = "The same setup as doze doctor --fix, behind the system's password prompt";
/// The machine line's Machine… (library.tsx Machine).
pub const MACHINE: &str = "What doze set up here, part by part";
/// The domain setup banner's Set up the machine (one password) (overview.tsx `home.setup`).
pub const SET_UP_MACHINE: &str = "Runs the machine's setup (doze doctor --fix), which routes and trusts the domain, behind the system's password prompt";
/// The domain setup banner's Copy <command> (overview.tsx `home.setup`).
pub const COPY_COMMAND: &str = "For a terminal instead";
/// The logs page's Copy (logs.tsx).
pub const COPY_LINES: &str = "The lines shown, with their times";
/// The instance head's Stop trying, on a crashing service (instance.tsx).
pub const STOP_TRYING: &str = "Take it out of this run while you look";
/// A task's Copy output, once it has run (instance.tsx).
pub const COPY_OUTPUT: &str = "The last run's output, as kept here";
/// A task's Edit doze.hcl, before it has run (instance.tsx).
pub const EDIT_TASK: &str = "The task's block, in your editor";

/// A check on the machine line: Go titles it with its text (`title={c.text}`), so a check without one has none.
pub fn check(text: &str) -> Option<&str> {
    (!text.is_empty()).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_titles_are_gos_words() {
        assert_eq!(OPEN_FOLDER, "Open a project folder (⌘O)");
        assert_eq!(SETTINGS, "Themes, the machine, about (⌘,)");
        assert_eq!(
            LEFT_BEHIND,
            "Processes, data and routes stacks left behind, out of step with doze"
        );
        assert_eq!(
            SET_UP,
            "The same setup as doze doctor --fix, behind the system's password prompt"
        );
        assert_eq!(MACHINE, "What doze set up here, part by part");
        assert!(SET_UP_MACHINE.starts_with("Runs the machine's setup (doze doctor --fix)"));
        assert_eq!(COPY_COMMAND, "For a terminal instead");
        assert_eq!(COPY_LINES, "The lines shown, with their times");
        assert_eq!(STOP_TRYING, "Take it out of this run while you look");
        assert_eq!(COPY_OUTPUT, "The last run's output, as kept here");
        assert_eq!(EDIT_TASK, "The task's block, in your editor");
    }

    #[test]
    fn a_check_is_titled_with_its_text_when_it_has_one() {
        assert_eq!(check("not linked"), Some("not linked"));
        assert_eq!(check(""), None);
    }
}
