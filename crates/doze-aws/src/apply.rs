//! `doze-aws apply`: a template becomes a stack on the running doze-aws through its own CloudFormation (`CreateStack`, or a change
//! set and `ExecuteChangeSet` when the stack is there), with the stack's events followed as they happen.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::cfn::{Cfn, CfnError};
use crate::cli::ApplyArgs;
use crate::config::Config;
use crate::events::{Follow, Outcome};
use crate::style::{Stream, Style};

/// The templates looked for when none is named, in order.
pub(crate) const DEFAULT_TEMPLATES: [&str; 4] = [
    "template.yaml",
    "template.yml",
    "template.json",
    "cloudformation.yaml",
];

/// What to apply.
#[derive(Debug)]
pub(crate) struct Plan {
    pub(crate) file: PathBuf,
    pub(crate) stack: String,
    pub(crate) params: Vec<(String, String)>,
}

/// Runs the command; the exit code.
pub(crate) fn run(args: ApplyArgs, cfg: &Config) -> i32 {
    let Some(target) = crate::locate::find(cfg) else {
        eprintln!("{}", crate::locate::not_running(cfg));
        return 1;
    };
    let cwd = std::env::current_dir().unwrap_or_default();
    let plan = match plan_of(args, &cwd) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("doze-aws apply: {e}");
            return 2;
        }
    };
    match target.client() {
        Ok(client) => apply(&Cfn::new(client), &plan, Style::of(Stream::Out)),
        Err(e) => {
            eprintln!("doze-aws apply: {e}");
            1
        }
    }
}

fn plan_of(args: ApplyArgs, cwd: &Path) -> Result<Plan, String> {
    let file = match args.template {
        Some(f) => f,
        None => DEFAULT_TEMPLATES
            .iter()
            .map(|n| cwd.join(n))
            .find(|p| p.is_file())
            .ok_or_else(|| {
                format!(
                    "no template given, and none of {} is here",
                    DEFAULT_TEMPLATES.join(", ")
                )
            })?,
    };
    let mut params = Vec::new();
    for p in &args.params {
        let (k, v) = p
            .split_once('=')
            .ok_or_else(|| format!("--param {p}: give it as KEY=VALUE"))?;
        params.push((k.to_owned(), v.to_owned()));
    }
    let stack = match args.stack {
        Some(s) => s,
        None => stack_name(cwd),
    };
    Ok(Plan {
        file,
        stack,
        params,
    })
}

/// A stack name from a directory's name: CloudFormation's letters, digits and dashes, starting with a letter.
pub(crate) fn stack_name(dir: &Path) -> String {
    let base = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut name: String = base
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    name = name.trim_matches('-').to_owned();
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        name = format!("stack-{name}").trim_end_matches('-').to_owned();
    }
    name.chars().take(128).collect()
}

/// Applies the plan; the exit code. Shared by the command and `--template` at start.
pub(crate) fn apply(cfn: &Cfn, plan: &Plan, style: Style) -> i32 {
    let body = match std::fs::read_to_string(&plan.file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("doze-aws apply: {}: {e}", plan.file.display());
            return 1;
        }
    };
    match converge(cfn, plan, &body, style) {
        Ok(Some(outcome)) => outcome.report(cfn, &plan.stack, style),
        Ok(None) => 0,
        Err(e) => {
            println!("\n  {} {}\n", style.red("✗"), style.bold(&e.to_string()));
            1
        }
    }
}

/// Creates or updates the stack and follows it; `None` when there was nothing to change.
fn converge(cfn: &Cfn, plan: &Plan, body: &str, style: Style) -> Result<Option<Outcome>, CfnError> {
    let name = &plan.stack;
    let file = file_name(plan);
    let mut existing = cfn.stack(name)?;
    if let Some(s) = existing
        .as_ref()
        .filter(|s| s.status == "ROLLBACK_COMPLETE" || s.status == "REVIEW_IN_PROGRESS")
    {
        // A stack whose first create rolled back holds nothing and cannot be updated: AWS makes you delete it. That is done here.
        println!(
            "  {} was left in {} by an earlier attempt; deleting it first.",
            style.bold(name),
            s.status
        );
        cfn.delete_stack(name)?;
        wait_gone(cfn, name)?;
        existing = None;
    }
    let given: Vec<(String, Option<String>)> = plan
        .params
        .iter()
        .map(|(k, v)| (k.clone(), Some(v.clone())))
        .collect();
    let Some(stack) = existing else {
        println!("\n  Creating {} from {file}\n", style.bold(name));
        let follow = Follow::start(cfn, name, style)?;
        cfn.create_stack(name, body, &given)?;
        return follow.until_settled(cfn).map(Some);
    };
    update(cfn, plan, body, &stack, given, style)
}

/// Updates the stack through a change set (parameters not given keep their values), showing what will change first.
fn update(
    cfn: &Cfn,
    plan: &Plan,
    body: &str,
    stack: &crate::cfn::Stack,
    given: Vec<(String, Option<String>)>,
    style: Style,
) -> Result<Option<Outcome>, CfnError> {
    let (name, file) = (&plan.stack, file_name(plan));
    let declared = cfn.declared_params(body)?;
    let mut params = given;
    for key in declared.iter().filter(|k| stack.params.contains(k)) {
        if !params.iter().any(|(k, _)| k == key) {
            params.push((key.clone(), None));
        }
    }
    let set = format!("doze-apply-{}", std::process::id());
    let id = cfn.create_change_set(name, &set, body, &params)?;
    let changes = wait_change_set(cfn, &id)?;
    if changes.status == "FAILED" {
        let _ = cfn.delete_change_set(&id);
        if changes.reason.contains("didn't contain changes") {
            println!(
                "\n  {} {} is up to date with {file}: nothing to change.\n",
                style.green("✓"),
                style.bold(name)
            );
            return Ok(None);
        }
        return Err(CfnError::Api {
            code: "ChangeSetFailed".into(),
            message: changes.reason,
        });
    }
    println!("\n  Updating {} from {file}\n", style.bold(name));
    print_changes(&changes.changes, style);
    let follow = Follow::start(cfn, name, style)?;
    cfn.execute_change_set(&id)?;
    follow.until_settled(cfn).map(Some)
}

fn print_changes(changes: &[crate::cfn::Change], style: Style) {
    for c in changes {
        let (mark, color): (&str, fn(Style, &str) -> String) = match c.action.as_str() {
            "Add" => ("+", Style::green),
            "Remove" => ("-", Style::red),
            _ => ("~", Style::yellow),
        };
        let replace = if c.replacement == "True" {
            "  (replaced)"
        } else {
            ""
        };
        println!(
            "    {} {:<28} {}{}",
            color(style, mark),
            c.logical,
            style.dim(&c.kind),
            replace
        );
    }
    println!();
}

fn file_name(plan: &Plan) -> String {
    plan.file
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn wait_change_set(cfn: &Cfn, id: &str) -> Result<crate::cfn::ChangeSet, CfnError> {
    loop {
        let cs = cfn.change_set(id)?;
        if !cs.status.ends_with("PENDING") && !cs.status.ends_with("IN_PROGRESS") {
            return Ok(cs);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

fn wait_gone(cfn: &Cfn, name: &str) -> Result<(), CfnError> {
    loop {
        match cfn.stack(name)? {
            None => return Ok(()),
            Some(s) if s.status == "DELETE_COMPLETE" => return Ok(()),
            Some(s) if s.status == "DELETE_FAILED" => {
                return Err(CfnError::Api {
                    code: s.status,
                    message: s.reason,
                });
            }
            Some(_) => std::thread::sleep(Duration::from_millis(150)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stack_is_named_after_its_directory_as_cloudformation_allows() {
        assert_eq!(stack_name(Path::new("/w/orders_api")), "orders-api");
        assert_eq!(stack_name(Path::new("/w/2024 app")), "stack-2024-app");
        assert_eq!(stack_name(Path::new("/w/.hidden")), "hidden");
    }

    #[test]
    fn a_param_needs_key_equals_value() {
        let args = |p: &str| ApplyArgs {
            template: Some("t.yaml".into()),
            stack: None,
            params: vec![p.into()],
        };
        assert!(
            plan_of(args("A=b=c"), Path::new("/x/y")).unwrap().params
                == vec![("A".into(), "b=c".into())]
        );
        assert!(
            plan_of(args("A"), Path::new("/x/y"))
                .unwrap_err()
                .contains("KEY=VALUE")
        );
    }
}
