//! The variables an expression reads (hclsyntax's `Variables`): every `root.step.step` traversal
//! that is not one of a `for`'s own names, with its range. doze reads references from these.

use crate::body::Body;
use doze_hcl::diag::Range;
use doze_hcl::syntax::Source;
use hcl_edit::Span;
use hcl_edit::expr::{Expression as E, TraversalOperator as Op};
use hcl_edit::structure::Body as Syntax;
use hcl_edit::template::{Directive, Element, Template};

/// One step after the root.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Attr(String),
    /// A literal index (`[0]`, `["k"]`, `.0`).
    Index(String),
}

/// A traversal from a root variable.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Traversal {
    pub root: String,
    pub steps: Vec<Step>,
    /// From the root through the last step (Go's `SourceRange`).
    pub range: Range,
}

impl Traversal {
    /// The name in `kind.name…` (Go's `traversalName`).
    pub(crate) fn name(&self) -> Option<&str> {
        match self.steps.first() {
            Some(Step::Attr(n)) => Some(n),
            _ => None,
        }
    }

    /// The attribute in `kind.name.attr…`.
    pub(crate) fn attr(&self) -> Option<&str> {
        match self.steps.get(1) {
            Some(Step::Attr(n)) => Some(n),
            _ => None,
        }
    }
}

/// The traversals `e` reads.
pub(crate) fn variables(e: &E, src: &Source) -> Vec<Traversal> {
    let mut w = Walker {
        src,
        locals: Vec::new(),
        out: Vec::new(),
    };
    w.expr(e);
    w.out
}

/// Go's `referencedTraversals`: every attribute's traversals under `body` (nested blocks too, and
/// what a schema already took), except the body's own `after`.
pub(crate) fn referenced(body: &Body<'_>) -> Vec<Traversal> {
    let mut out = Vec::new();
    for (syntax, src) in body.syntax_parts() {
        walk_body(syntax, src, true, &mut out);
    }
    out
}

/// Every traversal of every attribute in `syntax`, in source order.
pub(crate) fn walk_body(syntax: &Syntax, src: &Source, top: bool, out: &mut Vec<Traversal>) {
    for a in syntax.attributes() {
        if top && a.key.as_str() == "after" {
            continue;
        }
        out.extend(variables(&a.value, src));
    }
    for b in syntax.blocks() {
        walk_body(&b.body, src, false, out);
    }
}

struct Walker<'s> {
    src: &'s Source,
    locals: Vec<String>,
    out: Vec<Traversal>,
}

impl Walker<'_> {
    fn range(&self, span: Option<std::ops::Range<usize>>) -> Range {
        span.map(|s| self.src.range(s)).unwrap_or_default()
    }

    fn expr(&mut self, e: &E) {
        match e {
            E::Variable(v) => self.root(v.as_str(), v.span(), &[]),
            E::Traversal(t) => self.traversal(t),
            E::Array(a) => a.iter().for_each(|x| self.expr(x)),
            E::Object(o) => {
                for (k, v) in o.iter() {
                    if let hcl_edit::expr::ObjectKey::Expression(k) = k {
                        self.expr(k);
                    }
                    self.expr(v.expr());
                }
            }
            E::StringTemplate(t) => self.template(t),
            E::HeredocTemplate(h) => self.template(&h.template),
            E::Parenthesis(p) => self.expr(p.inner()),
            E::Conditional(c) => {
                self.expr(&c.cond_expr);
                self.expr(&c.true_expr);
                self.expr(&c.false_expr);
            }
            E::FuncCall(f) => f.args.iter().for_each(|x| self.expr(x)),
            E::UnaryOp(u) => self.expr(&u.expr),
            E::BinaryOp(b) => {
                self.expr(&b.lhs_expr);
                self.expr(&b.rhs_expr);
            }
            E::ForExpr(f) => self.for_expr(f),
            E::Null(_) | E::Bool(_) | E::Number(_) | E::String(_) => {}
        }
    }

    fn for_expr(&mut self, f: &hcl_edit::expr::ForExpr) {
        self.expr(&f.intro.collection_expr);
        let names = for_names(f.intro.key_var.as_ref(), &f.intro.value_var);
        let depth = self.locals.len();
        self.locals.extend(names);
        if let Some(k) = &f.key_expr {
            self.expr(k);
        }
        self.expr(&f.value_expr);
        if let Some(c) = &f.cond {
            self.expr(&c.expr);
        }
        self.locals.truncate(depth);
    }

    fn template(&mut self, t: &Template) {
        for el in t.iter() {
            match el {
                Element::Literal(_) => {}
                Element::Interpolation(i) => self.expr(&i.expr),
                Element::Directive(d) => match d.as_ref() {
                    Directive::If(i) => {
                        self.expr(&i.if_expr.cond_expr);
                        self.template(&i.if_expr.template);
                        if let Some(e) = &i.else_expr {
                            self.template(&e.template);
                        }
                    }
                    Directive::For(f) => {
                        let fe = &f.for_expr;
                        self.expr(&fe.collection_expr);
                        let depth = self.locals.len();
                        self.locals
                            .extend(for_names(fe.key_var.as_ref(), &fe.value_var));
                        self.template(&fe.template);
                        self.locals.truncate(depth);
                    }
                },
            }
        }
    }

    /// `root.a[0].b`: the literal steps belong to the traversal; a splat or a computed index
    /// ends it (and what follows is not a variable).
    fn traversal(&mut self, t: &hcl_edit::expr::Traversal) {
        let E::Variable(v) = &t.expr else {
            self.expr(&t.expr);
            for op in &t.operators {
                if let Op::Index(i) = op.value() {
                    self.expr(i);
                }
            }
            return;
        };
        let n = t
            .operators
            .iter()
            .take_while(|op| literal_step(op.value()).is_some())
            .count();
        self.root(v.as_str(), v.span(), t.operators.get(..n).unwrap_or(&[]));
        for op in t.operators.get(n..).unwrap_or(&[]) {
            if let Op::Index(i) = op.value() {
                self.expr(i);
            }
        }
    }

    fn root(
        &mut self,
        name: &str,
        span: Option<std::ops::Range<usize>>,
        ops: &[hcl_edit::Decorated<Op>],
    ) {
        if self.locals.iter().any(|l| l == name) {
            return;
        }
        let mut range = self.range(span);
        let steps = ops
            .iter()
            .filter_map(|op| {
                range = Range::between(range, self.range(op.span()));
                literal_step(op.value())
            })
            .collect();
        self.out.push(Traversal {
            root: name.to_owned(),
            steps,
            range,
        });
    }
}

fn for_names(
    k: Option<&hcl_edit::Decorated<hcl_edit::Ident>>,
    v: &hcl_edit::Decorated<hcl_edit::Ident>,
) -> Vec<String> {
    k.iter()
        .map(|k| k.as_str().to_owned())
        .chain(std::iter::once(v.as_str().to_owned()))
        .collect()
}

fn literal_step(op: &Op) -> Option<Step> {
    match op {
        Op::GetAttr(a) => Some(Step::Attr(a.as_str().to_owned())),
        Op::LegacyIndex(n) => Some(Step::Index(n.value().to_string())),
        Op::Index(E::Number(n)) => Some(Step::Index(n.to_string())),
        Op::Index(E::String(s)) => Some(Step::Index(s.value().clone())),
        _ => None,
    }
}
