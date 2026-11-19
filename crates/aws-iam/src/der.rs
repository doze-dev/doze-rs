//! Just enough DER to walk an X.509 certificate: one element at a time, its tag checked, its length bounded by the bytes there are.

/// One DER element.
#[derive(Debug)]
pub(crate) struct Tlv<'a> {
    /// The element with its tag and length.
    pub(crate) whole: &'a [u8],
    pub(crate) contents: &'a [u8],
}

/// The element `bytes` starts with, which must carry the tag `tag`, and what follows it.
pub(crate) fn tlv(bytes: &[u8], tag: u8) -> Option<(Tlv<'_>, &[u8])> {
    if *bytes.first()? != tag {
        return None;
    }
    let first = *bytes.get(1)?;
    let (len, header) = if first < 0x80 {
        (usize::from(first), 2)
    } else {
        let n = usize::from(first & 0x7f);
        if n == 0 || n > 4 {
            return None;
        }
        let len = bytes
            .get(2..2 + n)?
            .iter()
            .fold(0usize, |acc, b| (acc << 8) | usize::from(*b));
        (len, 2 + n)
    };
    let end = header.checked_add(len)?;
    Some((
        Tlv {
            whole: bytes.get(..end)?,
            contents: bytes.get(header..end)?,
        },
        bytes.get(end..)?,
    ))
}
