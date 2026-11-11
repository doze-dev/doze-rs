//! The typed keys and the `Operation` enum: constants only, so a member costs no code.

use super::model::{
    K_BLOB, K_BOOL, K_FLOAT, K_INT, K_LIST, K_MAP, K_STR, K_STRUCT, K_TS, K_UNION, Model,
};
use anyhow::Result;
use std::fmt::Write as _;

impl Model {
    pub(crate) fn counts(&self) -> String {
        format!(
            "shapes {} members {} cons {} enums {} ops {} errs {} hand-patterns {}",
            self.shapes.len(),
            self.members.len(),
            self.cons.len(),
            self.enums.len(),
            self.ops.len(),
            self.errs.len(),
            self.hand_patterns.len()
        )
    }

    /// Typed key constants: one marker type per struct and one `Key` per member, for the thin views.
    pub(crate) fn emit_keys(&self) -> Result<String> {
        let mut o =
            String::from("use aws_schema::view::kind::*;\nuse aws_schema::view::{Key, Shape};\n\n");
        for (i, s) in self.shapes.iter().enumerate() {
            let ty = type_ident(&s.name);
            let _ = writeln!(
                o,
                "#[derive(Clone, Copy, Debug)]\npub struct {ty};\nimpl Shape for {ty} {{ const INDEX: u32 = {i}; }}\nimpl {ty} {{"
            );
            let mut seen = std::collections::HashSet::new();
            for k in 0..s.count {
                let m = &self.members[(s.first + k) as usize];
                let cname = const_ident(&m.name);
                if !seen.insert(cname.clone()) {
                    continue;
                }
                let kt = match m.kind {
                    K_BOOL => "Bool".to_owned(),
                    K_INT => "Int".to_owned(),
                    K_FLOAT => "Float".to_owned(),
                    K_STR => "Str".to_owned(),
                    K_BLOB => "Blob".to_owned(),
                    K_TS => "Time".to_owned(),
                    K_STRUCT | K_UNION => {
                        format!("Obj<{}>", type_ident(&self.shapes[m.target as usize].name))
                    }
                    K_LIST => format!("List<{}>", self.elem_kind(m.target)),
                    K_MAP => format!("Map<{}>", self.elem_kind(m.target + 1)),
                    _ => "Other".to_owned(),
                };
                let _ = writeln!(
                    o,
                    "    pub const {cname}: Key<{ty}, {kt}> = Key::new({});",
                    s.first + k
                );
            }
            o.push_str("}\n");
        }
        Ok(o)
    }

    fn elem_kind(&self, mi: u32) -> String {
        let m = &self.members[mi as usize];
        match m.kind {
            K_BOOL => "Bool".to_owned(),
            K_INT => "Int".to_owned(),
            K_FLOAT => "Float".to_owned(),
            K_STR => "Str".to_owned(),
            K_BLOB => "Blob".to_owned(),
            K_TS => "Time".to_owned(),
            K_STRUCT | K_UNION => {
                format!("Obj<{}>", type_ident(&self.shapes[m.target as usize].name))
            }
            K_LIST => format!("List<{}>", self.elem_kind(m.target)),
            K_MAP => format!("Map<{}>", self.elem_kind(m.target + 1)),
            _ => "Other".to_owned(),
        }
    }

    pub(crate) fn emit_ops(&self) -> String {
        let mut o = String::from(
            "/// Every operation of the model, implemented or not: an unimplemented one answers 501 by name.\n#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n#[non_exhaustive]\npub enum Operation {\n",
        );
        for op in &self.ops {
            let _ = writeln!(o, "    {},", type_ident(&op.name));
        }
        o.push_str("}\nimpl Operation {\n    pub const ALL: &[Operation] = &[\n");
        for op in &self.ops {
            let _ = writeln!(o, "        Operation::{},", type_ident(&op.name));
        }
        o.push_str("    ];\n    pub const fn index(self) -> u32 { self as u32 }\n}\n");
        o
    }
}

fn type_ident(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if t.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{t}")
    } else {
        t
    }
}

fn const_ident(s: &str) -> String {
    let mut o = String::new();
    let cs: Vec<char> = s.chars().collect();
    for (i, c) in cs.iter().enumerate() {
        if !c.is_ascii_alphanumeric() {
            o.push('_');
            continue;
        }
        if c.is_ascii_uppercase()
            && i > 0
            && (cs[i - 1].is_ascii_lowercase()
                || cs[i - 1].is_ascii_digit()
                || cs.get(i + 1).is_some_and(char::is_ascii_lowercase)
                    && cs[i - 1].is_ascii_uppercase())
        {
            o.push('_');
        }
        o.push(c.to_ascii_uppercase());
    }
    if o.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        format!("_{o}")
    } else {
        o
    }
}
