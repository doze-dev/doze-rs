//! The screen of a folder with no doze.hcl yet: three ways to start, in the order they are offered.

/// One way to start: its title and a line on what it does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// The title.
    pub title: &'static str,
    /// What it does.
    pub what: &'static str,
}

/// The three ways to start, in order: from what the project says, one named thing, or an empty file.
pub const EMPTY_CHOICES: [Choice; 3] = [
    Choice {
        title: "Set it up from this repo",
        what: "reads go.mod, package.json, compose files and writes a doze.hcl",
    },
    Choice {
        title: "Add one thing",
        what: "name a module or a service: postgres, valkey, mongo, …",
    },
    Choice {
        title: "Start with an empty file",
        what: "writes a short doze.hcl, with comments, to fill in",
    },
];

/// What the first choice says instead when the project has nothing doze recognises.
pub const EMPTY_NOTHING_FOUND: &str = "nothing here it recognises; add one thing instead";

/// What "start with an empty file" writes: the four nouns and what each is for, with nothing declared. Both the
/// CLI and the app write this one.
pub const STARTER_FILE: &str = r#"# doze.hcl: what this project runs, and what it needs.
#
#   app     "name" { run = "..." }          your code
#   module  postgres "db" { }              from the doze registry
#   service "name" { run = "..." }         anything else
#   task    "name" { run = "..." }         runs once, then gets out of the way
#
# doze add postgres   adds one for you.   doze up   starts it all.

stack = "my-project"
"#;
