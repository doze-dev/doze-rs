//! A request body handed to a service as a stream: plain, or in the aws-chunked encoding with its chunk signatures (or trailing
//! checksum) checked as the chunks arrive. Neither is ever held whole.

use aws_auth::chunked::{ChunkAuth, ChunkDecoder, ChunkError, Event};
use aws_wire::ApiError;
use aws_wire::rest::Upload;
use aws_wire::service::BoxFuture;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Incoming;

/// How the payload is framed.
pub(crate) enum Framing {
    /// The bytes are the payload.
    Plain,
    /// aws-chunked: with the signing context when chunks are verified, and whether the trailer is signed.
    Chunked(Option<ChunkAuth>, bool),
}

struct Stream {
    body: Incoming,
    decoder: Option<ChunkDecoder>,
    /// Bytes received and not yet consumed by the chunk decoder.
    pending: Bytes,
    length: Option<u64>,
    trailers: Vec<(String, String)>,
    done: bool,
}

/// An upload over `body`; `length` is the payload's declared length.
pub(crate) fn upload(body: Incoming, framing: Framing, length: Option<u64>) -> Box<dyn Upload> {
    let decoder = match framing {
        Framing::Plain => None,
        Framing::Chunked(auth, signed_trailer) => Some(ChunkDecoder::new(auth, signed_trailer)),
    };
    Box::new(Stream {
        body,
        decoder,
        pending: Bytes::new(),
        length,
        trailers: Vec::new(),
        done: false,
    })
}

fn broken(e: &dyn std::fmt::Display) -> ApiError {
    ApiError::new(
        400,
        "IncompleteBody",
        format!("The request body could not be read: {e}"),
    )
}

fn framing(e: &ChunkError) -> ApiError {
    match e {
        ChunkError::BadSignature | ChunkError::BadTrailerSignature | ChunkError::MissingSignature => {
            ApiError::new(
                403,
                "SignatureDoesNotMatch",
                "The request signature we calculated does not match the signature you provided. Check your key and signing method.",
            )
        }
        other => ApiError::new(400, "IncompleteBody", format!("The aws-chunked body is not valid: {other}")),
    }
}

impl Stream {
    /// The next data frame of the connection, `None` at its end.
    async fn frame(&mut self) -> Result<Option<Bytes>, ApiError> {
        loop {
            match self.body.frame().await {
                None => return Ok(None),
                Some(Err(e)) => return Err(broken(&e)),
                Some(Ok(f)) => {
                    if let Ok(data) = f.into_data()
                        && !data.is_empty()
                    {
                        return Ok(Some(data));
                    }
                }
            }
        }
    }

    async fn next_plain(&mut self) -> Result<Option<Bytes>, ApiError> {
        self.frame().await
    }

    async fn next_chunked(&mut self) -> Result<Option<Bytes>, ApiError> {
        loop {
            let Some(decoder) = self.decoder.as_mut() else {
                return Ok(None);
            };
            match decoder.next(&mut self.pending).map_err(|e| framing(&e))? {
                Event::Data(data) => {
                    if !data.is_empty() {
                        return Ok(Some(data));
                    }
                }
                Event::Done => return Ok(self.finish()),
                Event::Need => match self.frame().await? {
                    Some(data) => self.pending = data,
                    None => {
                        let Some(decoder) = self.decoder.as_mut() else {
                            return Ok(None);
                        };
                        decoder.finish_at_eof().map_err(|e| framing(&e))?;
                        return Ok(self.finish());
                    }
                },
            }
        }
    }

    fn finish(&mut self) -> Option<Bytes> {
        self.done = true;
        if let Some(d) = &self.decoder {
            self.trailers = d.trailers().to_vec();
        }
        None
    }
}

impl Upload for Stream {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, ApiError>> {
        Box::pin(async move {
            if self.done {
                return Ok(None);
            }
            if self.decoder.is_some() {
                self.next_chunked().await
            } else {
                let next = self.next_plain().await?;
                self.done = next.is_none();
                Ok(next)
            }
        })
    }

    fn trailers(&self) -> Vec<(String, String)> {
        self.trailers.clone()
    }

    fn length(&self) -> Option<u64> {
        self.length
    }
}
