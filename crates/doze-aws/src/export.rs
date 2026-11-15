//! `doze-aws export`: the template a stack on the running doze-aws was made from (`GetTemplate`), on standard output.
//!
//! The Go edition synthesised a template from whatever was running, through its own provisioning engine. That is not done here: a
//! template rebuilt from live resources loses what only the author wrote (parameters, conditions, intrinsics, SAM), invents logical
//! ids, and cannot say which resources belong together. The stack's own template is exact, and it is what `apply` sent.

use crate::cfn::Cfn;
use crate::cli::ExportArgs;
use crate::config::Config;

/// Runs the command; the exit code.
pub(crate) fn run(args: &ExportArgs, cfg: &Config) -> i32 {
    let Some(target) = crate::locate::find(cfg) else {
        eprintln!("{}", crate::locate::not_running(cfg));
        return 1;
    };
    let stack = args
        .stack
        .clone()
        .unwrap_or_else(|| crate::apply::stack_name(&std::env::current_dir().unwrap_or_default()));
    let cfn = match target.client() {
        Ok(c) => Cfn::new(c),
        Err(e) => {
            eprintln!("doze-aws export: {e}");
            return 1;
        }
    };
    match cfn.template(&stack, args.processed) {
        Ok(body) => {
            print!("{body}");
            if !body.ends_with('\n') {
                println!();
            }
            0
        }
        Err(e) => {
            eprintln!("doze-aws export: {stack}: {e}");
            1
        }
    }
}
