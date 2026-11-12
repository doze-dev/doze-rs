//! One [`Lazy`] service per key: a service whose state is kept apart by region (an AWS queue in `eu-west-1` is not the queue of the same
//! name in `us-east-1`). Each key has its own actor, its own directory and its own idle timer, so an unused region costs nothing and a
//! used one is reaped when it has been quiet.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use crate::lazy::{Ctx, Lazy, Service};

/// The most keys one service will hold at once: AWS has fewer than fifty regions, and an unbounded map would let a request grow
/// the process by naming new ones.
pub const MAX_KEYS: usize = 64;

/// Why no actor could be had for a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    /// The key is empty, too long, or has a character other than a lower-case letter, a digit or a dash: it names a directory.
    Unsafe,
    /// [`MAX_KEYS`] keys are already in use.
    TooMany,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsafe => "the key is not a valid region name",
            Self::TooMany => "too many regions are in use",
        })
    }
}

/// A set of [`Lazy`] services, one per key, made when first asked for.
pub struct Keyed<S: Service> {
    name: &'static str,
    ctx: Arc<Ctx>,
    home: &'static str,
    actors: Mutex<HashMap<String, Arc<Lazy<S>>>>,
}

impl<S: Service> fmt::Debug for Keyed<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Keyed({})", self.name)
    }
}

impl<S: Service> Keyed<S> {
    /// Registers the service. The `home` key keeps its files in `ctx.dir` itself (so state written before keys existed is still found);
    /// every other key keeps them in `ctx.dir/regions/<key>`.
    #[must_use]
    pub fn new(name: &'static str, ctx: Arc<Ctx>, home: &'static str) -> Self {
        Self {
            name,
            ctx,
            home,
            actors: Mutex::new(HashMap::new()),
        }
    }

    /// The service for `key`.
    ///
    /// # Errors
    /// [`KeyError`] for a key that is not safe to use as a directory name, or when too many keys are in use.
    pub fn actor(&self, key: &str) -> Result<Arc<Lazy<S>>, KeyError> {
        if key.is_empty()
            || key.len() > 32
            || !key
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(KeyError::Unsafe);
        }
        let mut actors = self.actors.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = actors.get(key) {
            return Ok(Arc::clone(found));
        }
        if actors.len() >= MAX_KEYS {
            return Err(KeyError::TooMany);
        }
        let dir = if key == self.home {
            self.ctx.dir.clone()
        } else {
            self.ctx.dir.join("regions").join(key)
        };
        let ctx = Arc::new(Ctx {
            dir,
            idle: self.ctx.idle,
            tasks: self.ctx.tasks.clone(),
            clock: Arc::clone(&self.ctx.clock),
            ext: self.ctx.ext.clone(),
        });
        let actor = Arc::new(Lazy::new(self.name, ctx));
        actors.insert(key.to_owned(), Arc::clone(&actor));
        Ok(actor)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use std::convert::Infallible;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::Tasks;

    /// A service that says which directory it was opened in.
    struct Where(PathBuf);

    impl Service for Where {
        type Request = ();
        type Response = PathBuf;
        type Error = Infallible;

        async fn open(ctx: &Ctx, _: &'static str) -> Result<Self, Infallible> {
            Ok(Self(ctx.dir.clone()))
        }

        async fn handle(&mut self, (): ()) -> PathBuf {
            self.0.clone()
        }

        async fn close(self) {}
    }

    fn keyed() -> Keyed<Where> {
        let ctx = Arc::new(Ctx {
            dir: PathBuf::from("/data/sqs"),
            idle: Duration::from_secs(60),
            tasks: Tasks::new(),
            clock: Arc::new(crate::ManualClock::at(0)),
            ext: None,
        });
        Keyed::new("where", ctx, "us-east-1")
    }

    #[tokio::test]
    async fn the_home_key_keeps_the_directory_and_the_others_get_their_own() {
        let k = keyed();
        let home = k.actor("us-east-1").unwrap().call(()).await.unwrap();
        let other = k.actor("eu-west-1").unwrap().call(()).await.unwrap();
        assert_eq!(home, PathBuf::from("/data/sqs"));
        assert_eq!(other, PathBuf::from("/data/sqs/regions/eu-west-1"));
        assert!(Arc::ptr_eq(
            &k.actor("eu-west-1").unwrap(),
            &k.actor("eu-west-1").unwrap()
        ));
    }

    #[test]
    fn a_key_that_could_name_another_directory_is_refused() {
        let k = keyed();
        for bad in ["", "..", "a/b", "US-EAST-1", "a b", &"x".repeat(33)] {
            assert_eq!(k.actor(bad).err(), Some(KeyError::Unsafe), "{bad:?}");
        }
        for n in 0..MAX_KEYS {
            k.actor(&format!("r-{n}")).unwrap();
        }
        assert_eq!(k.actor("one-more").err(), Some(KeyError::TooMany));
    }
}
