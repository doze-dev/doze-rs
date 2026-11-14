//! The needs graph of a stack: checked once when the engine is built (every need declared, no cycle), and the order
//! `up` starts things in.

use crate::error::BuildError;
use crate::stack::Stack;
use std::collections::BTreeSet;

/// Checks the stack: names unique, every need declared, no cycle (reported along its path).
pub fn check(stack: &Stack) -> Result<(), BuildError> {
    let mut seen = BTreeSet::new();
    for s in &stack.services {
        if !seen.insert(s.name.as_str()) {
            return Err(BuildError::Duplicate(s.name.clone()));
        }
    }
    for s in &stack.services {
        for n in &s.needs {
            if stack.service(&n.name).is_none() {
                return Err(BuildError::UnknownNeed {
                    service: s.name.clone(),
                    need: n.name.clone(),
                });
            }
        }
    }
    start_order(stack).map(drop)
}

/// Every service, each after what it needs, in declaration order otherwise.
pub fn start_order(stack: &Stack) -> Result<Vec<String>, BuildError> {
    let mut out = Vec::new();
    let mut path = Vec::new();
    for s in &stack.services {
        visit(stack, &s.name, &mut path, &mut out)?;
    }
    Ok(out)
}

fn visit(
    stack: &Stack,
    name: &str,
    path: &mut Vec<String>,
    out: &mut Vec<String>,
) -> Result<(), BuildError> {
    if out.iter().any(|n| n == name) {
        return Ok(());
    }
    if let Some(at) = path.iter().position(|n| n == name) {
        let mut cycle: Vec<String> = path.iter().skip(at).cloned().collect();
        cycle.push(name.to_string());
        return Err(BuildError::Cycle(cycle));
    }
    path.push(name.to_string());
    if let Some(s) = stack.service(name) {
        for n in &s.needs {
            visit(stack, &n.name, path, out)?;
        }
    }
    path.pop();
    out.push(name.to_string());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::ProcessSpec;
    use crate::stack::Service;

    fn stack(services: Vec<Service>) -> Stack {
        Stack {
            name: "t".into(),
            config_name: "doze.hcl".into(),
            data_dir: "/nonexistent".into(),
            idle_timeout: None,
            services,
        }
    }

    fn svc(name: &str) -> Service {
        Service::process(name, ProcessSpec::new(["x"], "."))
    }

    #[test]
    fn order_puts_needs_first() {
        let s = stack(vec![
            svc("web").needs("api"),
            svc("api").needs("db").needs("cache"),
            svc("db"),
            svc("cache"),
        ]);
        assert_eq!(start_order(&s).unwrap(), ["db", "cache", "api", "web"]);
        assert!(check(&s).is_ok());
    }

    #[test]
    fn cycles_unknowns_and_twins_are_refused() {
        let s = stack(vec![
            svc("a").needs("b"),
            svc("b").needs("c"),
            svc("c").needs("a"),
        ]);
        assert_eq!(
            check(&s),
            Err(BuildError::Cycle(vec![
                "a".into(),
                "b".into(),
                "c".into(),
                "a".into()
            ]))
        );
        let s = stack(vec![svc("a").needs("zz")]);
        assert_eq!(
            check(&s).err().map(|e| e.to_string()).unwrap_or_default(),
            "instance \"a\" depends on undeclared instance \"zz\""
        );
        let s = stack(vec![svc("a"), svc("a")]);
        assert_eq!(check(&s), Err(BuildError::Duplicate("a".into())));
    }
}
