//! A stored (uncompressed) zip archive, for `Code.ZipFile`: CloudFormation turns inline source into a one-file package (plus its
//! `cfn-response` module) before Lambda sees it.

/// CRC-32 (IEEE), bit by bit: archives here are a few kilobytes.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn u16le(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn u32le(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// An archive of `files` (name and bytes), each readable by all.
pub(crate) fn stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in files {
        let offset = u32::try_from(out.len()).unwrap_or(u32::MAX);
        let crc = crc32(data);
        let size = u32::try_from(data.len()).unwrap_or(u32::MAX);
        let name_len = u16::try_from(name.len()).unwrap_or(u16::MAX);
        u32le(&mut out, 0x0403_4b50);
        for v in [20u16, 0, 0, 0, 0x21] {
            u16le(&mut out, v);
        }
        u32le(&mut out, crc);
        u32le(&mut out, size);
        u32le(&mut out, size);
        u16le(&mut out, name_len);
        u16le(&mut out, 0);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        u32le(&mut central, 0x0201_4b50);
        for v in [0x031e_u16, 20, 0, 0, 0, 0x21] {
            u16le(&mut central, v);
        }
        u32le(&mut central, crc);
        u32le(&mut central, size);
        u32le(&mut central, size);
        u16le(&mut central, name_len);
        for v in [0u16, 0, 0, 0] {
            u16le(&mut central, v);
        }
        // Unix mode 0644 in the high half of the external attributes.
        u32le(&mut central, 0o100_644 << 16);
        u32le(&mut central, offset);
        central.extend_from_slice(name.as_bytes());
    }
    let start = u32::try_from(out.len()).unwrap_or(u32::MAX);
    let count = u16::try_from(files.len()).unwrap_or(u16::MAX);
    let len = u32::try_from(central.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&central);
    u32le(&mut out, 0x0605_4b50);
    for v in [0u16, 0, count, count] {
        u16le(&mut out, v);
    }
    u32le(&mut out, len);
    u32le(&mut out, start);
    u16le(&mut out, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::crc32;

    #[test]
    fn crc_of_a_known_text() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
