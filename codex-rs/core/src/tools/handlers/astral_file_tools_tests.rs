use pretty_assertions::assert_eq;

use super::add_line_numbers;
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
