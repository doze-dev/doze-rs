//! OpenAPI import and export for HTTP APIs.

use crate::core::Core;
use crate::errors::refused;
use crate::req::{Req, Res};

pub(crate) fn import(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("ImportApi"))
}

pub(crate) fn reimport(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("ReimportApi"))
}

pub(crate) fn export(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("ExportApi"))
}
