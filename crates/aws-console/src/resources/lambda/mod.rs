//! Lambda: functions and layers. A function is created, deployed and configured from its own pages (the code goes in as a zip the
//! page builds); its triggers, URL, permissions and versions are the calls Lambda itself has for them.

use super::Descriptor;

mod functions;
mod layers;

pub(super) static LAMBDA: Descriptor = Descriptor {
    service: "lambda",
    target: "lambda",
    kinds: &[functions::FUNCTIONS, layers::LAYERS],
};
