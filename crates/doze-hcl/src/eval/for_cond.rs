//! A `for` expression's `if` clause, per element.

use super::for_expr::Spec;
use super::{Context, Evaluator};
use crate::convert::convert;
use crate::diag::{Diagnostic, Diagnostics};
use crate::types::Type;
use crate::value::Value;

/// Whether an element is kept: `Ok(true)`, `Ok(false)`, or `Err` when the condition is null, not a
/// bool, or unknown (the result then becomes unknown; only the first such error is reported).
pub(super) enum Keep {
    Yes,
    No,
    Unknown,
}

impl Evaluator<'_> {
    /// The `if` clause for one element. `tuple_order` is Go's tuple form, which looks at unknown
    /// before trying the conversion; the object form converts first.
    pub(super) fn keep(
        &self,
        spec: &Spec<'_>,
        child: &Context<'_>,
        first: bool,
        tuple_order: bool,
        diags: &mut Diagnostics,
    ) -> Keep {
        let Some(cond) = spec.cond else {
            return Keep::Yes;
        };
        let (raw, d) = self.eval(cond, child);
        diags.extend(d);
        let at = self.range(cond);
        if raw.is_null() {
            if first {
                let d = Diagnostic::error(
                    "Invalid 'for' condition",
                    "The value of the 'if' clause must not be null.",
                    at,
                );
                diags.push(d.with_context(spec.whole));
            }
            return Keep::Unknown;
        }
        if tuple_order && !raw.is_known() {
            return Keep::Unknown;
        }
        match convert(&raw, &Type::Bool) {
            Err(e) => {
                if first {
                    let d = Diagnostic::error(
                        "Invalid 'for' condition",
                        format!("The 'if' clause value is invalid: {e}."),
                        at,
                    );
                    diags.push(d.with_context(spec.whole));
                }
                Keep::Unknown
            }
            Ok(Value::Bool(true)) => Keep::Yes,
            Ok(Value::Bool(false)) => Keep::No,
            Ok(_) => Keep::Unknown,
        }
    }
}
