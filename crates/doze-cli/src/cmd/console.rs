//! `doze open --console <name>` (new in the Rust doze; the Go dash's `o` key, `display.WebURL`): the module's own web
//! console (temporal's Web UI, the aws console) in the browser. The address is the running stack's: the instance's
//! `console`, else its `web_url` (the module's, its address filled in by the daemon). With no display here (SSH, a
//! Linux box with none) the address is printed instead, so it can be opened from where the person sits.

use crate::ctx::Ctx;
use crate::diag::{Diagnostic, ids};
use crate::output::{Done, Mode, Outcome};
use crate::stack::{self, Stack};
use crate::view::View;
use serde::Serialize;

/// Where a console is, and whether it was opened.
#[derive(Debug, Serialize)]
pub struct Console {
    pub name: String,
    pub url: String,
    /// False: there is no display here, so the address was printed.
    pub opened: bool,
}

impl Console {
    pub(crate) fn human(&self) -> String {
        if self.opened {
            format!("→ opened {} in your browser\n", self.url)
        } else {
            format!("{}\n", self.url)
        }
    }
}

/// `doze open --console <name>`; `display` says whether a browser can be shown here.
pub(crate) fn open(ctx: &mut Ctx<'_>, mode: &Mode, name: &str, display: bool) -> Outcome {
    let stack = Stack::find(ctx, mode)?;
    let Some(conn) = stack.open(mode.timeout)? else {
        // The name first, as every command checks it (exit 4), then the stack (exit 7).
        stack::check_offline(&stack, &[name.to_owned()])?;
        return Err(stack::down("doze open --console"));
    };
    let snap = stack::snapshot(&conn)?;
    let v = stack::declared(&snap)
        .find(|v| v.name == name)
        .ok_or_else(|| stack::not_declared(&snap, name))?;
    let url = if v.console.is_empty() {
        v.web_url.clone()
    } else {
        v.console.clone()
    };
    if url.is_empty() {
        return Err(Diagnostic::new(
            ids::NOTHING_TO_DO,
            format!("{name} has no console of its own"),
        )
        .fix(format!(
            "doze url {name} prints its address · doze open --show {name} shows it in the app"
        )));
    }
    let name = name.to_owned();
    if !display {
        if !mode.json {
            let _ = writeln!(ctx.err, "no display here; the console of {name} is at:");
        }
        return Ok(Done::ok(View::Console(Console {
            name,
            url,
            opened: false,
        })));
    }
    match ctx.host.open_url(&url) {
        Ok(()) => Ok(Done::ok(View::Console(Console {
            name,
            url,
            opened: true,
        }))),
        Err(e) => Err(Diagnostic::new(
            ids::NO_BROWSER,
            format!("the browser could not be opened ({e})"),
        )
        .fix(format!("open it yourself: {url}"))),
    }
}
