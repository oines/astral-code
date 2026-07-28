//! Fenced-code syntax resolution and highlighting derived from Grok Build.

use std::path::Path;
use std::sync::OnceLock;

use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::FontStyle;
use syntect::highlighting::Theme;
use syntect::parsing::SyntaxReference;
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;
use two_face::theme::EmbeddedThemeName;

use super::MarkdownSyntaxTheme;
use super::Segment;

const MAX_HIGHLIGHT_BYTES: usize = 512 * 1024;
const MAX_HIGHLIGHT_LINES: usize = 10_000;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static NIGHT_THEME: OnceLock<Theme> = OnceLock::new();
static DAY_THEME: OnceLock<Theme> = OnceLock::new();

/// Stateful, file-extension-aware highlighter used by structured diffs.
///
/// Grok Build keeps independent old/new highlighters while walking a hunk so
/// multiline syntax state cannot leak between file versions. The diff
/// renderer owns those two instances; this type only centralizes Astral's
/// grammar, theme, guardrail, and style conversion.
pub(crate) struct CodeLineHighlighter {
    inner: HighlightLines<'static>,
    theme: MarkdownSyntaxTheme,
}

impl CodeLineHighlighter {
    pub(crate) fn for_path(path: &Path, source: &str, theme: MarkdownSyntaxTheme) -> Option<Self> {
        if source.len() > MAX_HIGHLIGHT_BYTES || source.lines().count() > MAX_HIGHLIGHT_LINES {
            return None;
        }
        let extension = path.extension()?.to_str()?;
        let syntax = syntax_set().find_syntax_by_extension(extension)?;
        Some(Self {
            inner: HighlightLines::new(syntax, syntax_theme(theme)),
            theme,
        })
    }

    pub(crate) fn highlight_line(&mut self, source: &str) -> Option<Vec<Span<'static>>> {
        let source = format!("{source}\n");
        let highlighted = self.inner.highlight_line(&source, syntax_set()).ok()?;
        Some(
            highlighted
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches(['\n', '\r']);
                    (!text.is_empty())
                        .then(|| Span::styled(text.to_string(), convert_style(style, self.theme)))
                })
                .collect(),
        )
    }
}

pub(super) fn highlight_code(
    source: &str,
    fence_info: &str,
    theme: MarkdownSyntaxTheme,
) -> Option<Vec<Vec<Segment>>> {
    if source.is_empty()
        || source.len() > MAX_HIGHLIGHT_BYTES
        || source.lines().count() > MAX_HIGHLIGHT_LINES
    {
        return None;
    }
    let syntax = find_syntax(fence_info)?;
    let mut highlighter = HighlightLines::new(syntax, syntax_theme(theme));
    let mut lines = Vec::new();
    for line in LinesWithEndings::from(source) {
        let highlighted = highlighter.highlight_line(line, syntax_set()).ok()?;
        lines.push(
            highlighted
                .into_iter()
                .filter_map(|(style, text)| {
                    let text = text.trim_end_matches(['\n', '\r']);
                    (!text.is_empty()).then(|| Segment {
                        text: text.to_string(),
                        style: convert_style(style, theme),
                    })
                })
                .collect(),
        );
    }
    Some(lines)
}

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(two_face::syntax::extra_newlines)
}

fn syntax_theme(theme: MarkdownSyntaxTheme) -> &'static Theme {
    match theme {
        MarkdownSyntaxTheme::Night | MarkdownSyntaxTheme::Terminal => {
            NIGHT_THEME.get_or_init(|| {
                two_face::theme::extra()
                    .get(EmbeddedThemeName::CatppuccinMocha)
                    .clone()
            })
        }
        MarkdownSyntaxTheme::Day => DAY_THEME.get_or_init(|| {
            two_face::theme::extra()
                .get(EmbeddedThemeName::CatppuccinLatte)
                .clone()
        }),
    }
}

fn find_syntax(fence_info: &str) -> Option<&'static SyntaxReference> {
    let fence_info = fence_info.trim();
    if let Some(path) = citation_path(fence_info)
        && let Some(extension) = Path::new(path).extension()
        && let Some(extension) = extension.to_str()
        && let Some(syntax) = syntax_set().find_syntax_by_extension(extension)
    {
        return Some(syntax);
    }
    let token = fence_info.split_ascii_whitespace().next()?;
    let token = match token {
        "csharp" | "c-sharp" => "c#",
        "golang" => "go",
        "python3" => "python",
        "shell" | "sh" => "bash",
        other => other,
    };
    syntax_set()
        .find_syntax_by_token(token)
        .or_else(|| syntax_set().find_syntax_by_name(token))
        .or_else(|| {
            syntax_set()
                .syntaxes()
                .iter()
                .find(|syntax| syntax.name.eq_ignore_ascii_case(token))
        })
        .or_else(|| syntax_set().find_syntax_by_extension(token))
}

fn citation_path(info: &str) -> Option<&str> {
    let mut parts = info.splitn(3, ':');
    let start = parts.next()?;
    let end = parts.next()?;
    let path = parts.next()?;
    if start.is_empty()
        || end.is_empty()
        || path.is_empty()
        || !start.chars().all(|character| character.is_ascii_digit())
        || !end.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }
    Some(path)
}

// Syntect themes define arbitrary RGB token colors; preserving those colors is
// the purpose of this adapter. Terminal-native mode takes the ANSI-only branch.
#[allow(clippy::disallowed_methods)]
fn convert_style(style: syntect::highlighting::Style, theme: MarkdownSyntaxTheme) -> Style {
    let foreground = match theme {
        MarkdownSyntaxTheme::Night | MarkdownSyntaxTheme::Day => {
            Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b)
        }
        MarkdownSyntaxTheme::Terminal => {
            polarity_safe_color(style.foreground.r, style.foreground.g, style.foreground.b)
        }
    };
    let mut converted = Style::default().fg(foreground);
    if style.font_style.contains(FontStyle::BOLD) {
        converted = converted.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        converted = converted.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        converted = converted.add_modifier(Modifier::UNDERLINED);
    }
    converted
}

fn polarity_safe_color(r: u8, g: u8, b: u8) -> Color {
    let max = r.max(g).max(b) as i32;
    let min = r.min(g).min(b) as i32;
    let chroma = max - min;
    if chroma < 40 {
        return Color::Reset;
    }
    let (red, green, blue) = (i32::from(r), i32::from(g), i32::from(b));
    let hue = if max == red {
        let mut hue = (green - blue) * 60 / chroma;
        if hue < 0 {
            hue += 360;
        }
        hue
    } else if max == green {
        (blue - red) * 60 / chroma + 120
    } else {
        (red - green) * 60 / chroma + 240
    };
    match hue {
        0..30 | 330..=360 => Color::Red,
        30..90 => Color::Yellow,
        90..150 => Color::Green,
        150..210 => Color::Cyan,
        210..255 => Color::Blue,
        _ => Color::Magenta,
    }
}

#[cfg(test)]
#[path = "syntax_tests.rs"]
mod tests;
