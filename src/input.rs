//! Reading the input files: lines, line endings, final newline.

#[derive(Debug, PartialEq, Eq)]
pub struct FileText {
    pub lines: Vec<String>,
    /// Every line break is CRLF (and was stripped from `lines`).
    pub crlf: bool,
    /// Whether the last line ends with a line break; `None` for an empty file.
    pub final_newline: Option<bool>,
}

pub fn parse(text: &str) -> FileText {
    if text.is_empty() {
        return FileText {
            lines: Vec::new(),
            crlf: false,
            final_newline: None,
        };
    }
    let final_newline = text.ends_with('\n');
    let body = text.strip_suffix('\n').unwrap_or(text);
    let mut lines: Vec<String> = body.split('\n').map(String::from).collect();
    // Only a file that uses CRLF throughout is treated as a CRLF file;
    // anything else keeps its bytes as they are.
    let breaks = lines.len() - 1 + final_newline as usize;
    let crlf = breaks > 0 && lines.iter().take(breaks).all(|l| l.ends_with('\r'));
    if crlf {
        for l in lines.iter_mut().take(breaks) {
            l.pop();
        }
    }
    FileText {
        lines,
        crlf,
        final_newline: Some(final_newline),
    }
}

/// Three-way merge of the "ends with a newline" property.
pub fn merge_final_newline(base: Option<bool>, left: Option<bool>, right: Option<bool>) -> bool {
    let pick = if left == base {
        right.or(left)
    } else {
        left.or(right)
    };
    pick.unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> (Vec<&'static str>, bool, Option<bool>) {
        let f = parse(text);
        (
            f.lines.into_iter().map(|l| &*l.leak()).collect(),
            f.crlf,
            f.final_newline,
        )
    }

    #[test]
    fn splits_lines_and_notes_the_final_newline() {
        assert_eq!(parsed("a\nb\n"), (vec!["a", "b"], false, Some(true)));
        assert_eq!(parsed("a\nb"), (vec!["a", "b"], false, Some(false)));
    }

    #[test]
    fn keeps_trailing_blank_lines() {
        assert_eq!(parsed("a\n\n"), (vec!["a", ""], false, Some(true)));
    }

    #[test]
    fn an_empty_file_has_no_lines() {
        assert_eq!(parsed(""), (vec![], false, None));
    }

    #[test]
    fn crlf_files_lose_their_carriage_returns() {
        assert_eq!(parsed("a\r\nb\r\n"), (vec!["a", "b"], true, Some(true)));
        assert_eq!(parsed("a\r\nb"), (vec!["a", "b"], true, Some(false)));
    }

    #[test]
    fn mixed_line_endings_are_kept_verbatim() {
        assert_eq!(parsed("a\r\nb\n"), (vec!["a\r", "b"], false, Some(true)));
    }

    #[test]
    fn final_newline_follows_the_side_that_changed_it() {
        let (yes, no) = (Some(true), Some(false));
        assert!(merge_final_newline(yes, yes, yes));
        assert!(!merge_final_newline(yes, yes, no));
        assert!(!merge_final_newline(yes, no, yes));
        assert!(merge_final_newline(no, yes, no));
        assert!(!merge_final_newline(None, no, yes));
        assert!(merge_final_newline(None, None, None));
    }
}
