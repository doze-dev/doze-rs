//! `/gallery`: the component gallery (the examples build only; see `ui::components::gallery`).

use axum::extract::{RawQuery, State};
use axum::response::Response;

use super::html;
use crate::state::Shared;
use crate::ui::components::gallery::{Chosen, page};
use crate::ui::shell::Rail;

/// `GET /gallery?partition=&topic=&view=`
pub async fn gallery(State(s): State<Shared>, RawQuery(query): RawQuery) -> Response {
    let chosen = Chosen::from_query(query.as_deref().unwrap_or(""));
    html(
        &s.document(
            Rail::Lab,
            "Component gallery",
            &[("Lab", Some("/lab")), ("Component gallery", None)],
            &[],
            page(&chosen, &s.base),
        )
        .await,
    )
}
