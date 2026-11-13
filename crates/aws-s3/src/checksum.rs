//! The checksums S3 offers (CRC32, CRC32C, CRC64NVME, SHA1, SHA256) and the MD5 of an ETag, computed as bytes stream past, and the
//! CRC combination that gives a multipart upload its whole-object checksum without reading the object again.

use std::sync::OnceLock;

use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::Sha256;

/// A checksum algorithm, as the model names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum Algo {
    Crc32,
    Crc32c,
    Crc64Nvme,
    Sha1,
    Sha256,
}

impl Algo {
    /// The algorithm named by `ChecksumAlgorithm` (`CRC32`, `CRC32C`, `CRC64NVME`, `SHA1`, `SHA256`).
    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "CRC32" => Self::Crc32,
            "CRC32C" => Self::Crc32c,
            "CRC64NVME" => Self::Crc64Nvme,
            "SHA1" => Self::Sha1,
            "SHA256" => Self::Sha256,
            _ => return None,
        })
    }

    /// The name in the model's enum and in `ChecksumAlgorithm`.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Crc32 => "CRC32",
            Self::Crc32c => "CRC32C",
            Self::Crc64Nvme => "CRC64NVME",
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
        }
    }

    /// The output member and header (`ChecksumCRC32`, `x-amz-checksum-crc32`) share this suffix.
    pub(crate) const fn member(self) -> &'static str {
        match self {
            Self::Crc32 => "ChecksumCRC32",
            Self::Crc32c => "ChecksumCRC32C",
            Self::Crc64Nvme => "ChecksumCRC64NVME",
            Self::Sha1 => "ChecksumSHA1",
            Self::Sha256 => "ChecksumSHA256",
        }
    }

    /// The request header that carries a value (and the trailer that does).
    pub(crate) const fn header(self) -> &'static str {
        match self {
            Self::Crc32 => "x-amz-checksum-crc32",
            Self::Crc32c => "x-amz-checksum-crc32c",
            Self::Crc64Nvme => "x-amz-checksum-crc64nvme",
            Self::Sha1 => "x-amz-checksum-sha1",
            Self::Sha256 => "x-amz-checksum-sha256",
        }
    }

    pub(crate) const ALL: [Self; 5] = [
        Self::Crc32,
        Self::Crc32c,
        Self::Crc64Nvme,
        Self::Sha1,
        Self::Sha256,
    ];

    const fn is_crc(self) -> bool {
        matches!(self, Self::Crc32 | Self::Crc32c | Self::Crc64Nvme)
    }
}

/// The reflected polynomial and width of a CRC.
const fn spec(a: Algo) -> (u64, u32) {
    match a {
        Algo::Crc32 => (0xEDB8_8320, 32),
        Algo::Crc32c => (0x82F6_3B78, 32),
        _ => (0x9A6C_9329_AC4B_C9B5, 64),
    }
}

fn table(a: Algo) -> &'static [u64; 256] {
    static CRC32: OnceLock<[u64; 256]> = OnceLock::new();
    static CRC32C: OnceLock<[u64; 256]> = OnceLock::new();
    static CRC64: OnceLock<[u64; 256]> = OnceLock::new();
    let cell = match a {
        Algo::Crc32 => &CRC32,
        Algo::Crc32c => &CRC32C,
        _ => &CRC64,
    };
    cell.get_or_init(|| {
        let (poly, _) = spec(a);
        let mut t = [0u64; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u64;
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ poly } else { c >> 1 };
            }
            *e = c;
        }
        t
    })
}

/// Running state of one algorithm.
enum State {
    Crc(u64),
    Sha1(Box<Sha1>),
    Sha256(Box<Sha256>),
}

/// The checksums a body is being summed with: MD5 always, and each algorithm asked for.
pub(crate) struct Sums {
    md5: Md5,
    algos: Vec<(Algo, State)>,
}

/// What a body summed to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Done {
    pub(crate) md5: [u8; 16],
    /// Each requested algorithm with its raw digest (CRCs big-endian).
    pub(crate) sums: Vec<(Algo, Vec<u8>)>,
}

impl Sums {
    pub(crate) fn new(want: &[Algo]) -> Self {
        let algos = want
            .iter()
            .map(|a| {
                let s = match a {
                    Algo::Sha1 => State::Sha1(Box::default()),
                    Algo::Sha256 => State::Sha256(Box::default()),
                    _ => State::Crc(mask(*a)),
                };
                (*a, s)
            })
            .collect();
        Self {
            md5: Md5::new(),
            algos,
        }
    }

    pub(crate) fn update(&mut self, data: &[u8]) {
        self.md5.update(data);
        for (a, s) in &mut self.algos {
            match s {
                State::Crc(c) => *c = crc_update(*a, *c, data),
                State::Sha1(h) => h.update(data),
                State::Sha256(h) => h.update(data),
            }
        }
    }

    pub(crate) fn finish(self) -> Done {
        let sums = self
            .algos
            .into_iter()
            .map(|(a, s)| {
                let raw = match s {
                    State::Crc(c) => crc_bytes(a, !c & mask(a)),
                    State::Sha1(h) => h.finalize().to_vec(),
                    State::Sha256(h) => h.finalize().to_vec(),
                };
                (a, raw)
            })
            .collect();
        Done {
            md5: self.md5.finalize().into(),
            sums,
        }
    }
}

fn crc_update(a: Algo, mut crc: u64, data: &[u8]) -> u64 {
    let t = table(a);
    for b in data {
        let ix = usize::from((crc as u8) ^ b);
        crc = t.get(ix).copied().unwrap_or(0) ^ (crc >> 8);
    }
    crc
}

/// All ones of the CRC's width: the value it starts from and the value it is inverted against.
const fn mask(a: Algo) -> u64 {
    if spec(a).1 == 32 {
        0xFFFF_FFFF
    } else {
        u64::MAX
    }
}

/// A CRC value as big-endian bytes of its width.
fn crc_bytes(a: Algo, v: u64) -> Vec<u8> {
    let bytes = v.to_be_bytes();
    let n = if spec(a).1 == 32 { 4 } else { 8 };
    bytes.get(8 - n..).unwrap_or_default().to_vec()
}

fn crc_value(a: Algo, raw: &[u8]) -> u64 {
    raw.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b))
        & if spec(a).1 == 32 {
            0xFFFF_FFFF
        } else {
            u64::MAX
        }
}

/// The checksum of two bodies joined, from the checksum of each and the length of the second (zlib's `crc32_combine`, for any
/// reflected CRC). `None` for a hash that cannot be combined.
pub(crate) fn combine(a: Algo, first: &[u8], second: &[u8], second_len: u64) -> Option<Vec<u8>> {
    if !a.is_crc() {
        return None;
    }
    let (poly, width) = spec(a);
    let w = width as usize;
    let (mut c1, c2) = (crc_value(a, first), crc_value(a, second));
    if second_len == 0 {
        return Some(crc_bytes(a, c1));
    }
    let mut odd = vec![0u64; w];
    let mut even = vec![0u64; w];
    for (n, e) in odd.iter_mut().enumerate() {
        *e = if n == 0 { poly } else { 1u64 << (n - 1) };
    }
    square(&mut even, &odd);
    square(&mut odd, &even);
    let mut len = second_len;
    loop {
        square(&mut even, &odd);
        if len & 1 == 1 {
            c1 = times(&even, c1);
        }
        len >>= 1;
        if len == 0 {
            break;
        }
        square(&mut odd, &even);
        if len & 1 == 1 {
            c1 = times(&odd, c1);
        }
        len >>= 1;
        if len == 0 {
            break;
        }
    }
    Some(crc_bytes(a, c1 ^ c2))
}

fn times(mat: &[u64], mut vec: u64) -> u64 {
    let mut sum = 0;
    for row in mat {
        if vec == 0 {
            break;
        }
        if vec & 1 == 1 {
            sum ^= row;
        }
        vec >>= 1;
    }
    sum
}

fn square(out: &mut [u64], mat: &[u64]) {
    for (o, row) in out.iter_mut().zip(mat) {
        *o = times(mat, *row);
    }
}

/// The checksum of a multipart upload made of parts: the hash of the concatenated digests, then `-` and the part count.
pub(crate) fn composite(a: Algo, digests: &[Vec<u8>]) -> Vec<u8> {
    let all: Vec<u8> = digests.iter().flatten().copied().collect();
    let mut s = Sums::new(&[a]);
    s.update(&all);
    s.finish()
        .sums
        .into_iter()
        .next()
        .map(|(_, d)| d)
        .unwrap_or_default()
}

/// `base64` of a raw digest.
pub(crate) fn b64(raw: &[u8]) -> String {
    aws_core::base64::encode(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sum(a: Algo, data: &[u8]) -> Vec<u8> {
        let mut s = Sums::new(&[a]);
        s.update(data);
        s.finish().sums.remove(0).1
    }

    #[test]
    fn the_check_values_of_each_algorithm() {
        // The "check" value of each CRC in the catalogue of parametrised CRCs is the checksum of "123456789".
        assert_eq!(
            aws_core::hex::encode(&sum(Algo::Crc32, b"123456789")),
            "cbf43926"
        );
        assert_eq!(
            aws_core::hex::encode(&sum(Algo::Crc32c, b"123456789")),
            "e3069283"
        );
        assert_eq!(
            aws_core::hex::encode(&sum(Algo::Crc64Nvme, b"123456789")),
            "ae8b14860a799888"
        );
        // The recorded S3 values for "hello".
        assert_eq!(b64(&sum(Algo::Crc32, b"hello")), "NhCmhg==");
        assert_eq!(
            aws_core::hex::encode(&sum(Algo::Sha1, b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
    }

    #[test]
    fn md5_is_the_etag_of_a_small_object() {
        let mut s = Sums::new(&[]);
        s.update(b"hello");
        assert_eq!(
            aws_core::hex::encode(&s.finish().md5),
            "5d41402abc4b2a76b9719d911017c592"
        );
    }

    #[test]
    fn combining_two_checksums_is_the_checksum_of_the_joined_bytes() {
        let (a, b) = (
            &b"hello "[..],
            &b"world, a longer second part than the first"[..],
        );
        for algo in [Algo::Crc32, Algo::Crc32c, Algo::Crc64Nvme] {
            let joined = [a, b].concat();
            let got = combine(algo, &sum(algo, a), &sum(algo, b), b.len() as u64);
            assert_eq!(got, Some(sum(algo, &joined)), "{algo:?}");
        }
        assert_eq!(combine(Algo::Sha1, &[], &[], 1), None);
    }

    #[test]
    fn the_recorded_multipart_crc64nvme_is_what_combining_the_parts_gives() {
        // The recorded upload: 5 MiB of 'a', then ten 'b' (om/L0dM0gF4= on AWS).
        let first = vec![b'a'; 5 * 1024 * 1024];
        let second = vec![b'b'; 10];
        let got = combine(
            Algo::Crc64Nvme,
            &sum(Algo::Crc64Nvme, &first),
            &sum(Algo::Crc64Nvme, &second),
            10,
        )
        .unwrap();
        assert_eq!(b64(&got), "om/L0dM0gF4=");
    }
}
