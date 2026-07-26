use std::path::PathBuf;

use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;

use super::initial_input;

#[test]
fn initial_input_keeps_images_before_prompt() {
    let input = initial_input(
        Some("explain these".to_string()),
        vec![PathBuf::from("first.png"), PathBuf::from("second.png")],
    );

    assert_eq!(
        input,
        vec![
            UserInput::LocalImage {
                path: PathBuf::from("first.png"),
                detail: None,
            },
            UserInput::LocalImage {
                path: PathBuf::from("second.png"),
                detail: None,
            },
            UserInput::Text {
                text: "explain these".to_string(),
                text_elements: Vec::new(),
            },
        ]
    );
}
