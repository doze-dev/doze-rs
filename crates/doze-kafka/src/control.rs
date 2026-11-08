//! A test control: with `DOZE_KAFKA_TEST_CONTROL` naming a unix socket, the line `check-offsets` applies the offsets
//! retention now and is answered `ok` once it has. The conformance harness uses it to make an expiry happen when it
//! says rather than when the clock does. Not there on other systems, and not there unless the variable is set.

use std::sync::Arc;

use kafka_broker::Broker;
use kafka_storage::fs::RealFs;

/// Starts listening on the socket, if one is asked for.
#[cfg(unix)]
pub(crate) fn serve(broker: &Arc<Broker<RealFs>>) -> Option<tokio::task::JoinHandle<()>> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    let path = std::env::var_os("DOZE_KAFKA_TEST_CONTROL")?;
    // A socket left by an earlier run would refuse the bind; anything else at the path is not ours to remove.
    if let Ok(found) = std::fs::symlink_metadata(&path) {
        if std::os::unix::fs::FileTypeExt::is_socket(&found.file_type()) {
            let _ = std::fs::remove_file(&path);
        } else {
            tracing::warn!("the test control path exists and is not a socket; not listening");
            return None;
        }
    }
    let listener = match UnixListener::bind(&path) {
        Ok(l) => l,
        Err(e) => {
            tracing::warn!(error = %e, "the test control socket could not be bound");
            return None;
        }
    };
    let broker = Arc::clone(broker);
    Some(tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let broker = Arc::clone(&broker);
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut lines = BufReader::new(read).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if line.trim() == "check-offsets" {
                        broker.check_offsets().await;
                        if write.write_all(b"ok\n").await.is_err() {
                            break;
                        }
                    }
                }
            });
        }
    }))
}

#[cfg(not(unix))]
pub(crate) fn serve(_broker: &Arc<Broker<RealFs>>) -> Option<tokio::task::JoinHandle<()>> {
    None
}
