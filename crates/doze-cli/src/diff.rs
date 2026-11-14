//! A unified diff of two texts, line by line, for what `--dry-run` would change (`fmt`): the hunks with three lines of
//! context, as `diff -u` and git print them. The longest common subsequence is found by dynamic programming, which is
//! plenty for config files; past [`MAX_CELLS`] the whole file is shown as replaced rather than spend the time.

/// The largest table (lines × lines) the matcher builds.
const MAX_CELLS: usize = 4_000_000;
/// Lines of context around each change.
const CONTEXT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Same,
    Del,
    Add,
}

/// The diff of `before` and `after` under the `---`/`+++` header for `name`; empty when they are the same.
pub(crate) fn unified(name: &str, before: &str, after: &str) -> String {
    if before == after {
        return String::new();
    }
    // Lines keep their newline, so a missing final newline is a difference, marked as diff marks it.
    let a: Vec<&str> = before.split_inclusive('\n').collect();
    let b: Vec<&str> = after.split_inclusive('\n').collect();
    let ops = script(&a, &b);
    let mut out = format!("--- {name}\n+++ {name} (formatted)\n");
    for (start, end) in hunks(&ops) {
        out.push_str(&hunk(&ops, start, end, &a, &b));
    }
    out
}

/// The edit script: one op per line of either side, in order.
fn script(a: &[&str], b: &[&str]) -> Vec<Op> {
    let (n, m) = (a.len(), b.len());
    if n.saturating_mul(m) > MAX_CELLS {
        return std::iter::repeat_n(Op::Del, n)
            .chain(std::iter::repeat_n(Op::Add, m))
            .collect();
    }
    // lcs[i][j]: the common lines of a[i..] and b[j..], flattened.
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            let v = if a.get(i) == b.get(j) {
                cell(&lcs, (i + 1) * w + j + 1) + 1
            } else {
                cell(&lcs, (i + 1) * w + j).max(cell(&lcs, i * w + j + 1))
            };
            if let Some(c) = lcs.get_mut(i * w + j) {
                *c = v;
            }
        }
    }
    let (mut i, mut j, mut ops) = (0, 0, Vec::with_capacity(n + m));
    while i < n || j < m {
        if i < n && j < m && a.get(i) == b.get(j) {
            ops.push(Op::Same);
            (i, j) = (i + 1, j + 1);
        } else if i < n && (j == m || cell(&lcs, (i + 1) * w + j) >= cell(&lcs, i * w + j + 1)) {
            // A line goes before the one that replaces it, as diff prints them.
            ops.push(Op::Del);
            i += 1;
        } else {
            ops.push(Op::Add);
            j += 1;
        }
    }
    ops
}

fn cell(t: &[u32], at: usize) -> u32 {
    t.get(at).copied().unwrap_or(0)
}

/// The op ranges to print: each change with its context, ranges that touch merged.
fn hunks(ops: &[Op]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        if *op == Op::Same {
            continue;
        }
        let (start, end) = (k.saturating_sub(CONTEXT), (k + CONTEXT + 1).min(ops.len()));
        match out.last_mut() {
            Some(last) if start <= last.1 => last.1 = end.max(last.1),
            _ => out.push((start, end)),
        }
    }
    out
}

/// One hunk: its `@@` line and its lines.
fn hunk(ops: &[Op], start: usize, end: usize, a: &[&str], b: &[&str]) -> String {
    // Where the hunk starts on each side: the lines of that side the ops before it consumed.
    let before = ops.get(..start).unwrap_or_default();
    let (mut i, mut j) = (
        before.iter().filter(|o| **o != Op::Add).count(),
        before.iter().filter(|o| **o != Op::Del).count(),
    );
    let span = ops.get(start..end).unwrap_or_default();
    let old = span.iter().filter(|o| **o != Op::Add).count();
    let new = span.iter().filter(|o| **o != Op::Del).count();
    let mut out = format!("@@ -{} +{} @@\n", range(i, old), range(j, new));
    for op in span {
        let line = match op {
            Op::Same => {
                i += 1;
                j += 1;
                line(' ', a.get(i - 1))
            }
            Op::Del => {
                i += 1;
                line('-', a.get(i - 1))
            }
            Op::Add => {
                j += 1;
                line('+', b.get(j - 1))
            }
        };
        out.push_str(&line);
    }
    out
}

/// One line of a hunk, with its newline; a last line without one gets diff's marker.
fn line(mark: char, text: Option<&&str>) -> String {
    let text = text.copied().unwrap_or_default();
    match text.strip_suffix('\n') {
        Some(t) => format!("{mark}{t}\n"),
        None => format!("{mark}{text}\n\\ No newline at end of file\n"),
    }
}

/// `start,len` as diff prints it (1-based; an empty side is `start,0` at the line before).
fn range(start: usize, len: usize) -> String {
    match len {
        0 => format!("{start},0"),
        1 => format!("{}", start + 1),
        n => format!("{},{n}", start + 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_changed_line_with_context() {
        let before = "a\nb\nc\nd\ne\nf\ng\nh\n";
        let after = "a\nb\nc\nD\ne\nf\ng\nh\n";
        assert_eq!(
            unified("x", before, after),
            "--- x\n+++ x (formatted)\n@@ -1,7 +1,7 @@\n a\n b\n c\n-d\n+D\n e\n f\n g\n"
        );
        assert_eq!(unified("x", "same\n", "same\n"), "");
    }

    #[test]
    fn far_apart_changes_are_two_hunks() {
        let before: String = (0..20).map(|n| format!("{n}\n")).collect();
        let after = before
            .replace("2\n", "two\n")
            .replace("17\n", "seventeen\n");
        let d = unified("x", &before, &after);
        assert_eq!(d.matches("@@ ").count(), 2, "{d}");
        assert!(d.contains("-2\n+two\n") && d.contains("-17\n+seventeen\n"));
    }

    #[test]
    fn added_and_removed_lines_count_right() {
        let d = unified("x", "a\n", "a\nb\n");
        assert_eq!(d, "--- x\n+++ x (formatted)\n@@ -1 +1,2 @@\n a\n+b\n");
        let d = unified("x", "", "a\n");
        assert_eq!(d, "--- x\n+++ x (formatted)\n@@ -0,0 +1 @@\n+a\n");
        // Only the final newline differs: diff's marker says so.
        let d = unified("x", "a", "a\n");
        assert_eq!(
            d,
            "--- x\n+++ x (formatted)\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+a\n"
        );
    }
}
