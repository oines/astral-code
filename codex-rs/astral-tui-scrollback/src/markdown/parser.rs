//! Stateless Markdown event parser derived from Grok Build's renderer.

use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;

use super::LineJoiner;
use super::MarkdownLine;
use super::MarkdownLink;
use super::MarkdownStyle;
use super::Segment;
use super::SegmentLink;
use super::code::render_code_line;
use super::syntax::highlight_code;
use super::table::MarkdownTable;
use super::table::MarkdownTableAlignment;
use super::wrapping::wrap_segments_with_joiners;
use crate::web_link::find_web_links;

pub(super) fn render(text: &str, width: u16, style: MarkdownStyle) -> Vec<MarkdownLine> {
    MarkdownWriter::new(width, style).render(text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Paragraph,
    Heading,
    ListItem,
    Quote,
    Code,
    Rule,
    Table,
}

#[derive(Debug)]
struct ListContext {
    next: Option<u64>,
}

#[derive(Debug)]
struct ItemContext {
    marker: String,
    marker_style: Style,
    emitted: bool,
}

#[derive(Debug)]
struct LinkContext {
    id: u32,
    destination: String,
}

#[derive(Debug)]
struct CodeContext {
    source: String,
    fence_info: Option<String>,
}

struct MarkdownWriter {
    width: u16,
    style: MarkdownStyle,
    lines: Vec<Line<'static>>,
    joiners: Vec<LineJoiner>,
    line_links: Vec<Vec<MarkdownLink>>,
    segments: Vec<Segment>,
    inline_styles: Vec<Style>,
    lists: Vec<ListContext>,
    items: Vec<ItemContext>,
    links: Vec<LinkContext>,
    quote_depth: usize,
    heading: Option<HeadingLevel>,
    code: Option<CodeContext>,
    table: Option<MarkdownTable>,
    last_kind: Option<BlockKind>,
    next_link_id: u32,
}

impl MarkdownWriter {
    fn new(width: u16, style: MarkdownStyle) -> Self {
        Self {
            width: width.max(1),
            style,
            lines: Vec::new(),
            joiners: Vec::new(),
            line_links: Vec::new(),
            segments: Vec::new(),
            inline_styles: vec![style.text],
            lists: Vec::new(),
            items: Vec::new(),
            links: Vec::new(),
            quote_depth: 0,
            heading: None,
            code: None,
            table: None,
            last_kind: None,
            next_link_id: 0,
        }
    }

    fn render(mut self, text: &str) -> Vec<MarkdownLine> {
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
        for event in Parser::new_ext(text, options) {
            self.event(event);
        }
        self.flush_rich_block();
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
            self.joiners.pop();
            self.line_links.pop();
        }
        self.lines
            .into_iter()
            .zip(self.joiners)
            .zip(self.line_links)
            .map(|((line, joiner_to_previous), links)| MarkdownLine {
                line,
                joiner_to_previous,
                links,
            })
            .collect()
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start_tag(tag),
            Event::End(tag) => self.end_tag(tag),
            Event::Text(text) => self.push_text(&text),
            Event::Code(code) => self.push_styled(&code, self.style.inline_code),
            Event::SoftBreak => self.push_text(" "),
            Event::HardBreak => self.push_text("\n"),
            Event::Rule => {
                self.flush_rich_block();
                self.push_output(
                    vec![Line::from(Span::styled("───", self.style.rule))],
                    BlockKind::Rule,
                );
            }
            Event::Html(html) | Event::InlineHtml(html) => self.push_text(&html),
            Event::FootnoteReference(reference) => self.push_text(&format!("[{reference}]")),
            Event::TaskListMarker(checked) => self.set_task_marker(checked),
        }
    }

    fn start_tag(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_rich_block();
                self.heading = Some(level);
                self.push_inline_style(self.style.headings[heading_index(level)]);
            }
            Tag::BlockQuote => {
                self.flush_rich_block();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush_rich_block();
                let fence_info = match kind {
                    CodeBlockKind::Fenced(language) => Some(language.into_string()),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some(CodeContext {
                    source: String::new(),
                    fence_info,
                });
            }
            Tag::List(start) => {
                self.flush_rich_block();
                self.lists.push(ListContext { next: start });
            }
            Tag::Item => self.start_item(),
            Tag::Emphasis => self.push_inline_style(self.style.emphasis),
            Tag::Strong => self.push_inline_style(self.style.strong),
            Tag::Strikethrough => self.push_inline_style(self.style.strikethrough),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                let id = self.next_link_id;
                self.next_link_id = self.next_link_id.wrapping_add(1);
                self.links.push(LinkContext {
                    id,
                    destination: dest_url.into_string(),
                });
                self.push_inline_style(self.style.link_text);
            }
            Tag::Table(alignments) => {
                self.flush_rich_block();
                self.table = Some(MarkdownTable::new(
                    alignments
                        .into_iter()
                        .map(MarkdownTableAlignment::from)
                        .collect(),
                ));
            }
            Tag::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.start_head();
                }
            }
            Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.start_row();
                }
            }
            Tag::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.start_cell();
                }
            }
            Tag::HtmlBlock | Tag::FootnoteDefinition(_) | Tag::MetadataBlock(_) => {}
        }
    }

    fn end_tag(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_rich_block(),
            TagEnd::Heading(_) => {
                self.flush_rich_block();
                self.pop_inline_style();
                self.heading = None;
            }
            TagEnd::BlockQuote => {
                self.flush_rich_block();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => self.finish_code_block(),
            TagEnd::List(_) => {
                self.flush_rich_block();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_rich_block();
                self.items.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.pop_inline_style();
            }
            TagEnd::Link | TagEnd::Image => self.finish_link(),
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    let rendered = table.render(self.width, self.style);
                    self.push_metadata_output(rendered, BlockKind::Table);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.end_head();
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.end_row();
                }
            }
            TagEnd::TableCell => {
                if let Some(table) = self.table.as_mut() {
                    table.finish_cell();
                }
            }
            TagEnd::HtmlBlock | TagEnd::FootnoteDefinition | TagEnd::MetadataBlock(_) => {}
        }
    }

    fn start_item(&mut self) {
        self.flush_rich_block();
        let marker = match self.lists.last_mut().and_then(|list| list.next.as_mut()) {
            Some(next) => {
                let marker = format!("{next}. ");
                *next += 1;
                marker
            }
            None => "• ".to_string(),
        };
        self.items.push(ItemContext {
            marker,
            marker_style: self.style.list_marker,
            emitted: false,
        });
    }

    fn set_task_marker(&mut self, checked: bool) {
        if let Some(item) = self.items.last_mut() {
            item.marker = if checked { "☑ " } else { "☐ " }.to_string();
            item.marker_style = if checked {
                self.style.task_checked
            } else {
                self.style.task_unchecked
            };
        }
    }

    fn push_text(&mut self, text: &str) {
        if let Some(code) = self.code.as_mut() {
            code.source.push_str(text);
            return;
        }
        if self.links.is_empty() {
            let links = find_web_links(text);
            if !links.is_empty() {
                self.push_text_with_plain_links(text, links);
                return;
            }
        }
        self.push_styled(text, self.current_style());
    }

    fn push_text_with_plain_links(
        &mut self,
        text: &str,
        links: Vec<crate::web_link::WebLinkMatch>,
    ) {
        let mut cursor = 0usize;
        for link in links {
            let range = link.byte_range();
            self.push_styled(&text[cursor..range.start], self.current_style());

            let id = self.next_link_id;
            self.next_link_id = self.next_link_id.wrapping_add(1);
            self.links.push(LinkContext {
                id,
                destination: link.destination().to_string(),
            });
            self.push_inline_style(self.style.link_text);
            self.push_styled(&text[range.clone()], self.current_style());
            self.pop_inline_style();
            self.links.pop();
            cursor = range.end;
        }
        self.push_styled(&text[cursor..], self.current_style());
    }

    fn push_styled(&mut self, text: &str, style: Style) {
        if text.is_empty() {
            return;
        }
        let segment = Segment {
            text: text.to_string(),
            style,
            link: self.links.last().map(|link| SegmentLink {
                id: link.id,
                target: link.destination.clone(),
            }),
        };
        if self
            .table
            .as_mut()
            .is_some_and(|table| table.push(segment.clone()))
        {
            return;
        }
        self.segments.push(segment);
    }

    fn finish_link(&mut self) {
        self.pop_inline_style();
        self.links.pop();
    }

    fn finish_code_block(&mut self) {
        let Some(code) = self.code.take() else {
            return;
        };
        let source = code.source.strip_suffix('\n').unwrap_or(&code.source);
        let highlighted = code
            .fence_info
            .as_deref()
            .and_then(|fence_info| highlight_code(source, fence_info, self.style.syntax_theme));
        let mut rendered = Vec::new();
        for (index, source_line) in source.split('\n').enumerate() {
            let (initial_prefix, subsequent_prefix) = self.prefixes();
            rendered.extend(render_code_line(
                self.width,
                self.style,
                source_line,
                highlighted
                    .as_ref()
                    .and_then(|lines| lines.get(index))
                    .map(Vec::as_slice),
                initial_prefix,
                subsequent_prefix,
            ));
        }
        self.push_output(rendered, BlockKind::Code);
    }

    fn flush_rich_block(&mut self) {
        if self.segments.is_empty() {
            return;
        }
        let kind = if self.heading.is_some() {
            BlockKind::Heading
        } else if !self.items.is_empty() {
            BlockKind::ListItem
        } else if self.quote_depth > 0 {
            BlockKind::Quote
        } else {
            BlockKind::Paragraph
        };
        let (initial_prefix, subsequent_prefix) = self.prefixes();
        let prefix_width = Line::from(initial_prefix.clone()).width();
        let content_width = usize::from(self.width).saturating_sub(prefix_width).max(1);
        let segments = std::mem::take(&mut self.segments);
        let wrapped = wrap_segments_with_joiners(&segments, content_width);
        let lines = wrapped
            .into_iter()
            .enumerate()
            .map(|(index, mut wrapped)| {
                let mut spans = if index == 0 {
                    initial_prefix.clone()
                } else {
                    subsequent_prefix.clone()
                };
                let prefix_width =
                    u16::try_from(Line::from(spans.clone()).width()).unwrap_or(u16::MAX);
                for link in &mut wrapped.links {
                    link.columns = link.columns.start.saturating_add(prefix_width)
                        ..link.columns.end.saturating_add(prefix_width);
                }
                spans.append(&mut wrapped.spans);
                (Line::from(spans), wrapped.joiner_to_previous, wrapped.links)
            })
            .collect();
        self.push_wrapped_output(lines, kind);
    }

    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut initial = Vec::new();
        let mut subsequent = Vec::new();
        if self.quote_depth > 0 {
            let quote = format!("{} ", "│".repeat(self.quote_depth));
            initial.push(Span::styled(quote.clone(), self.style.blockquote));
            subsequent.push(Span::styled(quote, self.style.blockquote));
        }
        let item_depth = self.items.len();
        if let Some(item) = self.items.last_mut() {
            let indent = "  ".repeat(item_depth.saturating_sub(1));
            initial.push(Span::raw(indent.clone()));
            subsequent.push(Span::raw(indent));
            if item.emitted {
                let padding = " ".repeat(Line::from(item.marker.as_str()).width());
                initial.push(Span::raw(padding.clone()));
                subsequent.push(Span::raw(padding));
            } else {
                initial.push(Span::styled(item.marker.clone(), item.marker_style));
                subsequent.push(Span::raw(
                    " ".repeat(Line::from(item.marker.as_str()).width()),
                ));
                item.emitted = true;
            }
        }
        (initial, subsequent)
    }

    fn push_output(&mut self, lines: Vec<Line<'static>>, kind: BlockKind) {
        if lines.is_empty() {
            return;
        }
        self.insert_block_gap(kind);
        self.joiners
            .extend(std::iter::repeat_n(LineJoiner::HardBreak, lines.len()));
        self.line_links
            .extend(std::iter::repeat_n(Vec::new(), lines.len()));
        self.lines.extend(lines);
        self.last_kind = Some(kind);
    }

    fn push_wrapped_output(
        &mut self,
        lines: Vec<(Line<'static>, LineJoiner, Vec<MarkdownLink>)>,
        kind: BlockKind,
    ) {
        if lines.is_empty() {
            return;
        }
        self.insert_block_gap(kind);
        for (line, joiner, links) in lines {
            self.lines.push(line);
            self.joiners.push(joiner);
            self.line_links.push(links);
        }
        self.last_kind = Some(kind);
    }

    fn push_metadata_output(&mut self, lines: Vec<MarkdownLine>, kind: BlockKind) {
        if lines.is_empty() {
            return;
        }
        self.insert_block_gap(kind);
        for line in lines {
            self.lines.push(line.line);
            self.joiners.push(line.joiner_to_previous);
            self.line_links.push(line.links);
        }
        self.last_kind = Some(kind);
    }

    fn insert_block_gap(&mut self, kind: BlockKind) {
        let needs_blank = !self.lines.is_empty()
            && !matches!(
                (self.last_kind, kind),
                (Some(BlockKind::ListItem), BlockKind::ListItem)
            );
        if needs_blank && self.lines.last().is_some_and(|line| !line.spans.is_empty()) {
            self.lines.push(Line::default());
            self.joiners.push(LineJoiner::HardBreak);
            self.line_links.push(Vec::new());
        }
    }

    fn push_inline_style(&mut self, style: Style) {
        self.inline_styles.push(self.current_style().patch(style));
    }

    fn pop_inline_style(&mut self) {
        if self.inline_styles.len() > 1 {
            self.inline_styles.pop();
        }
    }

    fn current_style(&self) -> Style {
        self.inline_styles.last().copied().unwrap_or_default()
    }
}

fn heading_index(level: HeadingLevel) -> usize {
    (level as usize).saturating_sub(1).min(5)
}
