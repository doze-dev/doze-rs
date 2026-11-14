//! The wire filter (step 12): the host hands the module each client connection (`wire.accept`), because PostgreSQL's
//! first packets need answers a byte splice cannot give. The filter answers encryption requests (terminating TLS
//! when the host gives it a certificate), refuses a plain connection where TLS is required, reads the startup
//! message, has the host boot the server only then, replays the startup to it, and splices. Cancel keys are swapped
//! for synthetic ones, so a later CancelRequest (a connection of its own) reaches the right server with the real key.
//!
//! Better than Go: a client gets 30 seconds to send its startup, not forever; PostgreSQL 18's longer cancel keys
//! (protocol 3.2) are swapped and routed too (Go passed them through, so cancel failed); a synthetic key is as long
//! as the real one and unguessable; a second encryption request after TLS is refused as PostgreSQL refuses it.

use crate::wire::{self, First, Key};
use doze_module::Result;
use doze_module::caps::WireHost;
use doze_module::proto::msg::console::WireAccept;
use doze_module::proto::msg::error::{Error, ErrorKind};
use doze_module::proto::msg::instance::Address;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

/// How long a client has to send its startup message.
const PREAMBLE: Duration = Duration::from_secs(30);
/// How long routing a cancel to its server may take.
const CANCEL: Duration = Duration::from_secs(5);

/// A client or server stream, plain or TLS.
pub(crate) trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}
type Stream = Box<dyn Io>;

/// Where a synthetic key's cancel goes: the server's socket and its real key.
#[derive(Debug, Clone)]
struct Target {
    socket: String,
    real: Key,
}

/// The synthetic keys of the sessions this module is splicing, across every instance it serves.
#[derive(Debug, Default)]
pub(crate) struct Cancels(Mutex<HashMap<Key, Target>>);

impl Cancels {
    fn lookup(&self, k: &Key) -> Option<Target> {
        self.0.lock().ok().and_then(|m| m.get(k).cloned())
    }

    /// A fresh synthetic key of the real one's length, registered until the guard drops.
    fn register(self: &Arc<Self>, socket: &str, real: Key) -> Option<(Key, Registered)> {
        let mut bytes = vec![0u8; 4 + real.secret.len()];
        getrandom::fill(&mut bytes).ok()?;
        let fake = wire::cancel_key(&bytes)?;
        let mut m = self.0.lock().ok()?;
        if m.contains_key(&fake) {
            return None;
        }
        m.insert(
            fake.clone(),
            Target {
                socket: socket.to_string(),
                real,
            },
        );
        Some((fake.clone(), Registered(Arc::clone(self), fake)))
    }
}

/// Unregisters a synthetic key when its session ends.
pub(crate) struct Registered(Arc<Cancels>, Key);

impl Drop for Registered {
    fn drop(&mut self) {
        if let Ok(mut m) = (self.0).0.lock() {
            m.remove(&self.1);
        }
    }
}

/// What the client's first packets came to.
enum Preamble {
    /// The startup message to replay, and whether the client is on TLS.
    Startup(Vec<u8>, bool),
    Cancel(Key),
}

async fn refuse(client: &mut Stream, code: &str, message: &str) {
    let _ = client
        .write_all(&wire::error_response(code, &format!("doze: {message}")))
        .await;
    let _ = client.shutdown().await;
}

fn err(message: String) -> Error {
    Error::new(ErrorKind::Invalid, message)
}

/// The TLS acceptor for the host's certificate and key (DER), if it gave one.
fn acceptor(req: &WireAccept) -> Result<Option<tokio_rustls::TlsAcceptor>> {
    let Some((cert, key)) = &req.tls else {
        return Ok(None);
    };
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let cert = CertificateDer::from(cert.to_vec());
    let key = PrivateKeyDer::try_from(key.to_vec())
        .map_err(|e| err(format!("the host's TLS key: {e}")))?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .and_then(|b| b.with_no_client_auth().with_single_cert(vec![cert], key))
        .map_err(|e| err(format!("the host's TLS certificate: {e}")))?;
    Ok(Some(tokio_rustls::TlsAcceptor::from(Arc::new(config))))
}

/// Reads the client's first packets, answering encryption requests, until a startup or cancel request.
async fn preamble(
    client: &mut Stream,
    tls: Option<&tokio_rustls::TlsAcceptor>,
) -> Result<Preamble> {
    let mut encrypted = false;
    loop {
        let mut header = [0u8; 8];
        client
            .read_exact(&mut header)
            .await
            .map_err(|e| err(format!("reading preamble: {e}")))?;
        match wire::first(header).map_err(err)? {
            First::Encrypt(_) if encrypted => {
                return Err(err("a second encryption request".into()));
            }
            First::Encrypt(ssl) => match tls.filter(|_| ssl) {
                Some(acceptor) => {
                    client
                        .write_all(b"S")
                        .await
                        .map_err(|e| err(e.to_string()))?;
                    let plain = std::mem::replace(client, Box::new(tokio::io::duplex(1).0));
                    let secured = acceptor
                        .accept(plain)
                        .await
                        .map_err(|e| err(format!("TLS handshake: {e}")))?;
                    *client = Box::new(secured);
                    encrypted = true;
                }
                None => client
                    .write_all(b"N")
                    .await
                    .map_err(|e| err(e.to_string()))?,
            },
            First::Cancel(n) => {
                let mut rest = vec![0u8; n];
                client
                    .read_exact(&mut rest)
                    .await
                    .map_err(|e| err(format!("reading cancel request: {e}")))?;
                let key = wire::cancel_key(&rest)
                    .ok_or_else(|| err("malformed cancel request".into()))?;
                return Ok(Preamble::Cancel(key));
            }
            First::Startup(n) => {
                let mut raw = header.to_vec();
                raw.resize(8 + n, 0);
                let body = raw.get_mut(8..).unwrap_or_default();
                client
                    .read_exact(body)
                    .await
                    .map_err(|e| err(format!("reading startup body: {e}")))?;
                wire::startup_database(header, raw.get(8..).unwrap_or_default()).map_err(err)?;
                return Ok(Preamble::Startup(raw, encrypted));
            }
        }
    }
}

/// Sends the real key's cancel to the server the synthetic key belongs to; an unknown key is dropped (a client
/// cannot learn from the answer whether a key was right, as with PostgreSQL itself).
async fn route_cancel(cancels: &Cancels, key: &Key) {
    let Some(t) = cancels.lookup(key) else { return };
    let send = async {
        let mut s = doze_os::net::connect_unix(&t.socket).await.ok()?;
        s.write_all(&wire::cancel_request(&t.real)).await.ok()?;
        let mut sink = Vec::new();
        s.read_to_end(&mut sink).await.ok()
    };
    let _ = tokio::time::timeout(CANCEL, send).await;
}

/// Server to client until the server is ready (or refused the session), the cancel key swapped on the way.
async fn handshake<R, W>(
    server: &mut R,
    client: &mut W,
    cancels: &Arc<Cancels>,
    socket: &str,
) -> std::io::Result<Option<Registered>>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut guard = None;
    loop {
        let mut header = [0u8; 5];
        server.read_exact(&mut header).await?;
        let (kind, len) = wire::message(header).map_err(std::io::Error::other)?;
        let mut payload = vec![0u8; len];
        server.read_exact(&mut payload).await?;
        if let Some(real) = wire::backend_key(kind, &payload)
            && let Some((fake, g)) = cancels.register(socket, real)
        {
            payload = wire::key_payload(&fake);
            guard = Some(g);
        }
        client.write_all(&header).await?;
        client.write_all(&payload).await?;
        if matches!(kind, b'Z' | b'E') {
            return Ok(guard);
        }
    }
}

/// One client connection, from its first byte to its last.
pub(crate) async fn accept(
    cancels: &Arc<Cancels>,
    conn: tokio::net::TcpStream,
    req: &WireAccept,
    host: &WireHost,
) -> Result<()> {
    let tls = acceptor(req)?;
    let mut client: Stream = Box::new(conn);
    let (raw, encrypted) =
        match tokio::time::timeout(PREAMBLE, preamble(&mut client, tls.as_ref())).await {
            Ok(Ok(Preamble::Startup(raw, enc))) => (raw, enc),
            Ok(Ok(Preamble::Cancel(key))) => {
                route_cancel(cancels, &key).await;
                return Ok(());
            }
            Ok(Err(e)) => {
                refuse(&mut client, "08P01", &e.message).await;
                return Err(e);
            }
            Err(_) => {
                return Err(Error::new(
                    ErrorKind::TimedOut,
                    "the client sent no startup message",
                ));
            }
        };
    if req.require_tls && !encrypted && !req.local {
        refuse(&mut client, "28000", "SSL connection required").await;
        return Ok(());
    }
    let socket = match host.boot().await {
        Ok(Address::Unix(p)) => p,
        Ok(Address::Tcp(a)) => {
            let e = err(format!(
                "postgres listens on its socket; the host booted it on {a}"
            ));
            refuse(&mut client, "57P03", &e.message).await;
            return Err(e);
        }
        Err(e) => {
            refuse(
                &mut client,
                "57P03",
                &format!("the server could not start: {}", e.message),
            )
            .await;
            return Err(e);
        }
    };
    let connected = async {
        let mut s = doze_os::net::connect_unix(&socket).await?;
        s.write_all(&raw).await.map(|()| s)
    };
    let server = match connected.await {
        Ok(s) => s,
        Err(e) => {
            let e = Error::new(
                ErrorKind::Unavailable,
                format!("the server is not answering: {e}"),
            );
            refuse(&mut client, "57P03", &e.message).await;
            return Err(e);
        }
    };
    host.acquire().await?;
    splice(client, server, cancels, &socket).await;
    host.release().await
}

/// Both directions until each side is done; the server's side filtered until it is ready.
async fn splice(
    client: Stream,
    server: doze_os::net::Stream,
    cancels: &Arc<Cancels>,
    socket: &str,
) {
    let (mut cr, mut cw) = tokio::io::split(client);
    let (sr, mut sw) = tokio::io::split(server);
    let up = async {
        let _ = tokio::io::copy(&mut cr, &mut sw).await;
        let _ = sw.shutdown().await;
    };
    let down = async {
        let mut sr = BufReader::new(sr);
        // The key stays registered while the session lives.
        // A server message the filter cannot read ends the session: the client's stream would be out of step.
        let Ok(_guard) = handshake(&mut sr, &mut cw, cancels, socket).await else {
            let _ = cw.shutdown().await;
            return;
        };
        let _ = tokio::io::copy_buf(&mut sr, &mut cw).await;
        let _ = cw.shutdown().await;
    };
    // The server's side ending ends the session (the client's may be gone without a word); the client's ending
    // closes the server's input, and its last answers still reach the client.
    tokio::pin!(up, down);
    tokio::select! {
        () = &mut down => {}
        () = &mut up => down.await,
    }
}
