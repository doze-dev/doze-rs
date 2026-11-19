//! Grok: `%{PATTERN:field[:type]}` references to named patterns, expanded into one regular expression. The pattern table holds the
//! common Logstash patterns (memory: AWS's "Supported grok patterns" page is not in the model; the definitions are Logstash's,
//! rewritten without look-around, which the regular expression engine here does not have).

use regex_lite::{Regex, RegexBuilder};

/// How deep pattern references may nest.
const MAX_DEPTH: usize = 12;
/// The most compiled size a grok expression may reach.
const MAX_REGEX_BYTES: usize = 1 << 20;
/// The longest expansion of one grok expression.
const MAX_EXPANDED: usize = 64 * 1024;

const PATTERNS: [(&str, &str); 60] = [
    ("USERNAME", r"[a-zA-Z0-9._-]+"),
    ("USER", "%{USERNAME}"),
    ("EMAILLOCALPART", r"[a-zA-Z0-9._%+-]+"),
    ("EMAILADDRESS", "%{EMAILLOCALPART}@%{HOSTNAME}"),
    ("INT", r"[+-]?[0-9]+"),
    ("BASE10NUM", r"[+-]?(?:[0-9]+(?:\.[0-9]+)?|\.[0-9]+)"),
    ("NUMBER", "%{BASE10NUM}"),
    ("BASE16NUM", r"[+-]?(?:0[xX])?[0-9A-Fa-f]+"),
    ("POSINT", r"\b[1-9][0-9]*\b"),
    ("NONNEGINT", r"\b[0-9]+\b"),
    ("WORD", r"\b\w+\b"),
    ("NOTSPACE", r"\S+"),
    ("SPACE", r"\s*"),
    ("DATA", r".*?"),
    ("GREEDYDATA", r".*"),
    (
        "QUOTEDSTRING",
        r#""(?:\\.|[^\\"])*"|'(?:\\.|[^\\'])*'|`(?:\\.|[^\\`])*`"#,
    ),
    ("QS", "%{QUOTEDSTRING}"),
    (
        "UUID",
        r"[A-Fa-f0-9]{8}-(?:[A-Fa-f0-9]{4}-){3}[A-Fa-f0-9]{12}",
    ),
    (
        "MAC",
        r"(?:[A-Fa-f0-9]{2}[:-]){5}[A-Fa-f0-9]{2}|(?:[A-Fa-f0-9]{4}\.){2}[A-Fa-f0-9]{4}",
    ),
    (
        "IPV6",
        r"(?:[0-9A-Fa-f]{0,4}:){2,7}[0-9A-Fa-f]{0,4}(?:%[0-9A-Za-z]+)?",
    ),
    (
        "IPV4",
        r"(?:(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])\.){3}(?:25[0-5]|2[0-4][0-9]|1[0-9]{2}|[1-9]?[0-9])",
    ),
    ("IP", "%{IPV6}|%{IPV4}"),
    (
        "HOSTNAME",
        r"\b[0-9A-Za-z][0-9A-Za-z-]{0,62}(?:\.[0-9A-Za-z][0-9A-Za-z-]{0,62})*\.?",
    ),
    ("IPORHOST", "%{IP}|%{HOSTNAME}"),
    ("HOSTPORT", "%{IPORHOST}:%{POSINT}"),
    ("URIPROTO", r"[A-Za-z][A-Za-z0-9+\-.]*"),
    ("URIHOST", "%{IPORHOST}(?::%{POSINT})?"),
    ("URIPATH", r"(?:/[A-Za-z0-9$.+!*'(){},~:;=@#%&_\-]*)+"),
    ("URIPARAM", r"\?[A-Za-z0-9$.+!*'|(){},~@#%&/=:;_?\-\[\]<>]*"),
    ("URIPATHPARAM", "%{URIPATH}(?:%{URIPARAM})?"),
    (
        "URI",
        "%{URIPROTO}://(?:%{USER}(?::[^@]*)?@)?(?:%{URIHOST})?(?:%{URIPATHPARAM})?",
    ),
    (
        "MONTH",
        r"\b(?:[Jj]an(?:uary)?|[Ff]eb(?:ruary)?|[Mm]ar(?:ch)?|[Aa]pr(?:il)?|[Mm]ay|[Jj]un(?:e)?|[Jj]ul(?:y)?|[Aa]ug(?:ust)?|[Ss]ep(?:tember)?|[Oo]ct(?:ober)?|[Nn]ov(?:ember)?|[Dd]ec(?:ember)?)\b",
    ),
    ("MONTHNUM", r"0?[1-9]|1[0-2]"),
    ("MONTHNUM2", r"0[1-9]|1[0-2]"),
    ("MONTHDAY", r"0[1-9]|[12][0-9]|3[01]|[1-9]"),
    (
        "DAY",
        r"Mon(?:day)?|Tue(?:sday)?|Wed(?:nesday)?|Thu(?:rsday)?|Fri(?:day)?|Sat(?:urday)?|Sun(?:day)?",
    ),
    ("YEAR", r"(?:\d\d){1,2}"),
    ("HOUR", r"2[0123]|[01]?[0-9]"),
    ("MINUTE", r"[0-5][0-9]"),
    ("SECOND", r"(?:[0-5]?[0-9]|60)(?:[:.,][0-9]+)?"),
    ("TIME", "%{HOUR}:%{MINUTE}(?::%{SECOND})?"),
    ("DATE_US", "%{MONTHNUM}[/-]%{MONTHDAY}[/-]%{YEAR}"),
    ("DATE_EU", "%{MONTHDAY}[./-]%{MONTHNUM}[./-]%{YEAR}"),
    ("DATE", "%{DATE_US}|%{DATE_EU}"),
    ("DATESTAMP", "%{DATE}[- ]%{TIME}"),
    ("ISO8601_TIMEZONE", "Z|[+-]%{HOUR}(?::?%{MINUTE})"),
    ("ISO8601_SECOND", "%{SECOND}"),
    (
        "TIMESTAMP_ISO8601",
        "%{YEAR}-%{MONTHNUM}-%{MONTHDAY}[T ]%{HOUR}:?%{MINUTE}(?::?%{SECOND})?(?:%{ISO8601_TIMEZONE})?",
    ),
    ("HTTPDATE", "%{MONTHDAY}/%{MONTH}/%{YEAR}:%{TIME} %{INT}"),
    ("SYSLOGTIMESTAMP", "%{MONTH} +%{MONTHDAY} %{TIME}"),
    ("PROG", r"[\x21-\x5a\x5c\x5e-\x7e]+"),
    ("SYSLOGPROG", r"%{PROG:program}(?:\[%{POSINT:pid}\])?"),
    ("SYSLOGHOST", "%{IPORHOST}"),
    (
        "SYSLOGBASE",
        "%{SYSLOGTIMESTAMP:timestamp} %{SYSLOGHOST:logsource} %{SYSLOGPROG}:",
    ),
    (
        "LOGLEVEL",
        r"[Aa]lert|ALERT|[Tt]race|TRACE|[Dd]ebug|DEBUG|[Nn]otice|NOTICE|[Ii]nfo(?:rmation)?|INFO(?:RMATION)?|[Ww]arn(?:ing)?|WARN(?:ING)?|[Ee]rr(?:or)?|ERR(?:OR)?|[Cc]rit(?:ical)?|CRIT(?:ICAL)?|[Ff]atal|FATAL|[Ss]evere|SEVERE|EMERG(?:ENCY)?|[Ee]merg(?:ency)?",
    ),
    ("HTTPDUSER", "%{EMAILADDRESS}|%{USER}"),
    (
        "COMMONAPACHELOG",
        r#"%{IPORHOST:clientip} %{HTTPDUSER:ident} %{USER:auth} \[%{HTTPDATE:timestamp}\] "(?:%{WORD:verb} %{NOTSPACE:request}(?: HTTP/%{NUMBER:httpversion})?|%{DATA:rawrequest})" %{NUMBER:response} (?:%{NUMBER:bytes}|-)"#,
    ),
    (
        "COMBINEDAPACHELOG",
        "%{COMMONAPACHELOG} %{QS:referrer} %{QS:agent}",
    ),
    ("NGUSERNAME", r"[a-zA-Z.@\-+_%]+"),
    ("NGUSER", "%{NGUSERNAME}"),
];

/// What a captured value becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cast {
    Text,
    Int,
    Float,
}

/// A compiled grok expression: the regular expression, and for each of its capture groups the field it fills.
#[derive(Clone, Debug)]
pub(crate) struct Grok {
    pub(crate) re: Regex,
    pub(crate) fields: Vec<(String, String, Cast)>,
}

/// Compiles a grok expression; the reason when it does not compile.
// fuzz:parser fuzz_transformer
pub(crate) fn compile(source: &str) -> Result<Grok, String> {
    let mut fields = Vec::new();
    let expanded = expand(source, 0, &mut fields)?;
    let re = RegexBuilder::new(&expanded)
        .size_limit(MAX_REGEX_BYTES)
        .build()
        .map_err(|_| "the grok pattern is not a valid pattern".to_owned())?;
    Ok(Grok { re, fields })
}

fn expand(
    text: &str,
    depth: usize,
    fields: &mut Vec<(String, String, Cast)>,
) -> Result<String, String> {
    if depth > MAX_DEPTH {
        return Err("grok patterns nest too deeply".into());
    }
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("%{") {
        out.push_str(rest.get(..start).unwrap_or_default());
        let after = rest.get(start + 2..).unwrap_or_default();
        let end = after
            .find('}')
            .ok_or("an unclosed %{ in the grok pattern")?;
        let inner = after.get(..end).unwrap_or_default();
        out.push_str(&reference(inner, depth, fields)?);
        if out.len() > MAX_EXPANDED {
            return Err("the grok pattern expands too far".into());
        }
        rest = after.get(end + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(out)
}

/// `NAME`, `NAME:field` or `NAME:field:type`, expanded.
fn reference(
    inner: &str,
    depth: usize,
    fields: &mut Vec<(String, String, Cast)>,
) -> Result<String, String> {
    let mut parts = inner.splitn(3, ':');
    let name = parts.next().unwrap_or_default();
    let field = parts.next().filter(|f| !f.is_empty());
    let cast = match parts.next() {
        None => Cast::Text,
        Some("int") => Cast::Int,
        Some("float") => Cast::Float,
        // (memory)
        Some(other) => {
            return Err(format!(
                "the grok type '{other}' is not supported; use int or float"
            ));
        }
    };
    let def = PATTERNS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, d)| *d)
        .ok_or_else(|| format!("the grok pattern {name} is not known"))?;
    let Some(field) = field else {
        return Ok(format!("(?:{})", expand(def, depth + 1, fields)?));
    };
    let group = format!("dozeg{}", fields.len());
    fields.push((group.clone(), field.to_owned(), cast));
    Ok(format!("(?P<{group}>{})", expand(def, depth + 1, fields)?))
}
