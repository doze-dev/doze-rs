#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use super::*;
use std::sync::Barrier;
use std::sync::mpsc;

/// A scratch folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let d =
            std::env::temp_dir().join(format!("doze-names-ca-under-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        Scratch(d)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ca(s: &Scratch) -> Arc<Ca> {
    Arc::new(Ca::load_or_create(&s.0).unwrap())
}

#[test]
fn a_mint_in_flight_holds_up_only_its_own_name() {
    let s = Scratch::new("parallel");
    let ca = ca(&s);
    let x = "x.execute-api.us-east-1.files.shop.doze";
    // As if a handshake for x were minting now: its slot is held.
    let slot = ca.under.lock().unwrap().slot(x);
    let minting = slot.lock().unwrap();
    let (done, other) = mpsc::channel();
    let c = ca.clone();
    std::thread::spawn(move || {
        let _ = done.send(
            c.leaf_under("y.execute-api.us-east-1.files.shop.doze")
                .is_ok(),
        );
    });
    assert_eq!(
        other.recv_timeout(Duration::from_secs(10)),
        Ok(true),
        "another name is served while x mints"
    );
    let (done, same) = mpsc::channel();
    let c = ca.clone();
    std::thread::spawn(move || {
        let _ = done.send(c.leaf_under(x).is_ok());
    });
    assert!(
        same.recv_timeout(Duration::from_millis(300)).is_err(),
        "x itself waits for the mint in flight"
    );
    drop(minting);
    assert_eq!(same.recv_timeout(Duration::from_secs(10)), Ok(true));
}

#[test]
fn the_same_name_asked_at_once_is_minted_once() {
    let s = Scratch::new("once");
    let ca = ca(&s);
    let name = "z.lambda-url.us-east-1.files.shop.doze";
    let start = Arc::new(Barrier::new(8));
    let ders: Vec<Vec<u8>> = (0..8)
        .map(|_| {
            let (ca, start) = (ca.clone(), start.clone());
            std::thread::spawn(move || {
                start.wait();
                ca.leaf_under(name).unwrap().cert_der.clone()
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|t| t.join().unwrap())
        .collect();
    assert!(ders.windows(2).all(|w| w[0] == w[1]), "one leaf for all");
    assert_eq!(stems(&s.0.join(CERTS).join(UNDER)).len(), 1);
}

#[test]
fn a_long_name_makes_a_short_file_and_a_subject_without_a_common_name() {
    let s = Scratch::new("long");
    let ca = ca(&s);
    let label = "a".repeat(55);
    let name = format!("{label}.{label}.{label}.{label}.b.files.shop.doze");
    assert!(
        name.len() > 222,
        "past what `<name>.tmp.<pid>.<nanos>` fits in 255"
    );
    let leaf = ca.leaf_under(&name).unwrap();
    let stem = file_stem(&name);
    assert!(stem.len() <= 64 + 17, "{stem}");
    assert!(s.0.join(format!("certs/under/{stem}.crt")).exists());
    let cert =
        <x509_cert::Certificate as x509_cert::der::Decode>::from_der(&leaf.cert_der).unwrap();
    assert_eq!(cert.tbs_certificate().subject().to_string(), "O=doze");
    assert_ne!(file_stem("a.doze"), file_stem("b.doze"));
}

#[test]
fn pruning_goes_by_stem_and_clears_what_a_dead_writer_left() {
    let s = Scratch::new("prune");
    let dir = s.0.join("under");
    std::fs::create_dir_all(&dir).unwrap();
    let old = SystemTime::now() - Duration::from_secs(3600);
    let touch = |name: &str, at: SystemTime| {
        let p = dir.join(name);
        std::fs::write(&p, b"x").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(at)
            .unwrap();
    };
    touch("half.key", old); // a writer died between the key and the certificate
    touch("a.crt", old + Duration::from_secs(1));
    touch("a.key", old + Duration::from_secs(1));
    touch("b.crt", SystemTime::now());
    touch("b.key", SystemTime::now());
    touch("b.tmp.1.2", old); // stray, old
    touch("c.tmp.1.3", SystemTime::now()); // a writer at work now
    assert_eq!(prune(&dir, 1), 1);
    let left: Vec<String> = {
        let mut v: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(left, ["b.crt", "b.key", "c.tmp.1.3"]);
}
