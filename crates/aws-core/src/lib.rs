//! What every aws crate shares. No dependencies: hex, the percent-encoding SigV4 and the REST protocols need, and calendar
//! arithmetic for the wire timestamp formats (so no date crate is linked). The domain types (ARNs, account ids, regions,
//! principals) join here in A4.

pub mod base64;
pub mod hex;
pub mod time;
pub mod uri;
