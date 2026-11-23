//! What a request's `User-Agent` says about the client that sent it: the SDK (or the AWS CLI) and its version, as a short label
//! (`boto3 1.34.69`, `JS v3 3.540.0`, `AWS CLI 2.15.30`). Home says from it which clients have connected.
//!
//! Every AWS SDK opens its `User-Agent` with `name/version` (`aws-sdk-go-v2/1.26.0 ua/2.0 os/macos ...`), and the AWS CLI with
//! `aws-cli/2.15.30`. Only that first token is read: the rest (OS, language, feature flags) is not kept. A client that is not an
//! AWS SDK keeps its own first token (`curl/8.4.0`), cut to printable ASCII. The label is at most [`MOST`] bytes, so what the
//! Live record holds for it is bounded whatever was sent.

/// The most bytes of a label.
pub const MOST: usize = 48;

/// The most bytes of the header looked at: the first token of any SDK's is far shorter.
const LOOK: usize = 256;

/// The names AWS's clients give themselves, and the words a person knows them by.
const KNOWN: &[(&str, &str)] = &[
    ("aws-cli", "AWS CLI"),
    ("boto3", "boto3"),
    ("botocore", "botocore"),
    ("aws-sdk-js", "JS v3"),
    ("aws-sdk-nodejs", "JS v2"),
    ("aws-sdk-go-v2", "Go v2"),
    ("aws-sdk-go", "Go v1"),
    ("aws-sdk-rust", "Rust"),
    ("aws-sdk-php", "PHP"),
    ("aws-sdk-cpp", "C++"),
    ("aws-sdk-kotlin", "Kotlin"),
    ("aws-sdk-swift", "Swift"),
];

/// The client a `User-Agent` names, as a short label; empty when it names none (no header, or nothing printable).
// fuzz:parser user_agent
#[must_use]
pub fn label(user_agent: &str) -> String {
    // Cut on a character boundary at or before LOOK.
    let mut end = LOOK.min(user_agent.len());
    while !user_agent.is_char_boundary(end) {
        end -= 1;
    }
    let head = user_agent.get(..end).unwrap_or_default();
    let Some(first) = head.split_ascii_whitespace().next() else {
        return String::new();
    };
    let (name, version) = first.split_once('/').unwrap_or((first, ""));
    let version: String = version
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
        .collect();
    let lower = name.to_ascii_lowercase();
    let known = KNOWN
        .iter()
        .find(|(n, _)| *n == lower)
        .map(|(_, words)| (*words).to_owned())
        .or_else(|| family(&lower, &version));
    let named = known.unwrap_or_else(|| {
        name.chars()
            .filter(|c| c.is_ascii_graphic())
            .collect::<String>()
    });
    if named.is_empty() {
        return String::new();
    }
    let mut out = if version.is_empty() {
        named
    } else {
        format!("{named} {version}")
    };
    if out.len() > MOST {
        // Only ASCII is left in it, so any index is a boundary.
        out.truncate(MOST);
    }
    out
}

/// The SDKs whose name carries a variant (`aws-sdk-dotnet-coreclr`, `aws-sdk-ruby3`) or whose major version is the SDK's
/// generation (Java 1.x and 2.x share a name).
fn family(lower: &str, version: &str) -> Option<String> {
    if lower == "aws-sdk-java" {
        return Some(
            if version.starts_with('1') {
                "Java v1"
            } else {
                "Java v2"
            }
            .to_owned(),
        );
    }
    if lower.starts_with("aws-sdk-dotnet") {
        return Some(".NET".to_owned());
    }
    if lower.starts_with("aws-sdk-ruby") {
        return Some("Ruby".to_owned());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{MOST, label};

    #[test]
    fn each_sdk_is_named_as_a_person_knows_it_with_its_version() {
        let seen = [
            (
                "aws-cli/2.15.30 md/awscrt#0.19.19 ua/2.0 os/macos#23.4.0 md/arch#arm64 lang/python#3.11.8",
                "AWS CLI 2.15.30",
            ),
            (
                "Boto3/1.34.69 md/Botocore#1.34.69 ua/2.0 os/macos#23.4.0 lang/python#3.12.2",
                "boto3 1.34.69",
            ),
            (
                "aws-sdk-js/3.540.0 ua/2.1 os/darwin#23.4.0 lang/js md/nodejs#20.11.0 api/sqs#3.540.0",
                "JS v3 3.540.0",
            ),
            (
                "aws-sdk-go-v2/1.26.0 ua/2.0 os/macos lang/go#1.22.1",
                "Go v2 1.26.0",
            ),
            (
                "aws-sdk-java/2.25.16 Mac_OS_X/14.4 OpenJDK_64-Bit_Server_VM/21.0.2+13",
                "Java v2 2.25.16",
            ),
            ("aws-sdk-java/1.12.600 Linux/6.1", "Java v1 1.12.600"),
            ("aws-sdk-rust/1.1.8 os/macos lang/rust/1.76.0", "Rust 1.1.8"),
            ("aws-sdk-go/1.50.0 (go1.21; darwin; arm64)", "Go v1 1.50.0"),
            ("aws-sdk-dotnet-coreclr/3.7.300 ua/2.0", ".NET 3.7.300"),
            ("curl/8.4.0", "curl 8.4.0"),
        ];
        for (sent, said) in seen {
            assert_eq!(label(sent), said, "{sent}");
        }
    }

    #[test]
    fn nothing_printable_says_nothing_and_a_long_name_is_cut() {
        assert_eq!(label(""), "");
        assert_eq!(label("   "), "");
        assert_eq!(label("\u{e9}\u{e9}/1"), "");
        let long = format!("{}/1.0", "x".repeat(300));
        assert_eq!(label(&long).len(), MOST);
        // A version stops at the first character no version has.
        assert_eq!(label("thing/1.2;evil<script>"), "thing 1.2");
        // Multi-byte text past the part looked at does not split a character.
        let wide = format!("{}/1 {}", "a".repeat(10), "\u{1f600}".repeat(100));
        assert_eq!(label(&wide), "aaaaaaaaaa 1");
    }
}
