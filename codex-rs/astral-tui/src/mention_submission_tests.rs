use std::path::PathBuf;

use codex_app_server_protocol::ByteRange;
use codex_app_server_protocol::TextElement;
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

#[test]
fn local_image_range_tracks_expanded_paste_and_round_trips_history() {
    let paste_placeholder = "[Pasted: 4 lines]";
    let image_placeholder = "[Image #2]";
    let text = format!("inspect {paste_placeholder} {image_placeholder}");
    let paste_start = "inspect ".len();
    let image_start = paste_start + paste_placeholder.len() + 1;
    let path = PathBuf::from("/tmp/screenshot.png");
    let submission = PromptSubmission {
        text,
        elements: vec![
            ComposerElement::paste(
                paste_start..paste_start + paste_placeholder.len(),
                paste_placeholder.to_string(),
                "one\ntwo\nthree\nfour".to_string(),
            ),
            ComposerElement::local_image(
                image_start..image_start + image_placeholder.len(),
                LocalImage {
                    path: path.clone(),
                    display_number: 2,
                    dimensions: Some((640, 480)),
                    byte_len: Some(12_345),
                },
            ),
        ],
    };
    let projected_text = "inspect one\ntwo\nthree\nfour [Image #2]".to_string();
    let projected_image_start = projected_text.len() - image_placeholder.len();
    let expected = vec![
        UserInput::Text {
            text: projected_text,
            text_elements: vec![TextElement::new(
                ByteRange {
                    start: projected_image_start,
                    end: projected_image_start + image_placeholder.len(),
                },
                Some(image_placeholder.to_string()),
            )],
        },
        UserInput::LocalImage { detail: None, path },
    ];

    assert_eq!(submission.user_input(), expected);
    assert_eq!(
        PromptSubmission::from_user_input(&expected).user_input(),
        expected
    );
}
