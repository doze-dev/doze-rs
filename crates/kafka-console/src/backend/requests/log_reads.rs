//! The read requests of the Producers pages that no other page sends: every transaction the broker knows, whatever its state.

use kafka_wire::api::Request;
use kafka_wire::messages::ListTransactionsRequest;

/// ListTransactions with no filter: every transactional id the broker knows, by any state, for any producer.
#[must_use]
pub fn list_all_transactions() -> Request {
    Request::ListTransactions(ListTransactionsRequest::default())
}
