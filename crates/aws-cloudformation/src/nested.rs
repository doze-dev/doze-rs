//! Nested stacks (`AWS::CloudFormation::Stack`). Written in milestone 3.

use crate::core::Core;

/// What a job asks of the actor for a nested stack.
#[derive(Debug)]
pub(crate) struct Ask;

impl Core {
    pub(crate) async fn child(&mut self, _ask: Ask) -> Result<String, String> {
        Err("nested stacks are not built yet".into())
    }
}
