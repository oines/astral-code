use super::*;

#[test]
fn local_compaction_projects_to_plain_user_input() {
    let projected = project_responses_input(vec![TranscriptItem::LocalCompaction {
        text: "local summary".to_string(),
    }]);

    assert_eq!(
        projected,
        vec![TranscriptItem::Message {
            id: None,
            role: "user".to_string(),
            content: vec![ContentItem::InputText {
                text: "local summary".to_string(),
            }],
            phase: None,
        }]
    );
    let json = serde_json::to_value(projected).expect("serialize projected input");
    assert_eq!(json[0]["type"], "message");
    assert!(json[0].get("encrypted_content").is_none());
}

#[test]
fn native_compaction_remains_opaque_responses_input() {
    let native = TranscriptItem::Compaction {
        encrypted_content: "opaque".to_string(),
    };

    assert_eq!(project_responses_input(vec![native.clone()]), vec![native]);
}

#[test]
fn encrypted_state_reset_preserves_visible_history() {
    let visible_user = TranscriptItem::Message {
        id: None,
        role: "user".to_string(),
        content: vec![ContentItem::InputText {
            text: "keep me".to_string(),
        }],
        phase: None,
    };
    let encrypted_reasoning = TranscriptItem::Reasoning {
        id: "reasoning-1".to_string(),
        summary: Vec::new(),
        content: None,
        encrypted_content: Some("opaque".to_string()),
        provider_metadata: None,
    };
    let native_compaction = TranscriptItem::Compaction {
        encrypted_content: "opaque-compaction".to_string(),
    };
    let local_compaction = TranscriptItem::LocalCompaction {
        text: "visible summary".to_string(),
    };

    let (cleaned, removed) = strip_responses_encrypted_state(vec![
        visible_user.clone(),
        encrypted_reasoning,
        native_compaction,
        local_compaction.clone(),
    ]);

    assert_eq!(removed, 2);
    assert_eq!(cleaned, vec![visible_user, local_compaction]);
}
