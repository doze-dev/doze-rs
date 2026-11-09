use super::*;
use crate::batch::BatchError;
use crate::batch::build::BatchBuilder;

fn ok() -> Vec<u8> {
    BatchBuilder::new()
        .records(3)
        .producer(7, 2, 40)
        .timestamps(100, 130)
        .build()
        .to_vec()
}

#[test]
fn a_peek_refuses_what_has_no_extent() {
    assert_eq!(
        Peek::read(&ok()[..30]).unwrap_err(),
        BatchError::TooShort(30)
    );
    let mut v = ok();
    v[16] = 1;
    assert_eq!(Peek::read(&v).unwrap_err(), BatchError::Magic(1));
    // A declared length too small to hold the header.
    let mut v = ok();
    v[8..12].copy_from_slice(&20i32.to_be_bytes());
    assert!(matches!(
        Peek::read(&v).unwrap_err(),
        BatchError::Length { .. }
    ));
    let mut v = ok();
    v[8..12].copy_from_slice(&(-5i32).to_be_bytes());
    assert!(matches!(
        Peek::read(&v).unwrap_err(),
        BatchError::Length { .. }
    ));
    // Counts: as strict as a parse (but for the checksum).
    let v = BatchBuilder::new().records(0).body(vec![]).build();
    assert!(matches!(
        Peek::read(&v).unwrap_err(),
        BatchError::Count { .. }
    ));
    let v = BatchBuilder::new()
        .records(2_000_000_000)
        .body(vec![0; 8])
        .build();
    assert!(matches!(
        Peek::read(&v).unwrap_err(),
        BatchError::CountExceedsBody { .. }
    ));
    let v = BatchBuilder::new().attributes(6).build();
    assert_eq!(Peek::read(&v).unwrap_err(), BatchError::UnknownCodec(6));
}

#[test]
fn a_verified_peek_is_a_parse_without_the_ownership() {
    let v = ok();
    assert!(Peek::read_verified(&v).is_ok());
    let mut bad = v.clone();
    let last = bad.len() - 1;
    bad[last] ^= 1;
    assert!(matches!(
        Peek::read_verified(&bad),
        Err(BatchError::Crc { .. })
    ));
    assert!(matches!(
        Peek::read_verified(&v[..v.len() - 1]),
        Err(BatchError::Length { .. })
    ));
    let mut longer = v.clone();
    longer.push(0);
    assert!(matches!(
        Peek::read_verified(&longer),
        Err(BatchError::Length { .. })
    ));
}
