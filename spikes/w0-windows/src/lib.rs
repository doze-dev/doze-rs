pub mod msg;
pub mod os;
pub mod wire;

/// Method ids. Host → module below 100, module → host from 100.
pub mod m {
    pub const HELLO: u16 = 1;
    pub const PLAN: u16 = 2;
    pub const RESOLVE: u16 = 3;
    pub const TAIL: u16 = 4;
    pub const WIRE: u16 = 5;
    pub const ECHO: u16 = 6;
    pub const CRASH: u16 = 7;
    pub const SLEEP: u16 = 9;
    pub const FETCH_ENSURE: u16 = 100;
    pub const WIRE_BOOT: u16 = 101;
    pub const WIRE_RELEASE: u16 = 102;
    pub const TAIL_CANCELLED: u16 = 103;
}
