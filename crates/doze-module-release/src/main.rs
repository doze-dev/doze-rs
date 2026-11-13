//! Packages one built Rust module for one target into a registry release directory: the archive, the merged schema-1
//! index (protocol 2), and meta.yaml (docs/port/modules/PLAN.md step 6). It publishes nothing: CI uploads the directory
//! to the module's rolling GitHub release, and doze-registry's `publish.mjs` signs it.
//!
//!   doze-module-release --binary PATH --target TRIPLE --version SEMVER --out DIR [--namespace NS] [--describe-with PATH]
//!                       --accepted
//!
//! `--accepted` states that the module's acceptance tests passed on TARGET (PLAN.md D12); CI passes it only from the
//! job that ran them. Without it, or for a target the module does not declare, nothing is written.

use std::process::ExitCode;

fn main() -> ExitCode {
    match doze_module_release::parse_args().and_then(|a| doze_module_release::release(&a)) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("doze-module-release: {e}");
            ExitCode::FAILURE
        }
    }
}
