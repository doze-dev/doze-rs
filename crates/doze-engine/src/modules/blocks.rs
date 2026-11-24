//! The console's working blocks (protocol minor 2): part of one value, a tree folder's children. Each is
//! asked only of a module whose manifest declares its capability; the instance must have started, and nothing here
//! wakes it (the daemon wakes it first when the person opened a value).

use super::ModuleInstance;
use crate::stack::PlanCtx;
use doze_module_proto::method::Method;
use doze_module_proto::msg::blocks::{Children, ChildrenArgs, ValueAnswer, ValueArgs};
use doze_module_proto::msg::instance::Instance;
use std::collections::BTreeMap;

/// Which block a call is for: its call and how to say it is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    Value,
    Tree,
}

impl Block {
    fn method(self) -> Method {
        match self {
            Block::Value => Method::Value,
            Block::Tree => Method::Children,
        }
    }

    fn what(self) -> &'static str {
        match self {
            Block::Value => "a value viewer",
            Block::Tree => "a tree",
        }
    }
}

impl ModuleInstance {
    /// Whether its module offers `block`: the capability bit, and a protocol minor agreed with it that has the call (a
    /// 2.1 module with a stray bit is never woken for a block it cannot answer).
    pub async fn has_block(&self, block: Block) -> Result<bool, String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.describe())?;
        Ok(m.offers(block.method()))
    }

    /// The running module and the instance as it started, once `block` is known to be offered.
    async fn for_block(
        &self,
        ctx: &PlanCtx,
        block: Block,
    ) -> Result<(std::sync::Arc<doze_module_host::Running>, Instance), String> {
        let m = self.pool.get(&self.key).await.map_err(|e| e.describe())?;
        if !m.offers(block.method()) {
            return Err(format!("{} has no {}", self.base.engine, block.what()));
        }
        let i = self.with_toolchain(ctx).await?.instance;
        Ok((m, i))
    }

    /// Part of the value of `row` in `panel`, from byte `offset`.
    pub async fn value(
        &self,
        ctx: &PlanCtx,
        panel: &str,
        row: BTreeMap<String, String>,
        offset: u64,
        max: u32,
    ) -> Result<ValueAnswer, String> {
        let (m, instance) = self.for_block(ctx, Block::Value).await?;
        let a = ValueArgs {
            instance,
            panel: panel.into(),
            row,
            offset,
            max,
        };
        m.value(&a).await.map_err(|e| e.describe())
    }

    /// One page of the children of `parent` in tree panel `panel`.
    pub async fn children(
        &self,
        ctx: &PlanCtx,
        panel: &str,
        parent: &str,
        filter: &str,
        cursor: Option<String>,
    ) -> Result<Children, String> {
        let (m, instance) = self.for_block(ctx, Block::Tree).await?;
        let a = ChildrenArgs {
            instance,
            panel: panel.into(),
            parent: parent.into(),
            filter: filter.into(),
            cursor,
        };
        m.children(&a).await.map_err(|e| e.describe())
    }
}
