use codex_terminal_detection::TerminalName;
use pretty_assertions::assert_eq;
use ratatui::style::Color;

use super::ColorLevel;
use super::indexed_color;
use super::promote_known_truecolor_terminal;
use super::quantize_color;
use super::rgb_color;

#[test]
fn apple_terminal_keeps_the_reported_256_color_level() {
    assert_eq!(
        promote_known_truecolor_terminal(ColorLevel::Ansi256, TerminalName::AppleTerminal),
        ColorLevel::Ansi256
    );
}

#[test]
fn known_truecolor_terminal_recovers_missing_colorterm_signal() {
    assert_eq!(
        promote_known_truecolor_terminal(ColorLevel::Ansi256, TerminalName::Ghostty),
        ColorLevel::TrueColor
    );
}

#[test]
fn rgb_grays_quantize_to_the_xterm_grayscale_ramp() {
    assert_eq!(
        quantize_color(rgb_color(244, 244, 244), ColorLevel::Ansi256),
        indexed_color(255)
    );
    assert_eq!(
        quantize_color(rgb_color(30, 30, 34), ColorLevel::Ansi256),
        indexed_color(234)
    );
}

#[test]
fn no_color_strips_every_color_variant() {
    for color in [
        rgb_color(1, 2, 3),
        indexed_color(42),
        Color::Magenta,
        Color::Reset,
    ] {
        assert_eq!(quantize_color(color, ColorLevel::None), Color::Reset);
    }
}
