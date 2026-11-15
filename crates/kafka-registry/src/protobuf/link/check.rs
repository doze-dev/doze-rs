//! The rules `protoc` holds a schema to, beyond its grammar: field numbers, labels per syntax, reserved numbers and
//! names, enum values, map keys, JSON names, and options (the built-in ones by name and value type, custom ones by
//! the extension they name). Only the schema being registered is checked; its references were checked when they were.

use super::super::Error;
use super::super::ast::{Decl, Enum, EnumItem, Field, Item, Label, MAX_FIELD, Opt, Val};
use super::{Card, Fld, Kind, Linker, Ty, Unit, json_name, ranges_of};

/// The built-in options: where each may be written (F file, M message, D field, E enum, V enum value, S service,
/// R method), its name and its value (s string, b bool, or the names of an enum).
const OPTION_TABLE: &str = "F java_package s;F java_outer_classname s;F java_multiple_files b;\
F java_generate_equals_and_hash b;F java_string_check_utf8 b;F optimize_for SPEED,CODE_SIZE,LITE_RUNTIME;\
F go_package s;F cc_generic_services b;F java_generic_services b;F py_generic_services b;F php_generic_services b;\
F deprecated b;F cc_enable_arenas b;F objc_class_prefix s;F csharp_namespace s;F swift_prefix s;\
F php_class_prefix s;F php_namespace s;F php_metadata_namespace s;F ruby_package s;\
M message_set_wire_format b;M no_standard_descriptor_accessor b;M deprecated b;M map_entry b;\
M deprecated_legacy_json_field_conflicts b;D ctype STRING,CORD,STRING_PIECE;D packed b;\
D jstype JS_NORMAL,JS_STRING,JS_NUMBER;D lazy b;D unverified_lazy b;D deprecated b;D weak b;D debug_redact b;\
D retention RETENTION_UNKNOWN,RETENTION_RUNTIME,RETENTION_SOURCE;E allow_alias b;E deprecated b;\
E deprecated_legacy_json_field_conflicts b;V deprecated b;V debug_redact b;S deprecated b;R deprecated b;\
R idempotency_level IDEMPOTENCY_UNKNOWN,NO_SIDE_EFFECTS,IDEMPOTENT";

/// Which options message a context's options belong to, for the messages.
fn options_message(ctx: &str) -> &'static str {
    match ctx {
        "F" => "FileOptions",
        "M" => "MessageOptions",
        "D" => "FieldOptions",
        "E" => "EnumOptions",
        "V" => "EnumValueOptions",
        "S" => "ServiceOptions",
        "R" => "MethodOptions",
        "O" => "OneofOptions",
        _ => "ExtensionRangeOptions",
    }
}

fn is_bool(v: &Val) -> bool {
    matches!(v, Val::Ident(b) if b == "true" || b == "false")
}

impl Linker<'_> {
    pub(super) fn check_number(&self, n: i64, at: u32) -> Result<(), Error> {
        if !(1..=MAX_FIELD).contains(&n) {
            return Err(Error::at(
                at,
                format!("field number {n} is out of range 1 to {MAX_FIELD}"),
            ));
        }
        if (19000..=19999).contains(&n) {
            return Err(Error::at(
                at,
                format!("field number {n} is reserved for the implementation (19000 to 19999)"),
            ));
        }
        Ok(())
    }

    /// Options written somewhere `ctx` names (a letter of [`OPTION_TABLE`]), seen from `scope`.
    pub(super) fn check_options(
        &self,
        u: &Unit<'_>,
        opts: &[Opt],
        ctx: &str,
        scope: &str,
    ) -> Result<(), Error> {
        for o in opts {
            if let Some((name, true)) = o.name.first() {
                if self
                    .resolve(name, scope, u.file, |k| k == Kind::Extension)
                    .is_none()
                {
                    return Err(Error::at(
                        o.at,
                        format!("option ({name}): unknown extension"),
                    ));
                }
                continue;
            }
            let name = o.simple().unwrap_or_default();
            let found = OPTION_TABLE.split(';').find_map(|e| {
                let mut w = e.split(' ');
                (w.next() == Some(ctx) && w.next() == Some(name))
                    .then(|| w.next().unwrap_or_default())
            });
            let ok = match found {
                None => {
                    return Err(Error::at(
                        o.at,
                        format!(
                            "option {}: google.protobuf.{} has no such option",
                            o.name
                                .iter()
                                .map(|(n, _)| n.as_str())
                                .collect::<Vec<_>>()
                                .join("."),
                            options_message(ctx)
                        ),
                    ));
                }
                Some("s") => matches!(o.val, Val::Str),
                Some("b") => is_bool(&o.val),
                Some(names) => matches!(&o.val, Val::Ident(v) if names.split(',').any(|n| n == v)),
            };
            if !ok {
                return Err(Error::at(
                    o.at,
                    format!("option {name}: value of the wrong type"),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn check_file_options(&self, u: &Unit<'_>, package: &str) -> Result<(), Error> {
        for d in &u.ast.decls {
            if let Decl::Option(o) = d {
                self.check_options(u, std::slice::from_ref(o), "F", package)?;
            }
        }
        Ok(())
    }

    pub(super) fn check_field(
        &self,
        u: &Unit<'_>,
        f: &Field,
        fld: &Fld,
        scope: &str,
    ) -> Result<(), Error> {
        let proto3 = self.proto3(u.file);
        let named = |m: &str| Error::at(f.at, format!("field {scope}.{}: {m}", f.name));
        if proto3 && f.label == Some(Label::Required) {
            return Err(named("required fields are not allowed in proto3"));
        }
        let is_map = matches!(f.ty, super::TypeRef::Map(..));
        if !proto3 && f.label.is_none() && fld.oneof.is_none() && !is_map {
            return Err(named(
                "a label (optional, required or repeated) is required in proto2",
            ));
        }
        self.check_number(f.number, f.at)?;
        if let Ty::Enum(e) = fld.ty
            && proto3
            && self.pool.enums.get(e).is_some_and(|e| !self.proto3(e.file))
        {
            return Err(named("a proto3 message cannot use a proto2 enum"));
        }
        let mut rest = Vec::new();
        for o in &f.opts {
            match o.simple() {
                Some("default") => self
                    .check_default(u, o, fld, proto3)
                    .map_err(|m| named(&m))?,
                Some("json_name") if !matches!(o.val, Val::Str) => {
                    return Err(named("json_name must be a string"));
                }
                Some("json_name") => {}
                Some("packed") => {
                    let packable = fld.card == Card::Repeated
                        && matches!(fld.ty, Ty::Enum(_) | Ty::Scalar(_))
                        && !matches!(fld.ty, Ty::Scalar(9 | 12))
                        && !is_map;
                    if !packable {
                        return Err(named(
                            "packed is only for repeated fields of numbers, bools or enums",
                        ));
                    }
                    rest.push(o);
                }
                _ => rest.push(o),
            }
        }
        for o in rest {
            self.check_options(u, std::slice::from_ref(o), "D", scope)?;
        }
        Ok(())
    }

    fn check_default(&self, _u: &Unit<'_>, o: &Opt, fld: &Fld, proto3: bool) -> Result<(), String> {
        if proto3 {
            return Err("default values are not allowed in proto3".into());
        }
        let ok = match fld.ty {
            _ if fld.card == Card::Repeated => false,
            Ty::Message(_) | Ty::Group(_) => false,
            Ty::Scalar(9 | 12) => matches!(o.val, Val::Str),
            Ty::Scalar(8) => is_bool(&o.val),
            Ty::Scalar(1 | 2) => {
                matches!(&o.val, Val::Num(..))
                    || matches!(&o.val, Val::Ident(v) if v == "inf" || v == "nan")
            }
            Ty::Scalar(t) => {
                let unsigned = matches!(t, 4 | 6 | 7 | 13);
                matches!(&o.val, Val::Num(n, false) if !(unsigned && n.starts_with('-')))
            }
            Ty::Enum(e) => {
                matches!(&o.val, Val::Ident(v) if self.pool.enums.get(e).is_some_and(|e| e.values.contains(v)))
            }
        };
        if ok {
            Ok(())
        } else {
            Err("the default value does not fit the field".into())
        }
    }

    pub(super) fn check_message(
        &self,
        u: &Unit<'_>,
        i: usize,
        items: &[Item],
        at: u32,
    ) -> Result<(), Error> {
        let Some(m) = self.pool.msg(i) else {
            return Ok(());
        };
        let mut reserved = Vec::new();
        let mut names = Vec::new();
        for item in items {
            if let Item::Reserved(r) = item {
                for (a, b) in ranges_of(r, MAX_FIELD) {
                    if a < 1 || a > b || b > MAX_FIELD {
                        return Err(Error::at(
                            r.at,
                            format!("reserved range {a} to {b} is not valid"),
                        ));
                    }
                    reserved.push((a, b));
                }
                names.extend(r.names.iter());
            }
        }
        let proto3 = self.proto3(u.file);
        for (n, f) in m.fields.iter().enumerate() {
            let num = i64::from(f.number);
            let clash = |what: String| {
                Error::at(at, format!("message {}: field {} {what}", m.full, f.name))
            };
            if let Some(g) = m.fields.iter().take(n).find(|g| g.number == f.number) {
                return Err(clash(format!(
                    "uses number {} already used by {}",
                    f.number, g.name
                )));
            }
            if reserved.iter().any(|&(a, b)| a <= num && num <= b) {
                return Err(clash(format!("uses reserved number {num}")));
            }
            if names.iter().any(|r| **r == f.name) {
                return Err(clash("uses a reserved name".into()));
            }
            if m.ext_ranges.iter().any(|&(a, b)| a <= num && num <= b) {
                return Err(clash(format!("uses number {num}, in an extension range")));
            }
            if proto3
                && let Some(g) = m
                    .fields
                    .iter()
                    .take(n)
                    .find(|g| json_name(&g.name) == json_name(&f.name))
            {
                return Err(clash(format!("has the JSON name of field {}", g.name)));
            }
        }
        Ok(())
    }

    pub(super) fn check_enum(&self, u: &Unit<'_>, e: &Enum, _i: usize) -> Result<(), Error> {
        let mut values: Vec<(&str, i64, u32)> = Vec::new();
        let mut reserved = Vec::new();
        let mut names = Vec::new();
        let mut alias = false;
        for d in &e.body {
            match d {
                EnumItem::Value(v) => {
                    if i32::try_from(v.number).is_err() {
                        return Err(Error::at(
                            v.at,
                            format!("enum value {} is out of range", v.name),
                        ));
                    }
                    self.check_options(u, &v.opts, "V", &e.name)?;
                    values.push((&v.name, v.number, v.at));
                }
                EnumItem::Reserved(r) => {
                    reserved.extend(ranges_of(r, i64::from(i32::MAX)));
                    names.extend(r.names.iter());
                }
                EnumItem::Option(o) => {
                    alias |= o.simple() == Some("allow_alias")
                        && matches!(&o.val, Val::Ident(v) if v == "true");
                    self.check_options(u, std::slice::from_ref(o), "E", &e.name)?;
                }
            }
        }
        let named = |m: &str| Error::at(e.at, format!("enum {}: {m}", e.name));
        let Some(&(_, first, _)) = values.first() else {
            return Err(named("an enum needs at least one value"));
        };
        if self.proto3(u.file) && first != 0 {
            return Err(named("the first value of a proto3 enum must be zero"));
        }
        let mut aliased = false;
        for (n, &(name, num, at)) in values.iter().enumerate() {
            if values.iter().take(n).any(|v| v.1 == num) {
                if !alias {
                    return Err(Error::at(
                        at,
                        format!(
                            "enum value {name} reuses number {num}; set allow_alias to allow it"
                        ),
                    ));
                }
                aliased = true;
            }
            if reserved.iter().any(|&(a, b)| a <= num && num <= b)
                || names.iter().any(|r| *r == name)
            {
                return Err(Error::at(at, format!("enum value {name} is reserved")));
            }
        }
        if alias && !aliased {
            return Err(named("allow_alias is set but no two values share a number"));
        }
        Ok(())
    }
}
