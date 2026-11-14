//! Names for resources the template does not name: `<stack>-<logical>-<random>` as AWS makes them (memory: twelve upper-case letters
//! and digits; stack and logical id shortened to fit the type's limit), or the logical id alone when the configuration asks for
//! the Go edition's naming.

use super::Rc;

/// A name for the resource of `rc`, at most `max` characters; `lower` for a type whose names are lower-case (S3 buckets).
pub(crate) fn generated_name(rc: &Rc, max: usize, lower: bool) -> String {
    let fix = |s: &str| {
        if lower {
            s.to_ascii_lowercase()
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        } else {
            s.to_owned()
        }
    };
    if rc.logical_names {
        let mut name = fix(&rc.logical);
        name.truncate(max);
        return name;
    }
    let suffix = crate::model::suffix(12, lower);
    let budget = max.saturating_sub(suffix.len() + 2);
    let (mut stack, mut logical) = (fix(&rc.stack_name), fix(&rc.logical));
    while stack.len() + logical.len() > budget {
        if stack.len() >= logical.len() {
            stack.pop();
        } else {
            logical.pop();
        }
    }
    format!("{stack}-{logical}-{suffix}")
}

/// A name for a type whose names carry a suffix of their own (`.fifo`).
pub(crate) fn generated_with(rc: &Rc, max: usize, lower: bool, ending: &str) -> String {
    let mut n = generated_name(rc, max.saturating_sub(ending.len()), lower);
    n.push_str(ending);
    n
}
