//! DNS on the wire, as much as the zone needs: one question in, one A answer (or an empty NOERROR, or NXDOMAIN) out,
//! and a TTL cap on relayed answers. Written by hand as Go's is (no dependency; `simple-dns` is the fallback if more
//! record types are ever needed), and fuzzed.
//!
//! The awkward part is compressed question names: macOS's mDNSResponder packs the A and AAAA questions into one
//! packet with the second name compressed to a pointer at the first, and a parser that rejects pointers silently
//! drops real queries.

use std::net::Ipv4Addr;

pub(crate) const TYPE_A: u16 = 1;
/// How long an answer may be cached: services come and go, and a stale answer is a connection to nothing.
const ANSWER_TTL: u32 = 10;
/// A relayed answer's cap: the moment a stack claims the domain the answer must change, and an hour-long upstream TTL
/// would keep the browser on the internet.
pub(crate) const RELAY_TTL: u32 = 10;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

/// The lowercase dotted name at `off` (following compression pointers) and the offset just past its encoding at the
/// original position: following a pointer must not move the caller past the pointer itself.
// fuzz:parser fuzz_wire
pub(crate) fn parse_name(q: &[u8], off: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let (mut pos, mut end, mut jumps) = (off, None, 0);
    loop {
        let l = usize::from(*q.get(pos)?);
        if l == 0 {
            return Some((labels.join("."), end.unwrap_or(pos + 1)));
        }
        if l & 0xC0 == 0xC0 {
            let ptr = u16_at(q, pos)?;
            end.get_or_insert(pos + 2);
            jumps += 1;
            if jumps > 8 {
                return None; // a pointer loop
            }
            pos = usize::from(ptr & 0x3FFF);
            continue;
        }
        let label = q.get(pos + 1..pos + 1 + l)?;
        labels.push(String::from_utf8_lossy(label).to_ascii_lowercase());
        pos += 1 + l;
    }
}

/// The response to one query and whether it should be forwarded: an A record for a name this machine serves, an empty
/// NOERROR for its other types (so a client falls back to the A answer instead of retrying), NXDOMAIN for the rest
/// of the zone. For a name outside the zone nobody serves, `forward` is true and the NXDOMAIN is the fallback if no
/// upstream answers. `None` for input that cannot be read, or anything but one question.
///
/// What this machine answers for (the zone, and a domain a stack registered) is answered with authority (`aa`), and
/// an answer that says no (NXDOMAIN, or NOERROR with nothing in it) carries the zone's SOA, whose TTL and MINIMUM
/// bound how long it may be cached (`NEGATIVE_TTL`, RFC 2308 §5). Go sent neither: a negative answer said nothing
/// of how long it held, and a cache was free to keep it as long as it liked, so a name asked for in the moment before
/// its stack came up could stay "not found" after it had (ENOTFOUND for `kafka.larder.doze` after a `doze down` and
/// `up`, 2026-11-25).
pub fn answer(q: &[u8], resolve: &dyn Fn(&str) -> Option<Ipv4Addr>) -> Option<(Vec<u8>, bool)> {
    if q.len() < 12 || u16_at(q, 4)? != 1 {
        return None;
    }
    let (name, qend) = parse_name(q, 12)?;
    let qtype = u16_at(q, qend)?;
    let question_end = qend + 4;
    let question = q.get(12..question_end)?;
    let ip = resolve(&name);
    let in_zone = crate::in_zone(&name);
    let ours = ip.is_some() || in_zone;
    let forward = !ours;
    let answers = u16::from(ip.is_some() && qtype == TYPE_A);
    let rcode: u16 = if ip.is_none() { 3 } else { 0 };
    let soa = if ours && answers == 0 {
        soa_owner(&name, in_zone)
    } else {
        None
    };
    let aa = if ours { 0x0400 } else { 0 };
    let flags = 0x8080 | aa | (u16_at(q, 2)? & 0x0100) | rcode; // QR, AA, RA, the client's RD
    let mut resp = Vec::with_capacity(question_end + 96);
    resp.extend_from_slice(q.get(..2)?);
    for v in [flags, 1, answers, u16::from(soa.is_some()), 0] {
        resp.extend_from_slice(&v.to_be_bytes());
    }
    resp.extend_from_slice(question);
    if let Some(ip) = ip.filter(|_| answers == 1) {
        resp.extend_from_slice(&[0xC0, 0x0C]); // the name: a pointer to the question
        resp.extend_from_slice(&TYPE_A.to_be_bytes());
        resp.extend_from_slice(&1u16.to_be_bytes()); // IN
        resp.extend_from_slice(&ANSWER_TTL.to_be_bytes());
        resp.extend_from_slice(&4u16.to_be_bytes());
        resp.extend_from_slice(&ip.octets());
    }
    if let Some(owner) = soa {
        let before = resp.len();
        // A plain UDP answer stops at 512 bytes: one too long to carry the SOA goes without it (a long registered
        // domain asked for under a long name), as Go's always did.
        if put_soa(&mut resp, &owner).is_none() || resp.len() > 512 {
            resp.truncate(before);
            resp.get_mut(8..10)?.copy_from_slice(&[0, 0]);
        }
    }
    Some((resp, forward))
}

pub(crate) const TYPE_SOA: u16 = 6;
/// How long a "no such name" or "no such record" from the zone may be cached: a stack comes up in about this long,
/// and the answer must change the moment it does. RFC 2308 has a cache take the lesser of the SOA's TTL and its
/// MINIMUM; both are this.
pub(crate) const NEGATIVE_TTL: u32 = 1;

/// The labels of the SOA a negative answer for `name` carries: the zone's apex for a name in it, else the name itself
/// (a registered domain, or a name under one: the resolver is not told which domain it was found under, and the
/// owner only has to be something this machine answers for). `None` when the name cannot be written back (a label
/// longer than DNS allows).
fn soa_owner(name: &str, in_zone: bool) -> Option<Vec<u8>> {
    let owner = if in_zone { crate::SUFFIX } else { name };
    let mut out = Vec::with_capacity(owner.len() + 2);
    for label in owner.split('.').filter(|l| !l.is_empty()) {
        let len = u8::try_from(label.len()).ok().filter(|l| *l <= 63)?;
        out.push(len);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Some(out)
}

/// Appends the SOA record for `owner` (wire labels): `ns.<owner>`, `hostmaster.<owner>`, serial 1, and
/// [`NEGATIVE_TTL`] as both its TTL and MINIMUM. The owner is written out (the question may be a pointer into
/// something this response does not carry) and the two names in it point back at it.
fn put_soa(resp: &mut Vec<u8>, owner: &[u8]) -> Option<()> {
    let at = u16::try_from(resp.len()).ok().filter(|a| *a < 0x3FFF)?;
    let back = (0xC000 | at).to_be_bytes();
    resp.extend_from_slice(owner);
    resp.extend_from_slice(&TYPE_SOA.to_be_bytes());
    resp.extend_from_slice(&1u16.to_be_bytes()); // IN
    resp.extend_from_slice(&NEGATIVE_TTL.to_be_bytes());
    let mut rdata = Vec::with_capacity(48);
    rdata.extend_from_slice(b"\x02ns");
    rdata.extend_from_slice(&back);
    rdata.extend_from_slice(b"\x0ahostmaster");
    rdata.extend_from_slice(&back);
    // Serial, refresh, retry, expire: nothing transfers this zone, so they are only well-formed; then MINIMUM.
    for v in [1u32, 3600, 600, 86_400, NEGATIVE_TTL] {
        rdata.extend_from_slice(&v.to_be_bytes());
    }
    resp.extend_from_slice(&u16::try_from(rdata.len()).ok()?.to_be_bytes());
    resp.extend_from_slice(&rdata);
    Some(())
}

/// The offset after a name at `off`, compressed or not.
fn skip_name(msg: &[u8], mut off: usize) -> Option<usize> {
    loop {
        let l = usize::from(*msg.get(off)?);
        if l == 0 {
            return Some(off + 1);
        }
        if l & 0xC0 == 0xC0 {
            return (off + 2 <= msg.len()).then_some(off + 2);
        }
        off += 1 + l;
    }
}

/// Every record's TTL in a DNS message capped at `max`, everything else as it was. A message it cannot walk is
/// returned untouched.
// fuzz:parser fuzz_wire
pub fn cap_ttl(msg: &[u8], max: u32) -> Vec<u8> {
    walk_ttls(msg, max).unwrap_or_else(|| msg.to_vec())
}

fn walk_ttls(msg: &[u8], max: u32) -> Option<Vec<u8>> {
    let qd = u16_at(msg, 4)?;
    let rrs = u32::from(u16_at(msg, 6)?) + u32::from(u16_at(msg, 8)?) + u32::from(u16_at(msg, 10)?);
    let mut off = 12;
    for _ in 0..qd {
        off = skip_name(msg, off)? + 4;
    }
    let mut out = msg.to_vec();
    for _ in 0..rrs {
        let n = skip_name(msg, off)?;
        let ttl_at = n + 4;
        let ttl = u32::from_be_bytes(msg.get(ttl_at..ttl_at + 4)?.try_into().ok()?);
        if ttl > max {
            out.get_mut(ttl_at..ttl_at + 4)?
                .copy_from_slice(&max.to_be_bytes());
        }
        off = n + 10 + usize::from(u16_at(msg, n + 8)?);
        if off > msg.len() {
            return None;
        }
    }
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    pub(crate) const TYPE_AAAA: u16 = 28;

    /// A minimal query for `host` (Go: `query`).
    pub(crate) fn query(host: &str, qtype: u16) -> Vec<u8> {
        let mut q = vec![0x12, 0x34, 0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in host.split('.').filter(|l| !l.is_empty()) {
            q.push(u8::try_from(l.len()).unwrap());
            q.extend_from_slice(l.as_bytes());
        }
        q.push(0);
        q.extend_from_slice(&qtype.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q
    }

    pub(crate) fn rcode(resp: &[u8]) -> u16 {
        u16_at(resp, 2).unwrap() & 0x000F
    }

    pub(crate) fn ancount(resp: &[u8]) -> u16 {
        u16_at(resp, 6).unwrap()
    }

    fn resolve(host: &str) -> Option<Ipv4Addr> {
        (host == "aws.doze").then(|| Ipv4Addr::new(127, 0, 0, 2))
    }

    #[test]
    fn answers_as_go_does() {
        // Go: TestAnswer.
        let (resp, _) = answer(&query("aws.doze", TYPE_A), &resolve).unwrap();
        assert_eq!((rcode(&resp), ancount(&resp)), (0, 1));
        assert_eq!(&resp[resp.len() - 4..], &[127, 0, 0, 2]);
        assert_eq!(&resp[..2], &[0x12, 0x34], "the query id is echoed");
        let (resp, _) = answer(&query("aws.doze", TYPE_AAAA), &resolve).unwrap();
        assert_eq!(
            (rcode(&resp), ancount(&resp)),
            (0, 0),
            "AAAA: an empty NOERROR, not NXDOMAIN"
        );
        let (resp, fwd) = answer(&query("nope.doze", TYPE_A), &resolve).unwrap();
        assert_eq!(
            (rcode(&resp), fwd),
            (3, false),
            "the zone is never forwarded"
        );
        let (resp, fwd) = answer(&query("example.com", TYPE_A), &resolve).unwrap();
        assert_eq!((rcode(&resp), fwd), (3, true));
        assert!(
            answer(&[1, 2, 3], &resolve).is_none(),
            "truncated input is dropped"
        );
    }

    #[test]
    fn a_compressed_question_name_is_answered() {
        // Go: TestAnswer/compressed QNAME (macOS's mDNSResponder).
        let base = query("aws.doze", TYPE_A);
        let target = 12 + 2 + 4;
        let mut q = base[..12].to_vec();
        q.extend_from_slice(&[0xC0, u8::try_from(target).unwrap()]);
        q.extend_from_slice(&TYPE_A.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        assert_eq!(q.len(), target);
        q.extend_from_slice(&base[12..]);
        let (resp, _) = answer(&q, &resolve).unwrap();
        assert_eq!(ancount(&resp), 1);
    }

    pub(crate) fn authoritative(resp: &[u8]) -> bool {
        u16_at(resp, 2).unwrap() & 0x0400 != 0
    }

    /// The SOA in a response's authority section: its owner, TTL, MNAME and MINIMUM. `None` when there is none.
    pub(crate) fn soa(resp: &[u8]) -> Option<(String, u32, String, u32)> {
        if u16_at(resp, 8).unwrap() != 1 {
            return None;
        }
        let mut off = skip_name(resp, 12).unwrap() + 4;
        for _ in 0..ancount(resp) {
            off = skip_name(resp, off).unwrap();
            off += 10 + usize::from(u16_at(resp, off + 8).unwrap());
        }
        let (owner, n) = parse_name(resp, off).unwrap();
        assert_eq!(u16_at(resp, n).unwrap(), TYPE_SOA);
        let ttl = u32::from_be_bytes(resp[n + 4..n + 8].try_into().unwrap());
        let rdlen = usize::from(u16_at(resp, n + 8).unwrap());
        let rdata = n + 10;
        assert_eq!(rdata + rdlen, resp.len(), "the SOA is the last thing");
        let (mname, m) = parse_name(resp, rdata).unwrap();
        let (_rname, r) = parse_name(resp, m).unwrap();
        let minimum = u32::from_be_bytes(resp[r + 16..r + 20].try_into().unwrap());
        Some((owner, ttl, mname, minimum))
    }

    #[test]
    fn a_negative_answer_from_the_zone_says_how_long_it_holds() {
        // RFC 2308: NXDOMAIN and NODATA carry the zone's SOA, and its TTL and MINIMUM are the negative TTL.
        let zone_soa = Some(("doze".into(), NEGATIVE_TTL, "ns.doze".into(), NEGATIVE_TTL));
        let (resp, _) = answer(&query("nope.shop.doze", TYPE_A), &resolve).unwrap();
        assert_eq!(rcode(&resp), 3);
        assert!(authoritative(&resp), "the zone is answered with authority");
        assert_eq!(soa(&resp), zone_soa, "NXDOMAIN");
        let (resp, _) = answer(&query("aws.doze", TYPE_AAAA), &resolve).unwrap();
        assert_eq!((rcode(&resp), ancount(&resp)), (0, 0));
        assert!(authoritative(&resp));
        assert_eq!(soa(&resp), zone_soa, "NODATA");
        // A positive answer is authoritative and carries no SOA.
        let (resp, _) = answer(&query("aws.doze", TYPE_A), &resolve).unwrap();
        assert!(authoritative(&resp));
        assert_eq!((ancount(&resp), soa(&resp)), (1, None));
        assert_eq!(&resp[resp.len() - 4..], &[127, 0, 0, 2]);
        // A name outside the zone that nobody here serves is not ours to deny: the fallback claims nothing.
        let (resp, fwd) = answer(&query("example.com", TYPE_A), &resolve).unwrap();
        assert!(fwd && !authoritative(&resp));
        assert_eq!(soa(&resp), None);
        // A registered domain's own NODATA names the domain as the SOA's owner.
        let dev = |h: &str| (h == "dev.acme.com").then(|| Ipv4Addr::new(198, 19, 3, 7));
        let (resp, fwd) = answer(&query("dev.acme.com", TYPE_AAAA), &dev).unwrap();
        assert!(!fwd && authoritative(&resp));
        assert_eq!(
            soa(&resp),
            Some((
                "dev.acme.com".into(),
                NEGATIVE_TTL,
                "ns.dev.acme.com".into(),
                NEGATIVE_TTL
            ))
        );
    }

    #[test]
    fn a_negative_answer_to_a_compressed_question_is_well_formed() {
        // mDNSResponder's pointer in the question points past it; the SOA must not lean on it.
        let base = query("gone.doze", TYPE_A);
        let target = 12 + 2 + 4;
        let mut q = base[..12].to_vec();
        q.extend_from_slice(&[0xC0, u8::try_from(target).unwrap()]);
        q.extend_from_slice(&TYPE_AAAA.to_be_bytes());
        q.extend_from_slice(&1u16.to_be_bytes());
        q.extend_from_slice(&base[12..]);
        let (resp, _) = answer(&q, &resolve).unwrap();
        assert_eq!(rcode(&resp), 3);
        assert_eq!(soa(&resp).unwrap().0, "doze");
    }

    #[test]
    fn an_answer_too_long_for_udp_goes_without_its_soa() {
        let long = format!(
            "{}.{}.{}.example.com",
            "a".repeat(63),
            "b".repeat(63),
            "c".repeat(60)
        );
        let host = format!("{}.{}.{long}", "x".repeat(40), "y".repeat(10));
        // A name under a registered domain resolves through it (the registry's walk).
        let under = move |h: &str| h.ends_with(&long).then(|| Ipv4Addr::new(198, 19, 3, 7));
        let (resp, _) = answer(&query(&host, TYPE_AAAA), &under).unwrap();
        assert!(resp.len() <= 512, "{} bytes", resp.len());
        assert!(authoritative(&resp));
        assert_eq!((rcode(&resp), soa(&resp)), (0, None));
    }

    #[test]
    fn a_relayed_answer_is_cached_ten_seconds_at_most() {
        let (mut resp, _) = answer(&query("aws.doze", TYPE_A), &resolve).unwrap();
        let ttl_at = resp.len() - 4 - 2 - 4;
        resp[ttl_at..ttl_at + 4].copy_from_slice(&3600u32.to_be_bytes());
        let capped = cap_ttl(&resp, RELAY_TTL);
        assert_eq!(&capped[ttl_at..ttl_at + 4], &10u32.to_be_bytes());
        assert_eq!(
            cap_ttl(&[1, 2], 10),
            vec![1, 2],
            "a message it cannot walk is untouched"
        );
    }
}
