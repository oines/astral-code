use super::*;
use pretty_assertions::assert_eq;

#[test]
fn format_compact_summary_strips_analysis_and_formats_summary() {
    let raw = r#"<analysis>
The model may mention raw tool text here:
<tool_use_error>Cancelled</tool_use_error>
</analysis>

<summary>
1. Primary Request and Intent:
   Continue the compact task.

8. Current Work:
   Tests were about to run.
</summary>"#;

    let formatted = format_compact_summary(raw);

    assert_eq!(
        formatted,
        "Summary:\n1. Primary Request and Intent:\n   Continue the compact task.\n\n8. Current Work:\n   Tests were about to run."
    );
}

#[test]
fn format_compact_summary_trims_unwrapped_text() {
    let formatted = format_compact_summary("\n\nAlready summarized.\n\n\n");

    assert_eq!(formatted, "Already summarized.");
}

#[test]
fn compact_user_summary_message_uses_claude_continuation_wrapper() {
    let raw = r#"<summary>
1. Primary Request and Intent:
   Fix the failing tests.
</summary>"#;

    let message = compact_user_summary_message(raw, true);

    assert_eq!(
        message,
        r#"This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation.

Summary:
1. Primary Request and Intent:
   Fix the failing tests.
Continue the conversation from where it left off without asking the user any further questions. Resume directly — do not acknowledge the summary, do not recap what was happening, do not preface with "I'll continue" or similar. Pick up the last task as if the break never happened."#
    );
}

#[test]
fn compact_user_summary_message_can_override_continuation_prompt() {
    let raw = r#"<summary>
1. Primary Request and Intent:
   Keep event stream context.
</summary>"#;

    let message = compact_user_summary_message_with_continuation(
        raw,
        true,
        Some("Do not replay historical events."),
    );

    assert_eq!(
        message,
        r#"This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation.

Summary:
1. Primary Request and Intent:
   Keep event stream context.
Do not replay historical events."#
    );
}
