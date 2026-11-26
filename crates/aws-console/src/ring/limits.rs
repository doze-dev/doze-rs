//! What the ring holds at most: calls, and the bytes of the words of each.

use super::{Call, Extra, Via};

/// The most calls held.
pub const CAPACITY: usize = 1024;
/// The part of a full ring the console's own calls may hold before they, not an app's, are let go: a quarter.
pub const CONSOLE_SHARE: usize = 4;
/// The most bytes of each text of a record that are kept.
pub const MOST_REQUEST_ID: usize = 64;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CALLER: usize = 128;
/// See [`MOST_REQUEST_ID`].
pub const MOST_REGION: usize = 32;
/// See [`MOST_REQUEST_ID`].
pub const MOST_CODE: usize = 96;
/// See [`MOST_REQUEST_ID`]: the words of an error answer.
pub const MOST_MESSAGE: usize = 120;
/// See [`MOST_REQUEST_ID`]: `Unhandled`, `Handled`.
pub const MOST_FUNCTION_ERROR: usize = 16;
/// See [`MOST_REQUEST_ID`]. An S3 key can be 1024 bytes; the head of it says which object.
pub const MOST_RESOURCE: usize = 256;
/// See [`MOST_REQUEST_ID`]: each of the three texts of a call between services ([`Via`]).
pub const MOST_VIA: usize = 64;
/// The most text a request from the wire holds.
pub const MOST_WIRE_TEXT: usize = MOST_REQUEST_ID
    + MOST_CALLER
    + MOST_REGION
    + MOST_CODE
    + MOST_RESOURCE
    + crate::agent::MOST
    + MOST_MESSAGE
    + MOST_FUNCTION_ERROR
    + std::mem::size_of::<Extra>();
/// The most a call between services holds beside its slot: it has no request id of its own, and a [`Via`].
pub const MOST_BUS_TEXT: usize = MOST_CALLER
    + MOST_REGION
    + MOST_CODE
    + MOST_RESOURCE
    + std::mem::size_of::<Via>()
    + 3 * MOST_VIA;
/// The most one record holds beside its slot: the larger of the two.
pub const MOST_TEXT: usize = if MOST_WIRE_TEXT > MOST_BUS_TEXT {
    MOST_WIRE_TEXT
} else {
    MOST_BUS_TEXT
};
/// One record in the ring, without its text.
pub const RECORD_BYTES: usize = std::mem::size_of::<Call>();
/// The most the ring can hold, in bytes, when every text of every record is as long as it may be.
pub const MOST_BYTES: usize = CAPACITY * (RECORD_BYTES + MOST_TEXT);
