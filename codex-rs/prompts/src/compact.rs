pub const SUMMARIZATION_PROMPT: &str = include_str!("../templates/compact/prompt.md");
pub const SUMMARY_PREFIX: &str = include_str!("../templates/compact/summary_prefix.md");

const COMPACT_CONTINUATION_SUFFIX: &str = r#"Continue the conversation from where it left off without asking the user any further questions. Resume directly — do not acknowledge the summary, do not recap what was happening, do not preface with "I'll continue" or similar. Pick up the last task as if the break never happened."#;

pub fn format_compact_summary(summary: &str) -> String {
    let without_analysis = replace_first_tag_block(summary, "analysis", None);
    let with_summary_header =
        replace_first_tag_block(&without_analysis, "summary", Some("Summary"));
    collapse_extra_blank_lines(&with_summary_header)
        .trim()
        .to_string()
}

pub fn compact_user_summary_message(summary: &str, suppress_follow_up_questions: bool) -> String {
    let formatted_summary = format_compact_summary(summary);
    let mut message = format!("{}\n\n{}", SUMMARY_PREFIX.trim_end(), formatted_summary);

    if suppress_follow_up_questions {
        message.push('\n');
        message.push_str(COMPACT_CONTINUATION_SUFFIX);
    }

    message
}

fn replace_first_tag_block(input: &str, tag: &str, replacement_header: Option<&str>) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let Some(start) = input.find(&open) else {
        return input.to_string();
    };
    let content_start = start + open.len();
    let Some(close_offset) = input[content_start..].find(&close) else {
        return input.to_string();
    };
    let content_end = content_start + close_offset;
    let end = content_end + close.len();

    let mut output = String::with_capacity(input.len());
    output.push_str(&input[..start]);
    if let Some(header) = replacement_header {
        let content = input[content_start..content_end].trim();
        output.push_str(header);
        output.push_str(":\n");
        output.push_str(content);
    }
    output.push_str(&input[end..]);
    output
}

fn collapse_extra_blank_lines(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut newlines = 0;
    for ch in input.chars() {
        if ch == '\n' {
            newlines += 1;
            if newlines <= 2 {
                output.push(ch);
            }
        } else {
            newlines = 0;
            output.push(ch);
        }
    }
    output
}

#[cfg(test)]
#[path = "compact_tests.rs"]
mod tests;
