use std::path::PathBuf;

use pretty_assertions::assert_eq;

use super::*;

#[test]
fn controller_prioritizes_plugins_and_preserves_dismissal_until_token_changes() {
    let mut controller = MentionController::default();
    controller.set_catalog(MentionCatalog {
        candidates: vec![
            candidate(MentionKind::Skill, "Browser Skill", "$browser"),
            candidate(MentionKind::Plugin, "Browser Plugin", "@Browser"),
        ],
    });

    controller.refresh("use $bro", "use $bro".len());
    assert_eq!(
        controller
            .snapshot()
            .matches
            .iter()
            .map(|suggestion| suggestion.kind)
            .collect::<Vec<_>>(),
        vec![MentionKind::Plugin, MentionKind::Skill]
    );

    controller.dismiss("use $bro");
    controller.refresh("use $bro", "use $bro".len());
    assert!(!controller.snapshot().open);
    controller.refresh("use $brow", "use $brow".len());
    assert!(controller.snapshot().open);
}

fn candidate(kind: MentionKind, display: &str, insert_text: &str) -> MentionCandidate {
    let target = match kind {
        MentionKind::Plugin => MentionTarget::Plugin {
            name: display.to_string(),
            path: format!("plugin://{display}"),
        },
        MentionKind::Skill => MentionTarget::Skill {
            name: display.to_string(),
            path: PathBuf::from(format!("/skills/{display}/SKILL.md")),
        },
    };
    MentionCandidate {
        kind,
        display: display.to_string(),
        description: "description".to_string(),
        insert_text: insert_text.to_string(),
        search_terms: vec![display.to_string()],
        target,
    }
}
