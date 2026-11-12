//! OpenAPI import and export for REST APIs.

use crate::core::Core;
use crate::errors::refused;
use crate::req::{Req, Res};

pub(crate) fn import(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("ImportRestApi"))
}

pub(crate) fn put(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("PutRestApi"))
}

pub(crate) fn export(_core: &mut Core, _r: &Req) -> Res {
    Err(refused("GetExport"))
}
