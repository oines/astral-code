//! Terminal color capability detection and palette quantization.
//!
//! Grok Build quantizes its RGB themes before they reach ratatui. Keep the
//! same boundary here so terminals such as Apple Terminal, which advertise
//! 256 colors but not truecolor, never receive unsupported RGB SGR sequences.

use codex_terminal_detection::TerminalName;
use codex_terminal_detection::terminal_info;
use ratatui::style::Color;

const CUBE_VALUES: [u8; 6] = [0, 95, 135, 175, 215, 255];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ColorLevel {
    None,
    Basic,
    Ansi256,
    #[default]
    TrueColor,
}

impl ColorLevel {
    pub(crate) fn detect() -> Self {
        if std::env::var_os("NO_COLOR").is_some() {
            return Self::None;
        }
        let reported = match supports_color::on(supports_color::Stream::Stdout) {
            Some(level) if level.has_16m => Self::TrueColor,
            Some(level) if level.has_256 => Self::Ansi256,
            Some(level) if level.has_basic => Self::Basic,
            Some(_) => Self::None,
            None => Self::TrueColor,
        };
        promote_known_truecolor_terminal(reported, terminal_info().name)
    }
}

pub(super) fn quantize_color(color: Color, level: ColorLevel) -> Color {
    match level {
        ColorLevel::TrueColor => color,
        ColorLevel::Ansi256 => match color {
            Color::Rgb(r, g, b) => indexed_color(nearest_indexed(r, g, b)),
            other => other,
        },
        ColorLevel::Basic => match color {
            Color::Rgb(r, g, b) => rgb_to_ansi16(r, g, b),
            Color::Indexed(index) => {
                let (r, g, b) = indexed_to_rgb(index);
                rgb_to_ansi16(r, g, b)
            }
            other => other,
        },
        ColorLevel::None => Color::Reset,
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
pub(super) const fn rgb_color(red: u8, green: u8, blue: u8) -> Color {
    Color::Rgb(red, green, blue)
}

#[allow(clippy::disallowed_methods)]
pub(super) const fn indexed_color(index: u8) -> Color {
    Color::Indexed(index)
}

fn promote_known_truecolor_terminal(level: ColorLevel, terminal: TerminalName) -> ColorLevel {
    if level < ColorLevel::TrueColor
        && matches!(
            terminal,
            TerminalName::Ghostty
                | TerminalName::Iterm2
                | TerminalName::WarpTerminal
                | TerminalName::VsCode
                | TerminalName::WezTerm
                | TerminalName::Kitty
                | TerminalName::Alacritty
                | TerminalName::WindowsTerminal
        )
    {
        ColorLevel::TrueColor
    } else {
        level
    }
}

fn nearest_indexed(r: u8, g: u8, b: u8) -> u8 {
    let red = nearest_cube_channel(r);
    let green = nearest_cube_channel(g);
    let blue = nearest_cube_channel(b);
    let cube_index = 16 + 36 * u16::from(red) + 6 * u16::from(green) + u16::from(blue);
    let cube_distance = squared_distance(
        (r, g, b),
        (
            CUBE_VALUES[usize::from(red)],
            CUBE_VALUES[usize::from(green)],
            CUBE_VALUES[usize::from(blue)],
        ),
    );

    let luminance = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gray_step = if luminance <= 3 {
        0
    } else if luminance >= 243 {
        23
    } else {
        ((luminance as i16 - 8 + 5) / 10).clamp(0, 23) as u8
    };
    let gray = (8 + u16::from(gray_step) * 10) as u8;
    if squared_distance((r, g, b), (gray, gray, gray)) < cube_distance {
        232 + gray_step
    } else {
        cube_index as u8
    }
}

fn nearest_cube_channel(value: u8) -> u8 {
    CUBE_VALUES
        .iter()
        .enumerate()
        .min_by_key(|(_, candidate)| value.abs_diff(**candidate))
        .map_or(0, |(index, _)| index as u8)
}

fn indexed_to_rgb(index: u8) -> (u8, u8, u8) {
    match index {
        0 => (0, 0, 0),
        1 => (128, 0, 0),
        2 => (0, 128, 0),
        3 => (128, 128, 0),
        4 => (0, 0, 128),
        5 => (128, 0, 128),
        6 => (0, 128, 128),
        7 => (192, 192, 192),
        8 => (128, 128, 128),
        9 => (255, 0, 0),
        10 => (0, 255, 0),
        11 => (255, 255, 0),
        12 => (0, 0, 255),
        13 => (255, 0, 255),
        14 => (0, 255, 255),
        15 => (255, 255, 255),
        16..=231 => {
            let offset = index - 16;
            (
                CUBE_VALUES[usize::from(offset / 36)],
                CUBE_VALUES[usize::from((offset % 36) / 6)],
                CUBE_VALUES[usize::from(offset % 6)],
            )
        }
        232..=255 => {
            let gray = 8 + (index - 232) * 10;
            (gray, gray, gray)
        }
    }
}

fn rgb_to_ansi16(r: u8, g: u8, b: u8) -> Color {
    const ANSI: [((u8, u8, u8), Color); 16] = [
        ((0, 0, 0), Color::Black),
        ((128, 0, 0), Color::Red),
        ((0, 128, 0), Color::Green),
        ((128, 128, 0), Color::Yellow),
        ((0, 0, 128), Color::Blue),
        ((128, 0, 128), Color::Magenta),
        ((0, 128, 128), Color::Cyan),
        ((192, 192, 192), Color::Gray),
        ((128, 128, 128), Color::DarkGray),
        ((255, 0, 0), Color::LightRed),
        ((0, 255, 0), Color::LightGreen),
        ((255, 255, 0), Color::LightYellow),
        ((0, 0, 255), Color::LightBlue),
        ((255, 0, 255), Color::LightMagenta),
        ((0, 255, 255), Color::LightCyan),
        ((255, 255, 255), Color::White),
    ];
    ANSI.iter()
        .min_by_key(|(rgb, _)| squared_distance((r, g, b), *rgb))
        .map_or(Color::Reset, |(_, color)| *color)
}

fn squared_distance(left: (u8, u8, u8), right: (u8, u8, u8)) -> u32 {
    let red = i32::from(left.0) - i32::from(right.0);
    let green = i32::from(left.1) - i32::from(right.1);
    let blue = i32::from(left.2) - i32::from(right.2);
    (red * red + green * green + blue * blue) as u32
}

#[cfg(test)]
#[path = "color_support_tests.rs"]
mod tests;
