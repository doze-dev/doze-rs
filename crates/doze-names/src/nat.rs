//! The translation the macOS daemon does to each packet (N4; the daemon itself needs root).
//!
//! A client connects to a service's address V on its public port: `db.shop.doze` is 198.19.3.7, port 5432. V is
//! routed to the daemon's interface, so the kernel hands the daemon the packet, from the interface's own address G:
//! `G:sport -> V:5432`. The service really listens on G, on a private port it registered. The daemon rewrites the
//! packet as though V had sent it to that port and writes it back, which delivers it: `V:sport -> G:private`. The
//! service answers V, which comes to the daemon again and is turned round the same way. So the client talks to
//! V:5432 and the service to a peer called V, and both ends are the kernel's own TCP. Nothing is kept per
//! connection: a packet is translated from the registered ports alone, so a daemon that restarts picks up where it
//! was. A port nobody registered gets a reset at once (a closed port's answer, not a hang); a ping is answered.

use std::collections::HashMap;

const ICMP: u8 = 1;
const TCP: u8 = 6;
const FIN: u8 = 0x01;
const SYN: u8 = 0x02;
const RST: u8 = 0x04;
const ACK: u8 = 0x10;

/// Every address's registered ports.
pub trait Forwards {
    /// The private port address `v`'s public port is served on.
    fn private(&self, v: [u8; 4], public: u16) -> Option<u16>;
    /// The public port one of `v`'s private ports stands for.
    fn public(&self, v: [u8; 4], private: u16) -> Option<u16>;
}

/// The ports as the registry has them.
#[derive(Debug, Default, Clone)]
pub struct PortTable {
    pub to_private: HashMap<[u8; 4], HashMap<u16, u16>>,
    pub to_public: HashMap<[u8; 4], HashMap<u16, u16>>,
}

impl PortTable {
    /// Adds `public -> private` for address `v`.
    pub fn add(&mut self, v: [u8; 4], public: u16, private: u16) {
        self.to_private
            .entry(v)
            .or_default()
            .insert(public, private);
        self.to_public.entry(v).or_default().insert(private, public);
    }
}

impl Forwards for PortTable {
    fn private(&self, v: [u8; 4], public: u16) -> Option<u16> {
        self.to_private.get(&v)?.get(&public).copied()
    }
    fn public(&self, v: [u8; 4], private: u16) -> Option<u16> {
        self.to_public.get(&v)?.get(&private).copied()
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn put(b: &mut [u8], at: usize, bytes: &[u8]) -> Option<()> {
    b.get_mut(at..at + bytes.len())?.copy_from_slice(bytes);
    Some(())
}

fn addr(b: &[u8], at: usize) -> Option<[u8; 4]> {
    b.get(at..at + 4)?.try_into().ok()
}

/// Rewrites one IPv4 packet read from the interface, in place; whether it should be written back. `gw` is the
/// interface's address.
// fuzz:parser fuzz_packet
pub fn translate(p: &mut [u8], gw: [u8; 4], f: &dyn Forwards) -> bool {
    translate_inner(p, gw, f).unwrap_or(false)
}

fn translate_inner(p: &mut [u8], gw: [u8; 4], f: &dyn Forwards) -> Option<bool> {
    let first = *p.first()?;
    if first >> 4 != 4 {
        return Some(false);
    }
    let ihl = usize::from(first & 0x0f) * 4;
    let total = usize::from(u16_at(p, 2)?);
    if p.len() < 20 || ihl < 20 || total < ihl || total > p.len() {
        return Some(false);
    }
    if u16_at(p, 6)? & 0x3fff != 0 {
        return Some(false); // a fragment: nothing local sends one over this interface
    }
    let (src, dst) = (addr(p, 12)?, addr(p, 16)?);
    if src != gw || dst == gw {
        return Some(false);
    }
    match *p.get(9)? {
        ICMP => {
            let body = p.get_mut(ihl..total)?;
            if body.len() < 8 || *body.first()? != 8 {
                return Some(false); // echo requests only
            }
            put(body, 0, &[0])?;
            put(body, 2, &[0, 0])?;
            let sum = checksum(body, 0);
            put(body, 2, &sum.to_be_bytes())?;
            set_addrs(p, ihl, dst, src)?;
            Some(true)
        }
        TCP => {
            let body = p.get(ihl..total)?;
            if body.len() < 20 {
                return Some(false);
            }
            let (sport, dport) = (u16_at(body, 0)?, u16_at(body, 2)?);
            // The service's answer is looked for first: its source port is one it listens on at this address, which
            // the kernel never gives a client, so the two cannot be confused.
            if let Some(public) = f.public(dst, sport) {
                put(p, ihl, &public.to_be_bytes())?;
            } else if let Some(private) = f.private(dst, dport) {
                put(p, ihl + 2, &private.to_be_bytes())?;
            } else {
                return refuse(p, ihl, total);
            }
            set_addrs(p, ihl, dst, src)?;
            tcp_checksum(p, ihl, total)?;
            Some(true)
        }
        _ => Some(false),
    }
}

/// Turns a TCP packet for a port nothing registered into the reset that answers it (never a reset for a reset).
fn refuse(p: &mut [u8], ihl: usize, total: usize) -> Option<bool> {
    let body = p.get(ihl..total)?;
    let flags = *body.get(13)?;
    if flags & RST != 0 {
        return Some(false);
    }
    let (src, dst) = (addr(p, 12)?, addr(p, 16)?);
    let (sport, dport) = (u16_at(body, 0)?, u16_at(body, 2)?);
    let (seq, ack) = (u32_at(body, 4)?, u32_at(body, 8)?);
    let data_off = usize::from(*body.get(12)? >> 4) * 4;
    let mut seglen = u32::try_from(body.len().saturating_sub(data_off)).ok()?;
    seglen += u32::from(flags & SYN != 0) + u32::from(flags & FIN != 0);
    // A bare 20-byte TCP header on a 20-byte IP header.
    put(p, 0, &[0x45])?;
    put(p, 2, &40u16.to_be_bytes())?;
    put(p, 6, &[0, 0])?;
    put(p, 8, &[64])?;
    put(p, 20, &dport.to_be_bytes())?;
    put(p, 22, &sport.to_be_bytes())?;
    if flags & ACK != 0 {
        put(p, 24, &ack.to_be_bytes())?;
        put(p, 28, &0u32.to_be_bytes())?;
        put(p, 33, &[RST])?;
    } else {
        put(p, 24, &0u32.to_be_bytes())?;
        put(p, 28, &seq.wrapping_add(seglen).to_be_bytes())?;
        put(p, 33, &[RST | ACK])?;
    }
    put(p, 32, &[5 << 4])?;
    put(p, 34, &[0, 0])?; // window
    put(p, 38, &[0, 0])?; // urgent
    set_addrs(p, 20, dst, src)?;
    tcp_checksum(p, 20, 40)?;
    Some(true)
}

/// The length of the packet `translate` left in `p`.
pub fn packet_len(p: &[u8]) -> usize {
    u16_at(p, 2).map_or(0, usize::from)
}

fn set_addrs(p: &mut [u8], ihl: usize, src: [u8; 4], dst: [u8; 4]) -> Option<()> {
    put(p, 12, &src)?;
    put(p, 16, &dst)?;
    put(p, 10, &[0, 0])?;
    let sum = checksum(p.get(..ihl)?, 0);
    put(p, 10, &sum.to_be_bytes())
}

/// The TCP checksum, worked out whole rather than adjusted: a packet from this machine's own stack can arrive with
/// it left for the hardware to fill in.
fn tcp_checksum(p: &mut [u8], ihl: usize, total: usize) -> Option<()> {
    put(p, ihl + 16, &[0, 0])?;
    let words = [
        u16_at(p, 12)?,
        u16_at(p, 14)?,
        u16_at(p, 16)?,
        u16_at(p, 18)?,
    ];
    let len = u32::try_from(total - ihl).ok()?;
    let pseudo = words.iter().map(|w| u32::from(*w)).sum::<u32>() + u32::from(TCP) + len;
    let sum = checksum(p.get(ihl..total)?, pseudo);
    put(p, ihl + 16, &sum.to_be_bytes())
}

/// The internet checksum of `b`, continued from `sum`.
pub(crate) fn checksum(b: &[u8], sum: u32) -> u16 {
    let mut s = u64::from(sum);
    let (pairs, rest) = b.as_chunks::<2>();
    for pair in pairs {
        s += u64::from(u16::from_be_bytes(*pair));
    }
    if let Some(last) = rest.first() {
        s += u64::from(*last) << 8;
    }
    while s >> 16 != 0 {
        s = (s >> 16) + (s & 0xffff);
    }
    !u16::try_from(s).unwrap_or(0)
}

#[cfg(test)]
#[path = "nat_tests.rs"]
mod tests;
