//! A failure as a service states it.

use std::borrow::Cow;

/// Who is at fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// The caller sent something wrong (a 4xx).
    Sender,
    /// The service failed (a 5xx).
    Receiver,
}

/// An error answer: the code a client branches on, and the words a person reads.
///
/// A service names an error by the *shape* the model gives it (`QueueDoesNotExist`); the protocol layer finds that shape in the
/// model's tables to learn the wire code, status and fault, so those are written down once, in the model, and not again in each
/// service. An error the model does not carry (an authentication failure, say) states its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError {
    /// The error shape's name, for a modelled error.
    pub shape: Option<Cow<'static, str>>,
    /// The code on the wire.
    pub code: Cow<'static, str>,
    /// The HTTP status.
    pub status: u16,
    /// Who is at fault.
    pub fault: Fault,
    /// The message a person reads.
    pub message: String,
    /// Further fields some protocols carry (S3's error bodies).
    pub extra: Vec<(String, String)>,
}

impl ApiError {
    /// A modelled error: `shape` names it in the model, which says its code, status and fault.
    #[must_use]
    pub fn modeled(shape: &'static str, message: impl Into<String>) -> Self {
        Self {
            shape: Some(Cow::Borrowed(shape)),
            code: Cow::Borrowed(shape),
            status: 400,
            fault: Fault::Sender,
            message: message.into(),
            extra: Vec::new(),
        }
    }

    /// An error the model does not carry, stated in full.
    #[must_use]
    pub fn new(
        status: u16,
        code: impl Into<Cow<'static, str>>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            shape: None,
            code: code.into(),
            status,
            fault: if status >= 500 {
                Fault::Receiver
            } else {
                Fault::Sender
            },
            message: message.into(),
            extra: Vec::new(),
        }
    }

    /// The service itself failed.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(500, "InternalFailure", message)
    }

    /// Adds a field.
    #[must_use]
    pub fn with(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra.push((name.into(), value.into()));
        self
    }
}
