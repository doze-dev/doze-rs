//! Whether a request fits a rule's matcher. Pure: a matcher and a view of the request in, a yes or no out.

use kafka_auth::Principal;
use kafka_wire::api::Request;
use kafka_wire::header::RequestHead;

use super::rule::Matcher;
use super::targets::{self, Names};

/// The request as a rule sees it.
#[derive(Clone, Copy)]
pub struct Ask<'a> {
    /// The connection it arrived on, when the server numbers its connections (it does with an observer installed).
    pub conn: Option<u64>,
    /// Who the connection is.
    pub principal: &'a Principal,
    /// The request's header.
    pub head: &'a RequestHead,
    /// The request, decoded; none for an ApiVersions request, which is answered from the profile.
    pub request: Option<&'a Request>,
    /// The names behind topic ids.
    pub names: Names<'a>,
}

impl std::fmt::Debug for Ask<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ask")
            .field("conn", &self.conn)
            .field("api", &self.head.key)
            .finish_non_exhaustive()
    }
}

impl Matcher {
    /// Whether `ask` fits every field that is set.
    #[must_use]
    pub fn fits(&self, ask: &Ask<'_>) -> bool {
        self.fits_header(ask) && self.fits_body(ask)
    }

    fn fits_header(&self, ask: &Ask<'_>) -> bool {
        if !self.apis.is_empty() && !self.apis.contains(&ask.head.key) {
            return false;
        }
        if let Some(limit) = self.up_to_conn
            && !ask.conn.is_some_and(|c| c <= limit)
        {
            return false;
        }
        if let Some(p) = &self.client
            && !p.fits(ask.head.client_id.as_ref().map_or("", |c| c.as_str()))
        {
            return false;
        }
        match &self.principal {
            Some(want) => ask.principal.as_str() == &**want,
            // The console's own principal is touched only by a rule that names it.
            None => !Principal::is_console(ask.principal.as_str()),
        }
    }

    fn fits_body(&self, ask: &Ask<'_>) -> bool {
        let Some(request) = ask.request else {
            return self.topics.is_empty() && self.partitions.is_empty() && self.group.is_none();
        };
        if let Some(g) = &self.group
            && !targets::group_of(request).is_some_and(|name| g.fits(&name))
        {
            return false;
        }
        if self.topics.is_empty() && self.partitions.is_empty() {
            return true;
        }
        targets::any_target(request, ask.names, &mut |topic, partition| {
            let topic_ok = self.topics.is_empty() || self.topics.iter().any(|t| t.fits(topic));
            let partition_ok = self.partitions.is_empty()
                || partition.is_some_and(|p| self.partitions.contains(&p));
            topic_ok && partition_ok
        })
    }
}

#[cfg(test)]
mod tests;
