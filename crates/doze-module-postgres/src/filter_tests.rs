//! The filter's opening, against a client written by hand: GSSAPI encryption declined, a second encryption request on
//! TLS refused, and a client that never sends its startup timed out.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{Preamble, Stream, opening};
use crate::wire::{GSSENC_REQUEST, SSL_REQUEST};
use doze_module::proto::msg::error::ErrorKind;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

const LIMIT: Duration = Duration::from_secs(5);

fn request(code: u32) -> Vec<u8> {
    [8u32.to_be_bytes(), code.to_be_bytes()].concat()
}

/// A protocol 3.0 startup for user `u` and database `d`.
fn startup() -> Vec<u8> {
    let body = b"\0\x03\0\0user\0u\0database\0d\0\0";
    let len = u32::try_from(body.len() + 4).unwrap();
    [len.to_be_bytes().as_slice(), body].concat()
}

fn pair() -> (DuplexStream, Stream) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    (client, Box::new(server))
}

fn fixtures(name: &str) -> String {
    format!(
        "{}/../kafka-server/tests/tls-fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn tls() -> (tokio_rustls::TlsAcceptor, tokio_rustls::TlsConnector) {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_file_iter(fixtures("ec-chain.pem"))
            .unwrap()
            .map(Result::unwrap)
            .collect();
    let key = PrivateKeyDer::from_pem_file(fixtures("ec-key-pkcs8.pem")).unwrap();
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(CertificateDer::from_pem_file(fixtures("root.pem")).unwrap())
        .unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    (
        tokio_rustls::TlsAcceptor::from(Arc::new(server)),
        tokio_rustls::TlsConnector::from(Arc::new(client)),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn gssapi_encryption_is_declined_and_the_startup_still_read() {
    let (mut client, mut server) = pair();
    let filter = tokio::spawn(async move { opening(&mut server, None, LIMIT).await });
    client.write_all(&request(GSSENC_REQUEST)).await.unwrap();
    let mut answer = [0u8; 1];
    client.read_exact(&mut answer).await.unwrap();
    assert_eq!(
        &answer, b"N",
        "GSSENC is declined with one byte, as PostgreSQL does"
    );
    client.write_all(&startup()).await.unwrap();
    match filter.await.unwrap().unwrap() {
        Preamble::Startup(raw, encrypted) => {
            assert_eq!(raw, startup());
            assert!(!encrypted);
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn gssapi_encryption_is_declined_even_where_tls_is_offered() {
    let (acceptor, _) = tls();
    let (mut client, mut server) = pair();
    let filter = tokio::spawn(async move { opening(&mut server, Some(&acceptor), LIMIT).await });
    client.write_all(&request(GSSENC_REQUEST)).await.unwrap();
    let mut answer = [0u8; 1];
    client.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"N");
    client.write_all(&startup()).await.unwrap();
    assert!(matches!(
        filter.await.unwrap().unwrap(),
        Preamble::Startup(_, false)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_second_encryption_request_inside_tls_is_refused() {
    let (acceptor, connector) = tls();
    let (mut client, mut server) = pair();
    let filter = tokio::spawn(async move { opening(&mut server, Some(&acceptor), LIMIT).await });
    client.write_all(&request(SSL_REQUEST)).await.unwrap();
    let mut answer = [0u8; 1];
    client.read_exact(&mut answer).await.unwrap();
    assert_eq!(&answer, b"S");
    let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let mut secured = connector.connect(name, client).await.unwrap();
    secured.write_all(&request(SSL_REQUEST)).await.unwrap();
    secured.flush().await.unwrap();
    let e = filter.await.unwrap().unwrap_err();
    assert_eq!(e.kind, Some(ErrorKind::Invalid));
    assert!(e.message.contains("second encryption request"), "{e:?}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_client_that_sends_no_startup_is_timed_out() {
    let (mut client, mut server) = pair();
    let filter =
        tokio::spawn(async move { opening(&mut server, None, Duration::from_millis(200)).await });
    // Half a header, then nothing: the filter must not wait for ever (Go did).
    client.write_all(&[0, 0, 0]).await.unwrap();
    let e = tokio::time::timeout(Duration::from_secs(10), filter)
        .await
        .expect("the filter gave up by itself")
        .unwrap()
        .unwrap_err();
    assert_eq!(e.kind, Some(ErrorKind::TimedOut));
    drop(client);
}
