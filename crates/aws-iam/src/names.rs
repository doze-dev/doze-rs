//! IAM does not tell names apart by case. The model says so of every name it takes ("Names are not distinguished by case. For example,
//! you cannot create resources named both MyResource and myresource"), and it is why `CreateUser DEV` is refused while `dev` exists.
//! The same holds for a lookup: `GetUser DEV` finds `dev`. A request that names an existing user, group or role in another case
//! is read as naming it by the spelling it was made with, so every handler after this sees one spelling.

use aws_model_iam::Operation as Op;

use crate::core::{Core, entity_key};
use crate::model::Kind;
use crate::req::Req;

impl Core {
    /// Puts the stored spelling in place of a name typed in another case. The operations that make a name keep what was typed.
    pub(crate) fn respell(&self, op: u32, r: &mut Req<'_>) {
        if matches!(
            Op::from_index(op),
            Some(Op::CreateUser | Op::CreateGroup | Op::CreateRole)
        ) {
            return;
        }
        for (member, kind) in [
            ("UserName", Kind::User),
            ("GroupName", Kind::Group),
            ("RoleName", Kind::Role),
        ] {
            let Some(typed) = r.opt(member) else {
                continue;
            };
            if self.entities.get(&entity_key(kind, typed)).is_some() {
                continue;
            }
            let prefix = entity_key(kind, "");
            let stored = self
                .entities
                .iter()
                .find(|(key, e)| key.starts_with(&prefix) && e.name.eq_ignore_ascii_case(typed))
                .map(|(_, e)| e.name.clone());
            if let Some(stored) = stored {
                r.set(member, stored);
            }
        }
    }
}
