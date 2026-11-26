//! A lazy service as the register of them sees it (`crate::Rests`): whether it runs, and the way to tell it to rest now.

use std::future::Future;
use std::pin::Pin;

use super::{Resting, Service, Shared, Slot};

impl<S: Service> Resting for Shared<S> {
    fn live(&self) -> bool {
        matches!(&self.lock().slot, Slot::Live { inbox, .. } if !inbox.is_closed())
    }

    fn rest(&self) -> Pin<Box<dyn Future<Output = bool> + Send + '_>> {
        Box::pin(async move {
            let tokens = match &self.lock().slot {
                Slot::Live {
                    inbox, rest, ended, ..
                } if !inbox.is_closed() => Some((rest.clone(), ended.clone())),
                _ => None,
            };
            let Some((rest, ended)) = tokens else {
                return false;
            };
            rest.cancel();
            ended.cancelled().await;
            true
        })
    }
}
