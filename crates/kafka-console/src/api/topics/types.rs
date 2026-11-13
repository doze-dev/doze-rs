//! The TypeScript types of the Topics API's answers, for `api::types`.

use std::collections::BTreeMap;

use super::*;
use crate::api::types::one;
use crate::log_ruler::{Cell, Key, Mark, Ruler, RulerCell};
use crate::log_words::{Expiry, Rule, RuleTone};
use crate::model::{
    CallVersions, HeaderRow, Hold, Kind, LogCleaner, LogCompaction, LogDurability, LogLimit,
    LogMoved, LogOther, LogOtherLink, LogPass, LogPlanner, LogRetentionView, LogRound, LogRules,
    LogSegment, LogState, LogStop, LogTombstone, LogView, OpenTxn, PartitionRow, ProdAborted,
    ProdBatch, ProdCell, ProdRow, ProdTail, ProdTxn, ProducersView, Reach, Reading, RecordRow,
    RecordTxn, Shown, Standing, TabCounts, TopicRow, TxnPart, TxnRow, TxnView,
};
use ts_rs::Config;

pub(crate) fn all(cfg: &Config, out: &mut BTreeMap<String, String>) {
    one::<TopicsResponse>(cfg, out);
    one::<Missing>(cfg, out);
    one::<TopicActs>(cfg, out);
    one::<TopicFrame>(cfg, out);
    one::<MessagesResponse>(cfg, out);
    one::<ReaderPartition>(cfg, out);
    one::<Reader>(cfg, out);
    one::<ConsumersResponse>(cfg, out);
    one::<LogToCome>(cfg, out);
    one::<CompactDialog>(cfg, out);
    one::<LogResponse>(cfg, out);
    one::<CompactDone>(cfg, out);
    one::<ProducersResponse>(cfg, out);
    one::<TxnResponse>(cfg, out);
    one::<TailRows>(cfg, out);
    one::<Cleanup>(cfg, out);
    one::<TopicRow>(cfg, out);
    one::<TopicsState>(cfg, out);
    one::<TopicsView>(cfg, out);
    one::<PartitionRow>(cfg, out);
    one::<OpenTxn>(cfg, out);
    one::<TabCounts>(cfg, out);
    one::<CallVersions>(cfg, out);
    one::<TopicHead>(cfg, out);
    one::<Standing>(cfg, out);
    one::<Kind>(cfg, out);
    one::<Shown>(cfg, out);
    one::<RecordRow>(cfg, out);
    one::<Hold>(cfg, out);
    one::<Reach>(cfg, out);
    one::<MessagesView>(cfg, out);
    one::<HeaderRow>(cfg, out);
    one::<RecordTxn>(cfg, out);
    one::<RecordView>(cfg, out);
    one::<Reading>(cfg, out);
    one::<MessageQuery>(cfg, out);
    log_and_producers(cfg, out);
}

/// The Log, Producers and transaction tabs' own types.
fn log_and_producers(cfg: &Config, out: &mut BTreeMap<String, String>) {
    one::<LogSegment>(cfg, out);
    one::<LogTombstone>(cfg, out);
    one::<LogLimit>(cfg, out);
    one::<LogCleaner>(cfg, out);
    one::<LogRound>(cfg, out);
    one::<LogRules>(cfg, out);
    one::<LogPlanner>(cfg, out);
    one::<LogCompaction>(cfg, out);
    one::<LogMoved>(cfg, out);
    one::<LogPass>(cfg, out);
    one::<LogRetentionView>(cfg, out);
    one::<LogStop>(cfg, out);
    one::<LogDurability>(cfg, out);
    one::<LogOtherLink>(cfg, out);
    one::<LogOther>(cfg, out);
    one::<LogState>(cfg, out);
    one::<LogView>(cfg, out);
    one::<ProdBatch>(cfg, out);
    one::<ProdTxn>(cfg, out);
    one::<ProdRow>(cfg, out);
    one::<ProdAborted>(cfg, out);
    one::<ProdCell>(cfg, out);
    one::<ProdTail>(cfg, out);
    one::<ProducersView>(cfg, out);
    one::<TxnRow>(cfg, out);
    one::<TxnView>(cfg, out);
    one::<TxnPart>(cfg, out);
    one::<Ruler>(cfg, out);
    one::<Mark>(cfg, out);
    one::<RulerCell>(cfg, out);
    one::<Cell>(cfg, out);
    one::<Key>(cfg, out);
    one::<Expiry>(cfg, out);
    one::<Rule>(cfg, out);
    one::<RuleTone>(cfg, out);
}
