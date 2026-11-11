//! The two messages a SCRAM client sends, read with exactly the strictness of Apache Kafka 4.3.1's regular
//! expressions (`ScramMessages.ClientFirstMessage` and `ClientFinalMessage`).
//!
//! Neither pattern can match across a comma, so a message is cut at its commas and each field judged alone.
//! Every class in them is inside `\x01-\x7F`, so one byte outside that range, a NUL included, refuses the whole
//! message, wherever it is. What is accepted, and why, is in `docs/port/behaviours/security.md` (SEC-40 and on).

use crate::crypto::base64;

/// A client-first message: `n,[a=authzid],[m=reserved,]n=name,r=nonce[,ext=value]...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct First<'a> {
    /// The `a=` authorization id, as sent (still escaped), if there was one.
    pub authzid: Option<&'a str>,
    /// The user name as sent: `=2C` and `=3D` still in it. It is what the signed message holds.
    pub saslname: &'a str,
    /// The client's nonce.
    pub nonce: &'a str,
    /// The `name=value` extensions after the nonce, in the order sent.
    pub extensions: Vec<(&'a str, &'a str)>,
}

/// A client-final message: `c=binding,r=nonce[,ext=value]...,p=proof`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Final<'a> {
    /// The decoded channel-binding bytes. Kafka never compares them with the header of the first message.
    pub channel: Vec<u8>,
    /// The nonce, which must be the server's.
    pub nonce: &'a str,
    /// The decoded proof, of whatever length: the length is judged by the caller.
    pub proof: Vec<u8>,
}

impl First<'_> {
    /// Whether the client asked to log in with a delegation token (`tokenauth=true`, the last one sent, any
    /// case of `true`). There are no tokens here, so the answer to it is an unknown user.
    #[must_use]
    pub fn is_token_auth(&self) -> bool {
        let last = self
            .extensions
            .iter()
            .rev()
            .find(|&&(k, _)| k == "tokenauth");
        last.is_some_and(|&(_, v)| v.eq_ignore_ascii_case("true"))
    }

    /// The part of the message that is signed, rebuilt as Kafka rebuilds it (`clientFirstMessageBare`): no gs2
    /// header and no reserved extension, `n=name,r=nonce`, then each extension as `,name=value`. A name sent
    /// twice counts once, with its last value, as in Kafka's map.
    #[must_use]
    pub fn bare(&self) -> String {
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for &(k, v) in &self.extensions {
            match seen.iter_mut().find(|(name, _)| *name == k) {
                Some(slot) => slot.1 = v,
                None => seen.push((k, v)),
            }
        }
        let mut bare = format!("n={},r={}", self.saslname, self.nonce);
        for (k, v) in seen {
            bare = format!("{bare},{k}={v}");
        }
        bare
    }
}

// fuzz:parser scram_first
/// Reads a client-first message. Only the `n` flag is accepted (`y`, `p=...` and anything else are not), a
/// reserved `m=` extension is accepted and ignored, and `=` in the user name must be `=2C` or `=3D`.
#[must_use]
pub fn first(token: &[u8]) -> Option<First<'_>> {
    let mut fields = ascii(token)?.split(',');
    if fields.next()? != "n" {
        return None;
    }
    let authzid = match fields.next()? {
        "" => None,
        field => Some(saslname(field.strip_prefix("a=")?)?),
    };
    let mut field = fields.next()?;
    if let Some(reserved) = field.strip_prefix("m=") {
        (!reserved.is_empty()).then_some(())?;
        field = fields.next()?;
    }
    let saslname = saslname(field.strip_prefix("n=")?)?;
    let nonce = printable(fields.next()?.strip_prefix("r=")?)?;
    let extensions = fields.map(extension).collect::<Option<Vec<_>>>()?;
    Some(First {
        authzid,
        saslname,
        nonce,
        extensions,
    })
}

// fuzz:parser scram_final
/// Reads a client-final message. The channel binding and the proof are base64 (padded, but Java-lax about
/// spare bits), the nonce is printable, and extensions between the nonce and `p=` are accepted and ignored. The
/// proof is last: an attribute after it is not a message.
#[must_use]
pub fn last(token: &[u8]) -> Option<Final<'_>> {
    let mut fields: Vec<&str> = ascii(token)?.split(',').collect();
    let proof = fields.pop()?.strip_prefix("p=")?;
    let mut rest = fields.into_iter();
    let channel = rest.next()?.strip_prefix("c=")?;
    let nonce = printable(rest.next()?.strip_prefix("r=")?)?;
    for field in rest {
        extension(field)?;
    }
    Some(Final {
        channel: base64::decode_lax(channel.as_bytes()).ok()?,
        nonce,
        proof: base64::decode_lax(proof.as_bytes()).ok()?,
    })
}

/// `token` as text if every byte is `\x01-\x7F`, which is all any field of either message may hold.
fn ascii(token: &[u8]) -> Option<&str> {
    token
        .iter()
        .all(|b| (1..=0x7f).contains(b))
        .then(|| std::str::from_utf8(token).ok())?
}

/// `(?:[\x01-\x7F&&[^=,]]|=2C|=3D)+`: one or more characters, `=` only as `=2C` or `=3D`.
fn saslname(field: &str) -> Option<&str> {
    let mut bytes = field.bytes();
    let mut any = false;
    while let Some(b) = bytes.next() {
        any = true;
        if b == b'=' {
            let escape = (bytes.next()?, bytes.next()?);
            matches!(escape, (b'2', b'C') | (b'3', b'D')).then_some(())?;
        }
    }
    any.then_some(field)
}

/// `[\x21-\x7E&&[^,]]+`: visible ASCII, at least one.
fn printable(field: &str) -> Option<&str> {
    (!field.is_empty() && field.bytes().all(|b| (0x21..=0x7e).contains(&b))).then_some(field)
}

/// `[A-Za-z]+=[\x01-\x7F&&[^,]]+`: a name of letters, then a value that may itself hold `=`.
fn extension(field: &str) -> Option<(&str, &str)> {
    let (name, value) = field.split_once('=')?;
    let letters = !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphabetic());
    (letters && !value.is_empty()).then_some((name, value))
}

/// The user name a SCRAM `saslname` stands for: `=2C` is a comma and `=3D` an equals sign. A name that
/// [`first`] accepted has no other `=` in it.
#[must_use]
pub fn unescape(saslname: &str) -> String {
    saslname.replace("=2C", ",").replace("=3D", "=")
}

#[cfg(test)]
mod tests;
