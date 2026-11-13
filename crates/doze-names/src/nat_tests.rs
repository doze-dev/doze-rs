//! Go's nat_test.go, ported.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use super::*;

/// The internet checksum written the obvious way (Go: slowChecksum).
fn slow_checksum(b: &[u8], mut sum: u32) -> u16 {
    for c in b.chunks(2) {
        sum += u32::from(c[0]) << 8 | u32::from(*c.get(1).unwrap_or(&0));
    }
    while sum >> 16 != 0 {
        sum = (sum >> 16) + (sum & 0xffff);
    }
    !(sum as u16)
}

#[test]
fn the_checksum_matches_the_obvious_one() {
    let mut x: u32 = 1;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        x
    };
    for n in 0..600 {
        let b: Vec<u8> = (0..n).map(|_| next() as u8).collect();
        let seed = next() >> 12;
        assert_eq!(checksum(&b, seed), slow_checksum(&b, seed), "{n} bytes");
    }
    let ff = vec![0xff; 4096];
    assert_eq!(checksum(&ff, 0xffff), slow_checksum(&ff, 0xffff));
}

const GW: [u8; 4] = [198, 19, 0, 1];
const DB: [u8; 4] = [198, 19, 3, 7];

fn table() -> PortTable {
    let mut t = PortTable::default();
    t.add(DB, 5432, 51000);
    t
}

#[allow(clippy::too_many_arguments)]
fn tcp(
    src: [u8; 4],
    dst: [u8; 4],
    sport: u16,
    dport: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    payload: &str,
) -> Vec<u8> {
    let mut p = vec![0u8; 40 + payload.len()];
    p[0] = 0x45;
    p[8] = 64;
    p[9] = TCP;
    let len = u16::try_from(p.len()).unwrap();
    p[2..4].copy_from_slice(&len.to_be_bytes());
    p[20..22].copy_from_slice(&sport.to_be_bytes());
    p[22..24].copy_from_slice(&dport.to_be_bytes());
    p[24..28].copy_from_slice(&seq.to_be_bytes());
    p[28..32].copy_from_slice(&ack.to_be_bytes());
    p[32] = 5 << 4;
    p[33] = flags;
    p[34..36].copy_from_slice(&65535u16.to_be_bytes());
    p[40..].copy_from_slice(payload.as_bytes());
    set_addrs(&mut p, 20, src, dst).unwrap();
    let total = p.len();
    tcp_checksum(&mut p, 20, total).unwrap();
    p
}

#[derive(Debug, PartialEq)]
struct Seen {
    src: [u8; 4],
    dst: [u8; 4],
    sport: u16,
    dport: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    payload: String,
}

/// Takes a packet apart, failing if either checksum is wrong.
fn read(p: &[u8]) -> Seen {
    let total = packet_len(p);
    assert_eq!(
        slow_checksum(&p[..20], 0),
        0,
        "the IP header checksum is wrong"
    );
    let seg = &p[20..total];
    let words: u32 = [12, 14, 16, 18]
        .iter()
        .map(|&i| u32::from(u16_at(p, i).unwrap()))
        .sum();
    let pseudo = words + u32::from(TCP) + u32::try_from(seg.len()).unwrap();
    assert_eq!(slow_checksum(seg, pseudo), 0, "the TCP checksum is wrong");
    let off = usize::from(seg[12] >> 4) * 4;
    Seen {
        src: addr(p, 12).unwrap(),
        dst: addr(p, 16).unwrap(),
        sport: u16_at(seg, 0).unwrap(),
        dport: u16_at(seg, 2).unwrap(),
        flags: seg[13],
        seq: u32_at(seg, 4).unwrap(),
        ack: u32_at(seg, 8).unwrap(),
        payload: String::from_utf8_lossy(&seg[off..]).into(),
    }
}

#[allow(clippy::too_many_arguments)]
fn seen(
    src: [u8; 4],
    dst: [u8; 4],
    sport: u16,
    dport: u16,
    flags: u8,
    seq: u32,
    ack: u32,
    payload: &str,
) -> Seen {
    Seen {
        src,
        dst,
        sport,
        dport,
        flags,
        seq,
        ack,
        payload: payload.into(),
    }
}

#[test]
fn a_connection_is_translated_both_ways() {
    let mut out = tcp(GW, DB, 49200, 5432, ACK, 100, 200, "select 1");
    assert!(
        translate(&mut out, GW, &table()),
        "the client's packet was dropped"
    );
    assert_eq!(
        read(&out),
        seen(DB, GW, 49200, 51000, ACK, 100, 200, "select 1")
    );
    let mut back = tcp(GW, DB, 51000, 49200, ACK, 200, 108, "1 row");
    assert!(
        translate(&mut back, GW, &table()),
        "the service's answer was dropped"
    );
    assert_eq!(
        read(&back),
        seen(DB, GW, 5432, 49200, ACK, 200, 108, "1 row")
    );
}

#[test]
fn an_unregistered_port_is_refused_at_once() {
    let mut syn = tcp(GW, DB, 49200, 6379, SYN, 1000, 0, "");
    assert!(
        translate(&mut syn, GW, &table()),
        "dropped, which a client sees as a hang"
    );
    assert_eq!(
        read(&syn),
        seen(DB, GW, 6379, 49200, RST | ACK, 0, 1001, "")
    );
    let mut data = tcp(GW, DB, 49200, 6379, ACK, 1001, 777, "ping");
    assert!(translate(&mut data, GW, &table()));
    assert_eq!(read(&data), seen(DB, GW, 6379, 49200, RST, 777, 0, ""));
    assert!(
        !translate(&mut tcp(GW, DB, 49200, 6379, RST, 1, 0, ""), GW, &table()),
        "a reset is never answered"
    );
}

#[test]
fn ping_is_answered() {
    let mut p = vec![0u8; 20 + 8 + 5];
    p[0] = 0x45;
    p[8] = 64;
    p[9] = ICMP;
    let len = u16::try_from(p.len()).unwrap();
    p[2..4].copy_from_slice(&len.to_be_bytes());
    p[20] = 8;
    p[24..26].copy_from_slice(&7u16.to_be_bytes());
    p[28..].copy_from_slice(b"hello");
    let sum = slow_checksum(&p[20..], 0);
    p[22..24].copy_from_slice(&sum.to_be_bytes());
    set_addrs(&mut p, 20, GW, DB).unwrap();
    assert!(translate(&mut p, GW, &table()));
    assert_eq!(
        (p[20], slow_checksum(&p[20..], 0), &p[28..]),
        (0, 0, &b"hello"[..])
    );
    assert_eq!((addr(&p, 12).unwrap(), addr(&p, 16).unwrap()), (DB, GW));
}

#[test]
fn other_packets_are_dropped() {
    let stranger = [10, 0, 0, 9];
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "from another source",
            tcp(stranger, DB, 49200, 5432, SYN, 1, 0, ""),
        ),
        (
            "to the gateway itself",
            tcp(GW, GW, 49200, 5432, SYN, 1, 0, ""),
        ),
        (
            "too short",
            tcp(GW, DB, 49200, 5432, SYN, 1, 0, "")[..30].to_vec(),
        ),
        ("not IPv4", [&[0x60u8][..], &[0u8; 60]].concat()),
    ];
    for (name, mut p) in cases {
        assert!(!translate(&mut p, GW, &table()), "{name}");
    }
    let mut frag = tcp(GW, DB, 49200, 5432, ACK, 1, 1, "x");
    frag[6..8].copy_from_slice(&0x2000u16.to_be_bytes());
    assert!(!translate(&mut frag, GW, &table()), "a fragment");
}
