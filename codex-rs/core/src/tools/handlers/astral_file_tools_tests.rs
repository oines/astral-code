use pretty_assertions::assert_eq;
use serde_json::json;

use super::GrepArgs;
use super::add_line_numbers;
use super::push_content_matches;
use super::split_lines_preserving_newline;

#[test]
fn read_output_uses_compact_line_number_prefixes() {
    let text = "first\n  second\nthird";
    let lines = split_lines_preserving_newline(text);

    assert_eq!(
        add_line_numbers(&lines[1..], /*start_line*/ 2),
        "2\t  second\n3\tthird"
    );
}

#[test]
fn grep_line_numbers_flag_is_optional() {
    let args: GrepArgs =
        serde_json::from_value(json!({ "pattern": "needle" })).expect("valid Grep args");

    assert_eq!(args.line_numbers, None);
}

#[test]
fn grep_content_output_can_include_line_numbers() {
    let text = "alpha\nneedle\nomega\n";
    let lines = split_lines_preserving_newline(text);
    let mut output = Vec::new();

    push_content_matches(
        &mut output,
        "src/lib.rs",
        &lines,
        &[1],
        /*line_numbers*/ true,
        /*context_before*/ 0,
        /*context_after*/ 0,
    );

    assert_eq!(output, vec!["src/lib.rs:2:needle"]);
}
