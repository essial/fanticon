//! In-application help: the F1 finder's data model, plus the console `HELP`
//! command's lookup.
//!
//! There are two kinds of content, deliberately stored differently:
//!
//! - Reference cards (opcodes, directives, console commands, editor
//!   shortcuts) are short and hand-authored in `documentation/help/*.help`,
//!   a small line-oriented record format matching the project's existing
//!   `KEY=VALUE`-style parsers (see `ProjectManifest::parse`). No new
//!   dependency is pulled in to read them.
//! - Guides are not duplicated here at all. They are the existing prose docs
//!   in `documentation/*.md`, embedded verbatim and indexed by heading at
//!   startup, so the finder can jump straight to a section (e.g. "bank
//!   switching") instead of surfacing an entire multi-page file as one hit.
//!   Wide markdown tables are lightly reflowed for the 80-column display;
//!   backticks are stripped because the character ROM has no glyph for them.
//!
//! Both are merged into one [`HelpIndex`] so the F1 finder searches
//! everything with a single fuzzy query.

use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum HelpCategory {
    Opcode,
    Directive,
    Command,
    Shortcut,
    Guide,
}

impl HelpCategory {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Opcode => "OPCODE",
            Self::Directive => "DIRECTIVE",
            Self::Command => "COMMAND",
            Self::Shortcut => "SHORTCUT",
            Self::Guide => "GUIDE",
        }
    }
}

#[derive(Clone, Debug)]
pub struct HelpEntry {
    pub key: String,
    pub category: HelpCategory,
    pub aliases: Vec<String>,
    pub summary: String,
    pub body: Vec<String>,
    /// Source document, guide entries only (e.g. `"AUDIO.MD"`).
    pub source: Option<String>,
}

impl HelpEntry {
    fn score(&self, query: &str) -> Option<i32> {
        let mut best = fuzzy_score(query, &self.key).map(|score| score + 30);
        for alias in &self.aliases {
            best = better(best, fuzzy_score(query, alias).map(|score| score + 15));
        }
        best = better(best, fuzzy_score(query, &self.summary));
        best
    }
}

fn better(a: Option<i32>, b: Option<i32>) -> Option<i32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(value), None) | (None, Some(value)) => Some(value),
        (None, None) => None,
    }
}

/// A small subsequence fuzzy matcher in the spirit of fzf: every query
/// character must appear in the candidate in order, case-insensitively.
/// Contiguous runs and an early first match score higher, so searching
/// "lda" ranks the `LDA` opcode above a guide paragraph that merely
/// contains those letters somewhere.
fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let query: Vec<char> = query.trim().to_ascii_uppercase().chars().collect();
    if query.is_empty() {
        return None;
    }
    let candidate: Vec<char> = candidate.to_ascii_uppercase().chars().collect();
    let mut score = 0i32;
    let mut cursor = 0usize;
    let mut previous_index: Option<usize> = None;
    for &query_char in &query {
        let found = candidate[cursor..].iter().position(|&c| c == query_char)?;
        let index = cursor + found;
        score += match previous_index {
            Some(previous) if index == previous + 1 => 15,
            _ => 5,
        };
        if index == 0 {
            score += 10;
        }
        previous_index = Some(index);
        cursor = index + 1;
    }
    // A candidate that is mostly the query (little left over) ranks a
    // little higher than one where the match is buried in a long string.
    let leftover = candidate.len().saturating_sub(query.len()) as i32;
    score -= leftover.min(20);
    Some(score)
}

/// Parses the small `.help` record format used for reference cards:
///
/// ```text
/// KEY: LDA
/// CATEGORY: OPCODE
/// ALIASES: LOAD ACCUMULATOR
/// SUMMARY: One line shown in the finder's list and status-bar gloss.
/// BODY:
/// Free-form lines shown verbatim in the preview pane. Alignment is
/// preserved, so tables here are not word-wrapped.
/// ---
/// ```
///
/// Records are separated by a line containing only `---`. `KEY` and
/// `SUMMARY` are required; `ALIASES` and `BODY` are optional. Malformed or
/// incomplete records are silently dropped rather than panicking, since a
/// missing card is a content bug, not a reason to fail the whole app.
fn parse_records(source: &str, category: HelpCategory) -> Vec<HelpEntry> {
    let mut entries = Vec::new();
    let mut key: Option<String> = None;
    let mut aliases: Vec<String> = Vec::new();
    let mut summary: Option<String> = None;
    let mut body: Vec<String> = Vec::new();
    let mut in_body = false;

    for raw_line in source.lines().chain(std::iter::once("---")) {
        let line = raw_line.trim_end();
        if line.trim() == "---" {
            if let (Some(entry_key), Some(entry_summary)) = (key.take(), summary.take()) {
                while body.last().is_some_and(|line: &String| line.trim().is_empty()) {
                    body.pop();
                }
                entries.push(HelpEntry {
                    key: entry_key,
                    category,
                    aliases: std::mem::take(&mut aliases),
                    summary: entry_summary,
                    body: std::mem::take(&mut body),
                    source: None,
                });
            } else {
                aliases.clear();
                body.clear();
            }
            in_body = false;
            continue;
        }
        if in_body {
            body.push(line.to_owned());
            continue;
        }
        if line.trim() == "BODY:" {
            in_body = true;
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let Some((field, value)) = line.split_once(':') else { continue };
        let value = value.trim().to_owned();
        match field.trim() {
            "KEY" => key = Some(value),
            "ALIASES" => {
                aliases = value
                    .split(',')
                    .map(|part| part.trim().to_owned())
                    .filter(|part| !part.is_empty())
                    .collect();
            }
            "SUMMARY" => summary = Some(value),
            _ => {}
        }
    }
    entries
}

/// Splits a doc into one guide entry per `#`/`##`/`###` heading, so the
/// finder can jump straight to a section instead of the whole file.
fn extract_guide_sections(source_name: &str, source: &str) -> Vec<HelpEntry> {
    let mut entries = Vec::new();
    let mut heading: Option<String> = None;
    let mut body: Vec<String> = Vec::new();

    for raw_line in source.lines() {
        if let Some(title) = heading_title(raw_line) {
            push_guide_section(&mut entries, source_name, heading.take(), &mut body);
            heading = Some(title);
            continue;
        }
        body.push(raw_line.to_owned());
    }
    push_guide_section(&mut entries, source_name, heading, &mut body);
    entries
}

fn heading_title(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    for prefix in ["### ", "## ", "# "] {
        if let Some(stripped) = trimmed.strip_prefix(prefix) {
            return Some(stripped.trim().to_owned());
        }
    }
    None
}

fn push_guide_section(
    entries: &mut Vec<HelpEntry>,
    source_name: &str,
    heading: Option<String>,
    body: &mut Vec<String>,
) {
    while body.last().is_some_and(|line: &String| line.trim().is_empty()) {
        body.pop();
    }
    while body.first().is_some_and(|line: &String| line.trim().is_empty()) {
        body.remove(0);
    }
    let Some(title) = heading else {
        body.clear();
        return;
    };
    if body.is_empty() {
        return;
    }
    let summary = body
        .iter()
        .find(|line| !line.trim().is_empty())
        .map(|line| line.replace('`', "").trim().to_owned())
        .unwrap_or_default();
    entries.push(HelpEntry {
        key: title,
        category: HelpCategory::Guide,
        aliases: Vec::new(),
        summary,
        body: std::mem::take(body),
        source: Some(source_name.to_owned()),
    });
}

/// A logical unit of guide content, after joining markdown's hard-wrapped
/// source lines back into whole paragraphs/list items/table rows. Docs are
/// written with each line manually wrapped near 80 columns; treating every
/// source line as independent (the previous approach) meant a bullet's
/// second line lost its indent and looked like an unrelated new line. This
/// groups by markdown's actual block structure instead, so reflow to the
/// pane's width is correct regardless of how the source happens to be
/// wrapped.
enum GuideBlock {
    Blank,
    Paragraph(String),
    ListItem(String, String),
    Table(Vec<String>),
    Code(String),
}

/// Groups raw markdown lines into logical blocks: consecutive non-blank
/// lines with no block marker of their own are joined into one paragraph or
/// list item (markdown's "soft wrap" rule), table rows accumulate until the
/// table ends, and fenced code blocks (` ``` `) are preserved verbatim and
/// never joined or wrapped.
#[allow(unused_assignments)]
fn guide_blocks(lines: &[String]) -> Vec<GuideBlock> {
    #[derive(PartialEq)]
    enum Mode {
        None,
        Paragraph,
        List,
        Table,
    }
    let mut blocks = Vec::new();
    let mut mode = Mode::None;
    let mut buffer = String::new();
    let mut marker = String::new();
    let mut table_rows: Vec<String> = Vec::new();
    let mut in_fence = false;

    macro_rules! flush {
        () => {
            match &mode {
                Mode::Paragraph if !buffer.is_empty() => {
                    blocks.push(GuideBlock::Paragraph(std::mem::take(&mut buffer)))
                }
                Mode::List if !buffer.is_empty() => blocks.push(GuideBlock::ListItem(
                    std::mem::take(&mut marker),
                    std::mem::take(&mut buffer),
                )),
                Mode::Table if !table_rows.is_empty() => {
                    blocks.push(GuideBlock::Table(std::mem::take(&mut table_rows)))
                }
                _ => {}
            }
            buffer.clear();
            marker.clear();
            table_rows.clear();
            mode = Mode::None;
        };
    }

    for raw_line in lines {
        if raw_line.trim().starts_with("```") {
            flush!();
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            flush!();
            blocks.push(GuideBlock::Code(raw_line.clone()));
            continue;
        }
        // Backticks have no glyph in the character ROM (they render as a
        // gap), so inline `code` spans are stripped here, before wrapping.
        let line = raw_line.replace('`', "");
        if line.trim().is_empty() {
            flush!();
            blocks.push(GuideBlock::Blank);
            continue;
        }
        if is_table_row(&line) {
            if mode != Mode::Table {
                flush!();
                mode = Mode::Table;
            }
            table_rows.push(line);
            continue;
        }
        if let Some((item_marker, text)) = list_marker(&line) {
            flush!();
            marker = item_marker;
            buffer = text.trim().to_owned();
            mode = Mode::List;
            continue;
        }
        if mode == Mode::Paragraph || mode == Mode::List {
            if !buffer.is_empty() {
                buffer.push(' ');
            }
            buffer.push_str(line.trim());
        } else {
            flush!();
            mode = Mode::Paragraph;
            buffer = line.trim().to_owned();
        }
    }
    flush!();
    blocks
}

fn is_table_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.len() > 1 && trimmed.starts_with('|') && trimmed.ends_with('|')
}

fn parse_table_cells(line: &str) -> Vec<String> {
    line.trim().trim_matches('|').split('|').map(|cell| cell.trim().to_owned()).collect()
}

fn is_separator_cells(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| !cell.is_empty() && cell.chars().all(|c| c == '-' || c == ':'))
}

/// Recognizes `- `, `* `, `+ `, and `1. `/`1) ` list markers (preserving
/// leading indentation, for nested lists), returning the marker text to
/// prefix the first wrapped line and the remaining text to wrap.
fn list_marker(line: &str) -> Option<(String, String)> {
    let stripped = line.trim_start_matches(' ');
    let indent = line.len() - stripped.len();
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = stripped.strip_prefix(bullet) {
            return Some((format!("{}- ", " ".repeat(indent)), rest.to_owned()));
        }
    }
    let digits_end = stripped.find(|character: char| !character.is_ascii_digit())?;
    if digits_end == 0 {
        return None;
    }
    let (number, rest) = stripped.split_at(digits_end);
    let rest = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") "))?;
    Some((format!("{}{number}. ", " ".repeat(indent)), rest.to_owned()))
}

/// Word-wraps a single logical line (already joined from any hard-wrapped
/// source lines) to `width` columns.
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Renders a markdown table. If every column fits `width` when aligned side
/// by side, it renders as an ordinary ruled grid; otherwise (most tables in
/// these docs, at F1's preview-pane width) it falls back to one wrapped
/// `Header: value` line per cell, which stays readable at any width instead
/// of wrapping mid-row and destroying the column alignment.
fn render_table_block(rows: &[String], width: usize) -> Vec<String> {
    let parsed: Vec<Vec<String>> = rows
        .iter()
        .map(|row| parse_table_cells(row))
        .filter(|cells| !is_separator_cells(cells))
        .collect();
    let Some((header, data)) = parsed.split_first() else { return Vec::new() };
    let columns = header.len();
    let mut widths = vec![0usize; columns];
    for row in &parsed {
        for (index, cell) in row.iter().enumerate().take(columns) {
            widths[index] = widths[index].max(cell.len());
        }
    }
    let total = widths.iter().sum::<usize>() + 3 * columns.saturating_sub(1);
    let mut output = Vec::new();
    if total <= width {
        let format_row = |row: &[String]| {
            row.iter()
                .enumerate()
                .map(|(index, cell)| format!("{:<width$}", cell, width = widths[index]))
                .collect::<Vec<_>>()
                .join(" | ")
        };
        output.push(format_row(header));
        output.push("-".repeat(total.min(width)));
        for row in data {
            output.push(format_row(row));
        }
    } else {
        // One blank line between records, or consecutive rows (e.g. every
        // register in a register table) run together with no visual
        // boundary between where one ends and the next begins.
        for (row_index, row) in data.iter().enumerate() {
            if row_index > 0 {
                output.push(String::new());
            }
            for (index, cell) in row.iter().enumerate().take(header.len()) {
                if cell.is_empty() {
                    continue;
                }
                let prefix = format!("{}: ", header[index]);
                let indent = prefix.len();
                for (line_index, line) in
                    wrap_text(cell, width.saturating_sub(indent)).into_iter().enumerate()
                {
                    if line_index == 0 {
                        output.push(format!("{prefix}{line}"));
                    } else {
                        output.push(format!("{}{line}", " ".repeat(indent)));
                    }
                }
            }
        }
    }
    output
}

fn render_guide_blocks(blocks: &[GuideBlock], width: usize) -> Vec<String> {
    let mut output = Vec::new();
    let mut previous_blank = true;
    for block in blocks {
        match block {
            GuideBlock::Blank => {
                if !previous_blank {
                    output.push(String::new());
                }
                previous_blank = true;
            }
            GuideBlock::Paragraph(text) => {
                output.extend(wrap_text(text, width));
                previous_blank = false;
            }
            GuideBlock::ListItem(marker, text) => {
                let indent = marker.len();
                for (index, line) in
                    wrap_text(text, width.saturating_sub(indent)).into_iter().enumerate()
                {
                    if index == 0 {
                        output.push(format!("{marker}{line}"));
                    } else {
                        output.push(format!("{}{line}", " ".repeat(indent)));
                    }
                }
                previous_blank = false;
            }
            GuideBlock::Table(rows) => {
                output.extend(render_table_block(rows, width));
                output.push(String::new());
                previous_blank = true;
            }
            GuideBlock::Code(line) => {
                output.push(format!("  {line}"));
                previous_blank = false;
            }
        }
    }
    while output.last().is_some_and(|line: &String| line.is_empty()) {
        output.pop();
    }
    output
}

/// Formats a guide entry's raw markdown body for display at `width`
/// columns: paragraphs and list items are reflowed as whole logical units
/// (not per hard-wrapped source line), tables render as an aligned grid or,
/// when too wide, as wrapped `Header: value` records, and fenced code is
/// left verbatim. Reference-card bodies never go through this: they are
/// hand-formatted tables and reflowing them would destroy their alignment.
pub fn format_guide_body(lines: &[String], width: usize) -> Vec<String> {
    render_guide_blocks(&guide_blocks(lines), width)
}

const COMMANDS_HELP: &str = include_str!("../../documentation/help/commands.help");
const DIRECTIVES_HELP: &str = include_str!("../../documentation/help/directives.help");
const OPCODES_HELP: &str = include_str!("../../documentation/help/opcodes.help");
const SHORTCUTS_HELP: &str = include_str!("../../documentation/help/shortcuts.help");

const GUIDE_DOCS: &[(&str, &str)] = &[
    ("6502.MD", include_str!("../../documentation/6502.md")),
    ("AUDIO.MD", include_str!("../../documentation/audio.md")),
    ("VIDEO.MD", include_str!("../../documentation/video.md")),
    ("EDITOR.MD", include_str!("../../documentation/editor.md")),
    ("GRAPHICS-EDITOR.MD", include_str!("../../documentation/graphics-editor.md")),
    ("MUSIC-EDITOR.MD", include_str!("../../documentation/music-editor.md")),
    ("ASSEMBLER.MD", include_str!("../../documentation/assembler.md")),
    ("SYSTEM-ARCHITECTURE.MD", include_str!("../../documentation/system-architecture.md")),
    ("MEMORY-MAP.MD", include_str!("../../documentation/memory-map.md")),
    ("CARTRIDGE-FORMAT.MD", include_str!("../../documentation/cartridge-format.md")),
    ("CARTRIDGE-PROJECTS.MD", include_str!("../../documentation/cartridge-projects.md")),
];

pub struct HelpIndex {
    entries: Vec<HelpEntry>,
}

impl HelpIndex {
    fn load() -> Self {
        let mut entries = Vec::new();
        entries.extend(parse_records(COMMANDS_HELP, HelpCategory::Command));
        entries.extend(parse_records(DIRECTIVES_HELP, HelpCategory::Directive));
        entries.extend(parse_records(OPCODES_HELP, HelpCategory::Opcode));
        entries.extend(parse_records(SHORTCUTS_HELP, HelpCategory::Shortcut));
        for (name, source) in GUIDE_DOCS {
            entries.extend(extract_guide_sections(name, source));
        }
        Self { entries }
    }

    /// Live-filter search for the F1 finder. Empty (or whitespace-only)
    /// queries return no results — the finder starts blank until the user
    /// types, by design.
    pub fn search(&self, query: &str) -> Vec<&HelpEntry> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(i32, &HelpEntry)> = self
            .entries
            .iter()
            .filter_map(|entry| entry.score(query).map(|score| (score, entry)))
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.key.cmp(&b.1.key)));
        scored.into_iter().take(60).map(|(_, entry)| entry).collect()
    }

    /// Exact-or-alias lookup for the console `HELP TOPIC` command, which has
    /// no interactive list to filter through.
    pub fn lookup(&self, key: &str) -> Option<&HelpEntry> {
        let key = key.trim();
        if key.is_empty() {
            return None;
        }
        self.entries.iter().find(|entry| entry.key.eq_ignore_ascii_case(key)).or_else(|| {
            self.entries
                .iter()
                .find(|entry| entry.aliases.iter().any(|alias| alias.eq_ignore_ascii_case(key)))
        })
    }

    /// The token under the editor cursor gets an ambient one-line gloss with
    /// no popup at all; this is the zero-keystroke lookup tier.
    pub fn ambient_gloss(&self, token: &str) -> Option<&HelpEntry> {
        let token = token.trim_end_matches(':').trim_start_matches('.');
        if token.is_empty() {
            return None;
        }
        self.entries.iter().find(|entry| {
            matches!(entry.category, HelpCategory::Opcode | HelpCategory::Directive)
                && entry.key.eq_ignore_ascii_case(token)
        })
    }
}

static HELP_INDEX: OnceLock<HelpIndex> = OnceLock::new();

pub fn shared_help_index() -> &'static HelpIndex {
    HELP_INDEX.get_or_init(HelpIndex::load)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_minimal_record() {
        let source = "KEY: LDA\nSUMMARY: Load Accumulator\nBODY:\nline one\nline two\n---\n";
        let entries = parse_records(source, HelpCategory::Opcode);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "LDA");
        assert_eq!(entries[0].summary, "Load Accumulator");
        assert_eq!(entries[0].body, vec!["line one".to_owned(), "line two".to_owned()]);
    }

    #[test]
    fn parses_aliases_and_multiple_records() {
        let source = "KEY: LDA\nALIASES: LOAD ACCUMULATOR, LOAD A\nSUMMARY: A\n---\nKEY: STA\nSUMMARY: B\n---\n";
        let entries = parse_records(source, HelpCategory::Opcode);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].aliases, vec!["LOAD ACCUMULATOR".to_owned(), "LOAD A".to_owned()]);
        assert_eq!(entries[1].key, "STA");
    }

    #[test]
    fn drops_records_missing_required_fields() {
        let source = "KEY: ORPHAN\n---\nKEY: LDA\nSUMMARY: A\n---\n";
        let entries = parse_records(source, HelpCategory::Opcode);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, "LDA");
    }

    #[test]
    fn fuzzy_score_requires_in_order_subsequence() {
        assert!(fuzzy_score("lda", "LDA").is_some());
        assert!(fuzzy_score("lax", "LDA").is_none());
        assert!(fuzzy_score("da", "LDA").is_some());
        assert!(fuzzy_score("", "LDA").is_none());
    }

    #[test]
    fn fuzzy_score_prefers_contiguous_and_early_matches() {
        let exact = fuzzy_score("lda", "LDA").unwrap();
        let scattered = fuzzy_score("lda", "LOAD ACCUMULATOR").unwrap();
        assert!(exact > scattered);
    }

    #[test]
    fn extracts_guide_sections_by_heading() {
        let doc = "# Title\nintro line\n## First\nbody one\nbody two\n## Second\nbody three\n";
        let entries = extract_guide_sections("TEST.MD", doc);
        let titles: Vec<&str> = entries.iter().map(|entry| entry.key.as_str()).collect();
        assert_eq!(titles, vec!["Title", "First", "Second"]);
        assert_eq!(entries[1].body, vec!["body one".to_owned(), "body two".to_owned()]);
        assert!(entries.iter().all(|entry| entry.source.as_deref() == Some("TEST.MD")));
    }

    #[test]
    fn joins_hard_wrapped_source_lines_before_reflowing() {
        // Docs hard-wrap prose near 80 columns in the source file; a single
        // logical sentence spans two physical lines with no blank line
        // between them. The old per-line approach rewrapped each physical
        // line independently and lost this, which is exactly what looked
        // "hard to read": a bullet's continuation line lost its indent and
        // read like an unrelated new line.
        let lines = vec!["Use `LDA` to load".to_owned(), "the accumulator.".to_owned()];
        let out = format_guide_body(&lines, 80);
        assert_eq!(out, vec!["Use LDA to load the accumulator.".to_owned()]);
    }

    #[test]
    fn list_items_wrap_with_a_hanging_indent() {
        let lines = vec!["- one two three four five six seven eight".to_owned()];
        let out = format_guide_body(&lines, 12);
        assert_eq!(
            out,
            vec![
                "- one two".to_owned(),
                "  three four".to_owned(),
                "  five six".to_owned(),
                "  seven".to_owned(),
                "  eight".to_owned(),
            ]
        );
    }

    #[test]
    fn narrow_table_renders_as_an_aligned_grid() {
        let lines =
            vec!["| A | B |".to_owned(), "| --- | --- |".to_owned(), "| 1 | 22 |".to_owned()];
        let out = format_guide_body(&lines, 40);
        assert_eq!(out, vec!["A | B ".to_owned(), "------".to_owned(), "1 | 22".to_owned()]);
    }

    #[test]
    fn wide_table_falls_back_to_wrapped_records() {
        let lines = vec![
            "| Name | Description |".to_owned(),
            "| --- | --- |".to_owned(),
            "| Alpha | one two three four five |".to_owned(),
        ];
        let out = format_guide_body(&lines, 24);
        assert_eq!(
            out,
            vec![
                "Name: Alpha".to_owned(),
                "Description: one two".to_owned(),
                "             three four".to_owned(),
                "             five".to_owned(),
            ]
        );
    }

    #[test]
    fn fenced_code_is_preserved_verbatim() {
        let lines = vec![
            "```asm".to_owned(),
            "lda #$01".to_owned(),
            "sta $2000".to_owned(),
            "```".to_owned(),
            "after the block".to_owned(),
        ];
        let out = format_guide_body(&lines, 40);
        assert_eq!(
            out,
            vec!["  lda #$01".to_owned(), "  sta $2000".to_owned(), "after the block".to_owned()]
        );
    }

    #[test]
    fn empty_query_returns_no_results() {
        let index = HelpIndex {
            entries: vec![HelpEntry {
                key: "LDA".to_owned(),
                category: HelpCategory::Opcode,
                aliases: Vec::new(),
                summary: "Load Accumulator".to_owned(),
                body: Vec::new(),
                source: None,
            }],
        };
        assert!(index.search("").is_empty());
        assert!(index.search("   ").is_empty());
        assert_eq!(index.search("lda").len(), 1);
    }

    #[test]
    fn lookup_matches_key_or_alias_case_insensitively() {
        let index = HelpIndex {
            entries: vec![HelpEntry {
                key: "LDA".to_owned(),
                category: HelpCategory::Opcode,
                aliases: vec!["LOAD ACCUMULATOR".to_owned()],
                summary: "Load Accumulator".to_owned(),
                body: Vec::new(),
                source: None,
            }],
        };
        assert!(index.lookup("lda").is_some());
        assert!(index.lookup("Load Accumulator").is_some());
        assert!(index.lookup("stx").is_none());
    }

    #[test]
    fn integrated_help_covers_fanticon_emitters_and_fixed_placement() {
        let index = HelpIndex::load();

        let helper = index.lookup("set_audio_master").unwrap();
        assert_eq!(helper.key, "FANTICON.INC");
        assert!(helper.body.iter().any(|line| line.contains("READ-ONLY")));

        for name in ["EMIT_VRAM_COPY", "EMIT_PAD_SCROLL"] {
            let emitter = index.lookup(name).unwrap();
            assert_eq!(emitter.key, name);
            assert!(emitter.body.iter().any(|line| line.contains("REQUIRE_FIXED")));
        }

        let assertion = index.lookup("require_fixed").unwrap();
        assert!(assertion.body.iter().any(|line| line.contains("BANK n")));

        let emitter_guide = index.lookup("Procedure emitters").unwrap();
        assert_eq!(emitter_guide.source.as_deref(), Some("ASSEMBLER.MD"));
        assert!(emitter_guide.body.iter().any(|line| line.contains("REQUIRE_FIXED")));

        let section_guide = index.lookup("ROM sections").unwrap();
        assert_eq!(section_guide.source.as_deref(), Some("CARTRIDGE-PROJECTS.MD"));
        assert!(section_guide.body.iter().any(|line| line.contains("REQUIRE_FIXED")));
    }
}
