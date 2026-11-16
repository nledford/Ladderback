//! The diff the "Pending changes" page shows (decision 89, point 6): the
//! lines of the configuration file a proposal's apply would remove and
//! add, with a few unchanged lines around each change.

use std::fmt::Write as _;

/// Unchanged lines shown before and after each change.
const CONTEXT: usize = 3;

/// The most cells of the table that matches the changed middles of the two
/// texts line by line. Beyond it, the middle shows as all removed, then all
/// added: still the right diff, if not the shortest. A configuration file is
/// capped well below what reaches it.
const MAX_CELLS: usize = 4_000_000;

/// What a line of the diff does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Keep,
    Remove,
    Add,
}

/// The diff of `before` to `after`, line by line: each hunk opens with
/// `@@ -a,b +c,d @@` as a unified diff does, then its lines, unchanged ones
/// after a space, removed ones after `-` and added ones after `+`. Equal
/// texts have no hunk.
pub fn unified_diff(before: &str, after: &str) -> String {
    let ops = edits(
        &before.lines().collect::<Vec<_>>(),
        &after.lines().collect::<Vec<_>>(),
    );
    let mut diff = String::new();
    // Where each op starts in the old and the new text, from 0.
    let mut at = Vec::with_capacity(ops.len());
    let (mut old, mut new) = (0_usize, 0_usize);
    for (op, _) in &ops {
        at.push((old, new));
        match op {
            Op::Keep => {
                old = old.saturating_add(1);
                new = new.saturating_add(1);
            }
            Op::Remove => old = old.saturating_add(1),
            Op::Add => new = new.saturating_add(1),
        }
    }
    for (start, end) in hunks(&ops) {
        let lines = ops.get(start..end).unwrap_or_default();
        let (old_at, new_at) = at.get(start).copied().unwrap_or_default();
        let old_count = lines.iter().filter(|(op, _)| *op != Op::Add).count();
        let new_count = lines.iter().filter(|(op, _)| *op != Op::Remove).count();
        let _ = writeln!(
            diff,
            "@@ -{} +{} @@",
            range(old_at, old_count),
            range(new_at, new_count)
        );
        for (op, line) in lines {
            let mark = match op {
                Op::Keep => ' ',
                Op::Remove => '-',
                Op::Add => '+',
            };
            let _ = writeln!(diff, "{mark}{line}");
        }
    }
    diff
}

/// A hunk's range of `count` lines from line `at`, from 0, as a unified
/// diff writes it: from 1, the count left out when it is 1, and an empty
/// range named by the line before it.
fn range(at: usize, count: usize) -> String {
    match count {
        0 => format!("{at},0"),
        1 => at.saturating_add(1).to_string(),
        _ => format!("{},{count}", at.saturating_add(1)),
    }
}

/// The ranges of `ops` each hunk shows: every change, with [`CONTEXT`]
/// unchanged lines around it, and changes whose context would meet in one.
fn hunks(ops: &[(Op, &str)]) -> Vec<(usize, usize)> {
    let mut hunks: Vec<(usize, usize)> = Vec::new();
    for (index, (op, _)) in ops.iter().enumerate() {
        if *op == Op::Keep {
            continue;
        }
        let start = index.saturating_sub(CONTEXT);
        let end = index
            .saturating_add(CONTEXT)
            .saturating_add(1)
            .min(ops.len());
        match hunks.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => hunks.push((start, end)),
        }
    }
    hunks
}

/// The lines of `before` kept or removed and of `after` added, in order:
/// the common start and end kept, and the middle by its longest common
/// run of lines, a removal before an addition where either would do.
fn edits<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<(Op, &'a str)> {
    let prefix = before.iter().zip(after).take_while(|(a, b)| a == b).count();
    let suffix = before
        .iter()
        .skip(prefix)
        .rev()
        .zip(after.iter().skip(prefix).rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old = before
        .get(prefix..before.len().saturating_sub(suffix))
        .unwrap_or_default();
    let new = after
        .get(prefix..after.len().saturating_sub(suffix))
        .unwrap_or_default();
    let mut ops: Vec<(Op, &str)> = Vec::with_capacity(before.len().saturating_add(after.len()));
    ops.extend(before.iter().take(prefix).map(|line| (Op::Keep, *line)));
    middle(old, new, &mut ops);
    ops.extend(
        before
            .iter()
            .skip(before.len().saturating_sub(suffix))
            .map(|line| (Op::Keep, *line)),
    );
    ops
}

/// The edits of `old` to `new`, pushed onto `ops`.
fn middle<'a>(old: &[&'a str], new: &[&'a str], ops: &mut Vec<(Op, &'a str)>) {
    let width = new.len().saturating_add(1);
    let cells = old.len().saturating_add(1).saturating_mul(width);
    if cells > MAX_CELLS {
        ops.extend(old.iter().map(|line| (Op::Remove, *line)));
        ops.extend(new.iter().map(|line| (Op::Add, *line)));
        return;
    }
    // `common[i * width + j]`: the longest common run of `old[i..]` and
    // `new[j..]`.
    let mut common = vec![0_u32; cells];
    let cell = |common: &[u32], i: usize, j: usize| {
        common
            .get(i.saturating_mul(width).saturating_add(j))
            .copied()
            .unwrap_or(0)
    };
    for (i, a) in old.iter().enumerate().rev() {
        for (j, b) in new.iter().enumerate().rev() {
            let value = if a == b {
                cell(&common, i + 1, j + 1).saturating_add(1)
            } else {
                cell(&common, i + 1, j).max(cell(&common, i, j + 1))
            };
            if let Some(slot) = common.get_mut(i.saturating_mul(width).saturating_add(j)) {
                *slot = value;
            }
        }
    }
    let (mut i, mut j) = (0, 0);
    // Each step takes a line of `old`, of `new` or of both, so the walk
    // ends within one step per line.
    for _ in 0..old.len().saturating_add(new.len()) {
        let (Some(a), Some(b)) = (old.get(i), new.get(j)) else {
            break;
        };
        if a == b {
            ops.push((Op::Keep, *a));
            i += 1;
            j += 1;
        } else if cell(&common, i + 1, j) >= cell(&common, i, j + 1) {
            ops.push((Op::Remove, *a));
            i += 1;
        } else {
            ops.push((Op::Add, *b));
            j += 1;
        }
    }
    ops.extend(old.iter().skip(i).map(|line| (Op::Remove, *line)));
    ops.extend(new.iter().skip(j).map(|line| (Op::Add, *line)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_texts_have_no_diff() {
        assert_eq!(unified_diff("a\nb\n", "a\nb\n"), "");
        assert_eq!(unified_diff("", ""), "");
    }

    #[test]
    fn an_added_table_shows_with_the_lines_around_it() {
        let before = "schema = 1\n\n[repositories.demo]\nrepository = \"/srv\"\n";
        let after = "schema = 1\n\n[repositories.demo]\nrepository = \"/srv\"\n\n[plans.home]\nrepository = \"demo\"\n";
        assert_eq!(
            unified_diff(before, after),
            "@@ -2,3 +2,6 @@\n \n [repositories.demo]\n repository = \"/srv\"\n+\n+[plans.home]\n+repository = \"demo\"\n"
        );
    }

    #[test]
    fn a_changed_line_is_removed_then_added_and_far_apart_changes_are_two_hunks() {
        let before: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let after = before
            .replace("line 2\n", "line two\n")
            .replace("line 19\n", "");
        assert_eq!(
            unified_diff(&before, &after),
            concat!(
                "@@ -1,5 +1,5 @@\n line 1\n-line 2\n+line two\n line 3\n line 4\n line 5\n",
                "@@ -16,5 +16,4 @@\n line 16\n line 17\n line 18\n-line 19\n line 20\n",
            )
        );
    }

    #[test]
    fn a_text_from_nothing_is_all_added() {
        assert_eq!(unified_diff("", "a\nb\n"), "@@ -0,0 +1,2 @@\n+a\n+b\n");
    }

    #[test]
    fn a_last_line_without_a_newline_still_ends_its_line() {
        assert_eq!(unified_diff("a", "b"), "@@ -1 +1 @@\n-a\n+b\n");
    }

    /// The longest common run of lines of `a` and `b`, tried every way.
    fn longest_common(a: &[&str], b: &[&str]) -> usize {
        match (a.split_first(), b.split_first()) {
            (Some((x, a_rest)), Some((y, b_rest))) if x == y => 1 + longest_common(a_rest, b_rest),
            (Some((_, a_rest)), Some((_, b_rest))) => {
                longest_common(a_rest, b).max(longest_common(a, b_rest))
            }
            _ => 0,
        }
    }

    #[test]
    fn a_diff_changes_the_fewest_lines_and_rebuilds_both_texts() {
        // Every text of up to four lines, each line one of three.
        let texts: Vec<Vec<&str>> = (0..=4_u32)
            .flat_map(|len| {
                (0..3_usize.pow(len)).map(move |n| {
                    (0..len)
                        .map(|k| ["a", "b", "c"][n / 3_usize.pow(k) % 3])
                        .collect()
                })
            })
            .collect();
        for before in &texts {
            for after in &texts {
                let ops = edits(before, after);
                let changed = ops.iter().filter(|(op, _)| *op != Op::Keep).count();
                let fewest = before.len() + after.len() - 2 * longest_common(before, after);
                assert_eq!(
                    changed,
                    fewest,
                    "{before:?} to {after:?}: {:?}",
                    ops.iter().map(|(_, l)| *l).collect::<Vec<_>>()
                );
                let old: Vec<&str> = ops
                    .iter()
                    .filter(|(op, _)| *op != Op::Add)
                    .map(|(_, l)| *l)
                    .collect();
                let new: Vec<&str> = ops
                    .iter()
                    .filter(|(op, _)| *op != Op::Remove)
                    .map(|(_, l)| *l)
                    .collect();
                assert_eq!((&old, &new), (before, after));
            }
        }
    }

    #[test]
    fn a_middle_beyond_the_table_s_cells_is_all_removed_then_all_added() {
        // 2,002 by 2,002 cells, past the 4,000,000 allowed: the line both
        // middles hold isn't kept.
        let old_lines: Vec<String> = (0..2_000).map(|n| format!("old {n}")).collect();
        let new_lines: Vec<String> = (0..2_000).map(|n| format!("new {n}")).collect();
        let before: Vec<&str> = std::iter::once("both")
            .chain(old_lines.iter().map(String::as_str))
            .collect();
        let after: Vec<&str> = new_lines
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("both"))
            .collect();
        let ops = edits(&before, &after);
        assert!(ops.iter().all(|(op, _)| *op != Op::Keep));
        assert_eq!(ops.len(), 4_002);
    }

    #[test]
    fn a_middle_of_exactly_the_table_s_cells_still_keeps_its_common_line() {
        // 2,000 by 2,000 cells, the 4,000,000 allowed: the line both
        // middles hold is kept.
        let old_lines: Vec<String> = (0..1_998).map(|n| format!("old {n}")).collect();
        let new_lines: Vec<String> = (0..1_998).map(|n| format!("new {n}")).collect();
        let before: Vec<&str> = std::iter::once("both")
            .chain(old_lines.iter().map(String::as_str))
            .collect();
        let after: Vec<&str> = new_lines
            .iter()
            .map(String::as_str)
            .chain(std::iter::once("both"))
            .collect();
        let ops = edits(&before, &after);
        let kept: Vec<&str> = ops
            .iter()
            .filter(|(op, _)| *op == Op::Keep)
            .map(|(_, line)| *line)
            .collect();
        assert_eq!(kept, ["both"]);
        assert_eq!(ops.len(), 3_997);
    }
}
