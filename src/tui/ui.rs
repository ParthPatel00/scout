use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SColor, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use super::App;

pub fn render(f: &mut Frame, app: &App) {
    let size = f.size();

    // Split into top (results + preview) and bottom (help bar).
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(size);

    // Split main area: 38% list, 62% preview.
    let content_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(main_chunks[0]);

    render_result_list(f, app, content_chunks[0]);
    render_preview(f, app, content_chunks[1]);
    render_help_bar(f, main_chunks[1]);
}

// ─── Result list ──────────────────────────────────────────────────────────────

fn render_result_list(f: &mut Frame, app: &App, area: Rect) {
    let title = format!(" Results ({}) ", app.results.len());
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let items: Vec<ListItem> = app
        .results
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let selected = i == app.selected;
            let rank_style = if selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let name_style = if selected {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Cyan)
            };
            let type_style = Style::default().fg(Color::Blue);

            let line = Line::from(vec![
                Span::styled(format!("{:>2}. ", i + 1), rank_style),
                Span::styled(format!("{:<8} ", r.unit.unit_type), type_style),
                Span::styled(r.unit.name.clone(), name_style),
            ]);
            ListItem::new(line)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.selected));

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(Color::DarkGray));

    f.render_stateful_widget(list, area, &mut state);
}

// ─── Preview pane ─────────────────────────────────────────────────────────────

fn render_preview(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let Some(result) = app.selected_result() else {
        f.render_widget(
            Paragraph::new("No results").block(block),
            area,
        );
        return;
    };

    let unit = &result.unit;
    let lang_ext = lang_to_ext(unit.language.as_str());

    // Header lines: location + score.
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled(
                format!("{}:{}", unit.file_path, unit.line_start),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("  [{:.1}]", result.score),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(Span::styled("─".repeat(area.width as usize), Style::default().fg(Color::DarkGray))),
    ];

    // Syntax-highlighted body or signature.
    // Prefer full body; fall back to signature header if body is empty.
    let source = if !unit.body.is_empty() {
        unit.body.as_str()
    } else {
        unit.full_signature.as_deref().unwrap_or("")
    };
    if !source.is_empty() {
        lines.extend(highlight_source(source, lang_ext));
    }

    // Docstring if available.
    if let Some(doc) = &unit.docstring {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {}", doc.lines().next().unwrap_or("")),
            Style::default().fg(Color::Green),
        )));
    }

    let para = Paragraph::new(Text::from(lines))
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.preview_scroll, 0));

    f.render_widget(para, area);
}

fn highlight_source<'a>(source: &str, lang_ext: &str) -> Vec<Line<'a>> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    let syntax = ss
        .find_syntax_by_extension(lang_ext)
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut h = HighlightLines::new(syntax, theme);

    LinesWithEndings::from(source)
        .filter_map(|line| {
            h.highlight_line(line, &ss).ok().map(|ranges| {
                let spans: Vec<Span<'static>> = ranges
                    .iter()
                    .map(|(style, text)| {
                        let fg = syntect_to_ratatui(style.foreground);
                        Span::styled(
                            text.trim_end_matches('\n').to_string(),
                            Style::default().fg(fg),
                        )
                    })
                    .collect();
                Line::from(spans)
            })
        })
        .collect()
}

fn syntect_to_ratatui(color: SColor) -> Color {
    Color::Rgb(color.r, color.g, color.b)
}

// ─── Help bar ─────────────────────────────────────────────────────────────────

fn render_help_bar(f: &mut Frame, area: Rect) {
    let key = |s| Span::styled(s, Style::default().fg(Color::Yellow));
    let dim = |s| Span::styled(s, Style::default().fg(Color::DarkGray));
    let help = Line::from(vec![
        key(" j/k"),
        dim(": navigate  "),
        key("Enter"),
        dim(": open in editor  "),
        key("o"),
        dim(": open (stay)  "),
        key("d/u"),
        dim(": scroll  "),
        key("q"),
        dim(": quit"),
    ]);
    f.render_widget(Paragraph::new(help), area);
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn lang_to_ext(lang: &str) -> &str {
    match lang {
        "python" => "py",
        "rust" => "rs",
        "typescript" => "ts",
        "javascript" => "js",
        "go" => "go",
        "java" => "java",
        "cpp" => "cpp",
        _ => "txt",
    }
}
