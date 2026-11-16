//! The diff the "Pending changes" page shows (decision 89, point 6): the
//! lines of the configuration file a proposal's apply would remove and
//! add, with a few unchanged lines around each change.

/// Unchanged lines shown before and after each change.
const CONTEXT: usize = 3;

/// The diff of `before` to `after`, line by line: each hunk opens with
/// `@@ -a,b +c,d @@` as a unified diff does, then its lines, unchanged ones
/// after a space, removed ones after `-` and added ones after `+`. Equal
/// texts have no hunk.
pub fn unified_diff(before: &str, after: &str) -> String {
    let _ = (before, after, CONTEXT);
    todo!()
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
}
