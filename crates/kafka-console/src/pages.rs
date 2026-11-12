//! The pages: each is `fn(&Model, &Base) -> Markup`, the content of `<main>` below the breadcrumb, written from the
//! board of its screen. The shell around it is `ui::shell`.

mod advertised;
mod common;
mod connect;
mod home;
mod notice;
pub mod topics;

pub use advertised::{advertised, probe_advertised};
pub use common::{PROBE_ADVERTISED, PROBE_CONNECT, PROBE_HOME, probe_fragment_url};
pub use connect::{connect, probe_connect};
pub use home::{
    HOME_CALLOUT, HOME_INSTANCE, HOME_SENTENCE, callout_fragment, home, instance_fragment,
    probe_home, sentence_fragment,
};
pub use notice::{not_found, planned};
