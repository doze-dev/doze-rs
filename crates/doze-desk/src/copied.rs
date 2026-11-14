//! What a Copy says once the text is on the clipboard, in Go's words (store.ts `copy`: "copied {what} to the
//! clipboard"), and what each Copy calls its text. A toast cannot show in a shot, so the words live here, with a test.

/// The toast after a copy: "copied the address to the clipboard".
pub fn copied(what: &str) -> String {
    format!("copied {what} to the clipboard")
}

/// What the failure banner's and the crash's Copy call their text (overview.tsx `FailedBanner`, instance.tsx).
pub const WHAT_IT_SAID: &str = "what it said";
/// A command line (a service's `run`, the setup command, a fix).
pub const THE_COMMAND: &str = "the command";
/// A task's output (instance.tsx `copyOutput`).
pub const THE_OUTPUT: &str = "the output";
/// The instance head's Copy address (instance.tsx).
pub const THE_ADDRESS: &str = "the address";

/// A snippet's line: "the python line" (instance.tsx `Snippet`).
pub fn snippet(lang: &str) -> String {
    format!("the {lang} line")
}

/// A service's address on Tiles, List and its pane: "the address of api" (overview.tsx).
pub fn address_of(name: &str) -> String {
    format!("the address of {name}")
}

/// A key/value row's Copy on a card (card.tsx `Copy`): "the export line" for an Env var row, else "the value".
pub fn row(k: &str) -> &'static str {
    if k == "Env var" {
        "the export line"
    } else {
        "the value"
    }
}

/// The instance page's `y` and the dash's connect line: "the connect line of api" (instance.tsx, app.tsx).
pub fn connect_line(name: &str) -> String {
    format!("the connect line of {name}")
}

/// A pane or card key's Copy: "the URL" for URL, else "the " and the key in lower case (overview.tsx pane rows).
pub fn key(k: &str) -> String {
    if k == "URL" {
        "the URL".to_owned()
    } else {
        format!("the {}", k.to_lowercase())
    }
}

/// ⌘K's version copy (store.ts: `copy(version, "doze's version")`).
pub const VERSION: &str = "doze's version";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copy_says_gos_words() {
        assert_eq!(copied(WHAT_IT_SAID), "copied what it said to the clipboard");
        assert_eq!(copied(THE_OUTPUT), "copied the output to the clipboard");
        assert_eq!(copied(THE_ADDRESS), "copied the address to the clipboard");
        assert_eq!(copied(THE_COMMAND), "copied the command to the clipboard");
        assert_eq!(
            copied(&snippet("python")),
            "copied the python line to the clipboard"
        );
        assert_eq!(
            copied(&connect_line("api")),
            "copied the connect line of api to the clipboard"
        );
        assert_eq!(copied(VERSION), "copied doze's version to the clipboard");
    }

    #[test]
    fn a_key_is_named_in_lower_case_but_url() {
        assert_eq!(key("URL"), "the URL");
        assert_eq!(key("Address"), "the address");
        assert_eq!(key("Connect"), "the connect");
        assert_eq!(address_of("api"), "the address of api");
    }

    #[test]
    fn a_card_row_is_the_value_or_the_export_line() {
        assert_eq!(row("Env var"), "the export line");
        assert_eq!(row("URL"), "the value");
    }
}
