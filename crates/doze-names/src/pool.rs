//! Where a name's address comes from.
//!
//! A zone address is a two-octet base plus an offset. The offset is what a name owns: 2 to 9 are the fixed apex
//! addresses, everything from 10 up is hashed. The base is the machine's:
//!
//! - Linux and Windows: 127.0. All of 127.0.0.0/8 is local there, so a service binds its address directly.
//! - macOS: 198.19, and no service binds its address. A loopback address exists on a Mac only once aliased onto lo0,
//!   and aliasing in bulk is not survivable (measured on macOS 27: 250 more took mDNSResponder to 100% of a core and
//!   a lookup from 10 ms to over a second; 4,000 took a lookup to 150 s and configd was killed by its watchdog). So a
//!   daemon owns one interface with the whole block routed to it and translates each address to a private port.
//!   198.18.0.0/15 is for benchmarking (RFC 2544), never routed; this takes the lower half of 198.19 because other
//!   tools on a Mac use the rest (OrbStack has 198.19.248.0/22, proxy tools default to 198.18.0.0/16).
//!
//! The hash is Go's (FNV-1a, 32 bits) and the probe is Go's, so a Go and a Rust program give a name the same address.

use crate::{Error, Name, Tier};
use std::collections::BTreeSet;
use std::net::Ipv4Addr;

pub(crate) const APEX_BASE: u16 = 2;
pub(crate) const DYNAMIC_BASE: u16 = 10;
/// The block is a /17: third octets 0 to 127; the last octet never 0 or 255, which leaves 254 to each third octet.
const BLOCK_THIRDS: u16 = 128;
const PER_THIRD: u32 = 254;
pub(crate) const BLOCK_SLOTS: u32 = BLOCK_THIRDS as u32 * PER_THIRD;
const BLOCK_PREFIX: u8 = 17;

/// The fixed offset of each well-known service. Adding one is a compatibility commitment: on Linux it goes into
/// people's /etc/hosts. 4 to 9 are held for postgres, valkey, and others as they arrive.
const APEX_OFFSET: &[(&str, u16)] = &[("aws", 2), ("kafka", 3)];

/// How this machine gives a name an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Net {
    a: u8,
    b: u8,
    /// No service can bind its address: the block is routed to the daemon's interface.
    pub virtual_: bool,
}

/// Linux and Windows.
pub const LOOPBACK: Net = Net {
    a: 127,
    b: 0,
    virtual_: false,
};
/// macOS.
pub const VIRTUAL: Net = Net {
    a: 198,
    b: 19,
    virtual_: true,
};

impl Net {
    /// This machine's network.
    pub fn native() -> Net {
        if cfg!(target_os = "macos") {
            VIRTUAL
        } else {
            LOOPBACK
        }
    }

    /// The address at an offset into the block.
    pub fn at(self, offset: u16) -> Ipv4Addr {
        let [hi, lo] = offset.to_be_bytes();
        Ipv4Addr::new(self.a, self.b, hi, lo)
    }

    /// An address's offset into the block, if it is in it.
    pub fn offset(self, ip: Ipv4Addr) -> Option<u16> {
        let [a, b, c, d] = ip.octets();
        (a == self.a && b == self.b && u16::from(c) < BLOCK_THIRDS)
            .then(|| u16::from_be_bytes([c, d]))
    }

    /// The daemon's own address (only a virtual network has one in use).
    pub fn gateway(self) -> Ipv4Addr {
        self.at(1)
    }

    /// The block, as a route to it is written.
    pub fn cidr(self) -> String {
        format!("{}.{}.0.0/{BLOCK_PREFIX}", self.a, self.b)
    }

    /// Whether `ip` is one of the addresses this package hands out.
    pub fn in_pool(self, ip: Ipv4Addr) -> bool {
        self.offset(ip).is_some_and(|o| o >= APEX_BASE)
    }
}

/// One of the block's slots as an offset: 254 addresses to each third octet, .1 to .254.
pub(crate) fn slot_offset(slot: u32) -> u16 {
    let third = slot / PER_THIRD;
    let last = slot % PER_THIRD + 1;
    u16::try_from((third << 8) | last).unwrap_or(u16::MAX)
}

/// The fixed address of a well-known service.
pub fn apex_addr(net: Net, service: &str) -> Option<Ipv4Addr> {
    APEX_OFFSET
        .iter()
        .find(|(s, _)| *s == service)
        .map(|(_, off)| net.at(*off))
}

/// Whether a stack may not be called `name`: its services would sit under a machine-wide name (a stack called "aws"
/// puts db.aws.doze under aws.doze, which belongs to whichever local AWS holds the apex).
pub fn reserved_stack(name: &str) -> bool {
    let l = crate::label(name);
    APEX_OFFSET.iter().any(|(s, _)| *s == l)
}

/// FNV-1a, 32 bits: Go's `hash/fnv` New32a.
pub(crate) fn fnv1a32(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

/// The address a name should resolve to. Apex names come from the fixed table; qualified names are hashed into the
/// block, so a name lands on the same address run after run with nothing persisted, probing forward past `taken`
/// (addresses live peers hold) so a collision shifts rather than fails.
pub(crate) fn address_for(
    net: Net,
    name: &Name,
    taken: &BTreeSet<Ipv4Addr>,
) -> Result<Ipv4Addr, Error> {
    if name.tier == Tier::Apex {
        return apex_addr(net, name.service())
            .ok_or_else(|| Error::NoApexAddress(name.host.clone()));
    }
    let start = fnv1a32(name.host.as_bytes()) % BLOCK_SLOTS;
    for i in 0..BLOCK_SLOTS {
        let off = slot_offset((start + i) % BLOCK_SLOTS);
        if off < DYNAMIC_BASE {
            continue;
        }
        let ip = net.at(off);
        if !taken.contains(&ip) {
            return Ok(ip);
        }
    }
    Err(Error::PoolFull(net.cidr()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn fnv1a_is_gos() {
        // Go: fnv.New32a over "" and "a" (the published FNV-1a test vectors).
        assert_eq!(fnv1a32(b""), 0x811c_9dc5);
        assert_eq!(fnv1a32(b"a"), 0xe40c_292c);
        assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
    }

    #[test]
    fn a_full_pool_says_so_and_the_last_free_address_is_found() {
        // Go: TestAFullPoolSaysSo.
        let net = LOOPBACK;
        let mut taken: BTreeSet<Ipv4Addr> =
            (0..BLOCK_SLOTS).map(|s| net.at(slot_offset(s))).collect();
        let free = net.at(slot_offset(BLOCK_SLOTS - 7));
        taken.remove(&free);
        assert_eq!(
            address_for(net, &Name::qualified("last", "shop"), &taken).unwrap(),
            free
        );
        taken.insert(free);
        let e = address_for(net, &Name::qualified("one-too-many", "shop"), &taken).unwrap_err();
        assert!(e.to_string().contains("in use"), "{e}");
    }

    #[test]
    fn a_name_has_the_same_offset_on_both_systems() {
        // Go: TestANameHasTheSameOffsetOnBothSystems.
        let n = Name::qualified("db", "shop");
        let a = address_for(LOOPBACK, &n, &BTreeSet::new()).unwrap();
        let b = address_for(VIRTUAL, &n, &BTreeSet::new()).unwrap();
        assert_eq!(LOOPBACK.offset(a), VIRTUAL.offset(b));
        assert_eq!(&b.octets()[..2], &[198, 19]);
    }

    #[test]
    fn stack_names_that_would_collide_with_the_apex() {
        // Go: TestStackNamesThatWouldCollideWithTheApex.
        for (name, want) in [
            ("aws", true),
            ("kafka", true),
            ("AWS", true),
            ("shop", false),
            ("awsome", false),
        ] {
            assert_eq!(reserved_stack(name), want, "{name}");
        }
    }
}
