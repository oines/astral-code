use std::path::PathBuf;

use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

use super::*;

#[test]
fn submission_keeps_text_and_projects_bound_targets_once() {
    let binding = ComposerElement::mention(
        4..11,
        "$review".to_string(),
        MentionTarget::Skill {
            name: "review".to_string(),
            path: PathBuf::from("/workspace/review/SKILL.md"),
        },
    );
    let submission = PromptSubmission {
        text: "use $review".to_string(),
        elements: vec![binding.clone(), binding],
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

#[test]
fn slash_args_keep_structured_mentions_after_removing_the_command() {
    let submission = PromptSubmission {
        text: "/plan use $review".to_string(),
        elements: vec![ComposerElement::mention(
            10..17,
            "$review".to_string(),
            MentionTarget::Skill {
                name: "review".to_string(),
                path: PathBuf::from("/workspace/review/SKILL.md"),
            },
        )],
    }
    .into_slash_args("plan", "use $review".to_string());

    assert_eq!(
        submission,
        PromptSubmission {
            text: "use $review".to_string(),
            elements: vec![ComposerElement::mention(
                4..11,
                "$review".to_string(),
                MentionTarget::Skill {
                    name: "review".to_string(),
                    path: PathBuf::from("/workspace/review/SKILL.md"),
                },
            )],
        }
    );
}
