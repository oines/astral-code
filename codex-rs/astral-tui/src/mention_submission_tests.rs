use std::path::PathBuf;

use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn submission_keeps_text_and_projects_bound_targets_once() {
    let binding = MentionBinding {
        range: 4..11,
        insert_text: "$review".to_string(),
        target: MentionTarget::Skill {
            name: "review".to_string(),
            path: PathBuf::from("/workspace/review/SKILL.md"),
        },
    };
    let submission = PromptSubmission {
        text: "use $review".to_string(),
        mentions: vec![binding.clone(), binding],
    };

    assert_eq!(
        submission.user_input(),
        vec![
            UserInput::Text {
                text: "use $review".to_string(),
                text_elements: Vec::new(),
            },
            UserInput::Skill {
                name: "review".to_string(),
                path: PathBuf::from("/workspace/review/SKILL.md"),
            },
        ]
    );
}
