use std::ops::Range;

use ratatui::layout::{Alignment, Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus, TabHit};
use crate::editor::{char_width, Editor, TAB_WIDTH};

// Colors modeled on VSCode's Dark Modern theme.
const SELECTION: Color = Color::Rgb(38, 79, 120);
const GUTTER: Style = Style::new().fg(Color::Rgb(110, 118, 129));
const GUTTER_CURRENT: Style = Style::new().fg(Color::Rgb(204, 204, 204));
const STATUS: Style = Style::new().fg(Color::White).bg(Color::Rgb(0, 122, 204));
const BORDER: Style = Style::new().fg(Color::Rgb(60, 60, 60));
const SIDEBAR_TITLE: Style = Style::new().fg(Color::Gray);
const LIST_FOCUSED: Style = Style::new().fg(Color::White).bg(Color::Rgb(4, 57, 94));
const LIST_UNFOCUSED: Style = Style::new().bg(Color::Rgb(55, 55, 61));
const TABBAR: Style = Style::new().bg(Color::Rgb(37, 37, 38));
const TAB_INACTIVE: Style = Style::new().fg(Color::Gray).bg(Color::Rgb(45, 45, 45));
const TAB_ACTIVE: Style = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
const DIM: Style = Style::new().fg(Color::Rgb(140, 140, 140));
const HINT_KEY: Style = Style::new().fg(Color::Gray);

pub fn draw(f: &mut Frame, app: &mut App) {
    let [main, status] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(f.area());

    let rest = if app.show_sidebar {
        let w = (main.width / 4).clamp(20, 40).min(main.width / 2);
        let [side, rest] =
            Layout::horizontal([Constraint::Length(w), Constraint::Min(0)]).areas(main);
        draw_sidebar(f, app, side);
        rest
    } else {
        app.sidebar_area = Rect::default();
        main
    };

    let [tabbar, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(rest);
    draw_tabs(f, app, tabbar);

    let focused = app.focus == Focus::Editor && app.input.is_none();
    match app.tabs.get_mut(app.active) {
        Some(ed) => draw_editor(f, ed, body, focused),
        None => draw_welcome(f, body),
    }
    draw_status(f, app, status);
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    app.sidebar_area = area;
    let block = Block::new().borders(Borders::RIGHT).border_style(BORDER);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [title, root, list] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(inner);
    f.render_widget(Paragraph::new(" EXPLORER").style(SIDEBAR_TITLE), title);
    f.render_widget(
        Paragraph::new(format!(" ▾ {}", app.tree.root_name().to_uppercase()))
            .style(Style::new().add_modifier(Modifier::BOLD)),
        root,
    );

    app.tree_area = list;
    let tree = &app.tree;
    let selected_style = if app.focus == Focus::Explorer {
        LIST_FOCUSED
    } else {
        LIST_UNFOCUSED
    };
    let lines: Vec<Line> = tree
        .entries
        .iter()
        .enumerate()
        .skip(tree.scroll)
        .take(list.height as usize)
        .map(|(i, e)| {
            let icon = match (e.is_dir, tree.is_expanded(&e.path)) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                (false, _) => "  ",
            };
            let text = format!(" {}{icon}{}", "  ".repeat(e.depth + 1), e.name);
            if i == tree.selected {
                Line::styled(pad(text, list.width as usize), selected_style)
            } else {
                Line::raw(text)
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), list);
}

fn pad(text: String, width: usize) -> String {
    let n = width.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(n))
}

fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    app.tabs_area = area;
    app.tab_hits.clear();
    f.render_widget(Block::new().style(TABBAR), area);

    let labels: Vec<(String, &str)> = app
        .tabs
        .iter()
        .map(|t| (t.buf.name(), if t.buf.dirty { "●" } else { "×" }))
        .collect();
    let widths: Vec<u16> = labels.iter().map(|(n, _)| (n.width() + 4) as u16).collect();

    // Elide tabs on the left so the active tab stays visible.
    let mut first = 0;
    while first < app.active && widths[first..=app.active].iter().sum::<u16>() > area.width {
        first += 1;
    }

    let mut x = area.x;
    let mut spans = Vec::new();
    for (idx, (name, mark)) in labels.iter().enumerate().skip(first) {
        let w = widths[idx];
        if x + w > area.right() {
            break;
        }
        let style = if idx == app.active {
            TAB_ACTIVE
        } else {
            TAB_INACTIVE
        };
        spans.push(Span::styled(format!(" {name} {mark} "), style));
        app.tab_hits.push(TabHit {
            x: x..x + w,
            close_x: x + w - 2,
            idx,
        });
        x += w;
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_editor(f: &mut Frame, ed: &mut Editor, area: Rect, focused: bool) {
    let digits = ed.buf.rope.len_lines().to_string().len();
    let [gutter, text] =
        Layout::horizontal([Constraint::Length((digits + 2) as u16), Constraint::Min(0)])
            .areas(area);
    ed.view = text;

    let sel = ed.selection();
    let cur_line = ed.cursor_line();
    let line_count = ed.buf.rope.len_lines();
    let rows = (text.height as usize).min(line_count.saturating_sub(ed.scroll_y));

    let mut gutter_lines = Vec::with_capacity(rows);
    let mut text_lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let li = ed.scroll_y + row;
        let style = if li == cur_line {
            GUTTER_CURRENT
        } else {
            GUTTER
        };
        gutter_lines.push(Line::styled(
            format!("{:>w$} ", li + 1, w = digits + 1),
            style,
        ));
        text_lines.push(render_line(ed, li, sel.as_ref(), text.width as usize));
    }
    f.render_widget(Paragraph::new(gutter_lines), gutter);
    f.render_widget(Paragraph::new(text_lines), text);

    if focused {
        let row = cur_line.checked_sub(ed.scroll_y);
        let col = ed.display_col(ed.cursor).checked_sub(ed.scroll_x);
        if let (Some(row), Some(col)) = (row, col) {
            if row < text.height as usize && col < text.width as usize {
                f.set_cursor_position(Position::new(text.x + col as u16, text.y + row as u16));
            }
        }
    }
}

/// Builds the spans for one line with selection and horizontal scroll.
fn render_line(ed: &Editor, li: usize, sel: Option<&Range<usize>>, width: usize) -> Line<'static> {
    let rope = &ed.buf.rope;
    let start = rope.line_to_char(li);
    let len = ed.buf.line_len(li);
    let is_selected = |pos: usize| sel.is_some_and(|r| r.contains(&pos));

    let mut spans = Vec::new();
    let mut chunk = String::new();
    let mut chunk_style = Style::new();
    let mut col = 0;
    for (i, c) in rope.line(li).chars().take(len).enumerate() {
        let mut style = Style::new();
        let w = char_width(c);
        // Skip a wide char cut off at the left edge.
        if col < ed.scroll_x {
            col += w;
            continue;
        }
        if col + w > ed.scroll_x + width {
            break;
        }
        let pos = start + i;
        if is_selected(pos) {
            style = style.bg(SELECTION);
        }
        if style != chunk_style && !chunk.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut chunk), chunk_style));
        }
        chunk_style = style;
        match c {
            '\t' => chunk.push_str(&" ".repeat(TAB_WIDTH)),
            c if c.is_control() => chunk.push('?'),
            c => chunk.push(c),
        }
        col += w;
    }
    if !chunk.is_empty() {
        spans.push(Span::styled(chunk, chunk_style));
    }
    // Highlight one cell at line end if the newline is selected.
    if is_selected(start + len) && start + len < rope.len_chars() {
        spans.push(Span::styled(" ", Style::new().bg(SELECTION)));
    }
    Line::from(spans)
}

/// Shown when no tabs are open; lists the main shortcuts.
fn draw_welcome(f: &mut Frame, area: Rect) {
    const KEYS: &[(&str, &str)] = &[
        ("Ctrl+E", "Focus Explorer / Editor"),
        ("Ctrl+B", "Toggle Sidebar"),
        ("Ctrl+N", "New File"),
        ("Ctrl+S", "Save"),
        ("Ctrl+W", "Close Tab"),
        ("Ctrl+Q", "Quit"),
    ];
    let mut lines = vec![
        Line::styled("Vin", Style::new().add_modifier(Modifier::BOLD)),
        Line::styled("Select a file in the Explorer with ↑↓ and Enter", DIM),
        Line::raw(""),
    ];
    lines.extend(KEYS.iter().map(|(k, d)| {
        Line::from(vec![
            Span::styled(format!("{k:>10}  "), HINT_KEY),
            Span::styled(format!("{d:<24}"), DIM),
        ])
    }));
    let top = area.height.saturating_sub(lines.len() as u16) / 2;
    let area = Rect {
        y: area.y + top,
        height: area.height - top,
        ..area
    };
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let ed = app.active_editor();

    // Show an input field if any (Explorer file operations).
    if let Some(input) = &app.input {
        return draw_prompt(f, area, &[(input.label(), &input.text, true)], "");
    }

    let message = app.message.as_ref().or(ed.and_then(|e| e.message.as_ref()));
    let left = match (message, ed) {
        (Some(m), _) => format!(" {m}"),
        (None, Some(ed)) => format!(" {}{}", ed.buf.name(), if ed.buf.dirty { " ●" } else { "" }),
        (None, None) => format!(" {}", app.tree.root_name()),
    };
    let right = match ed {
        Some(ed) => {
            let selected = ed
                .selection()
                .map_or(String::new(), |r| format!(" ({} selected)", r.len()));
            let indent = if ed.buf.indent == "\t" {
                format!("Tab Size: {TAB_WIDTH}")
            } else {
                format!("Spaces: {}", ed.buf.indent.len())
            };
            format!(
                "Ln {}, Col {}{selected}   {indent}   UTF-8   {} ",
                ed.cursor_line() + 1,
                ed.display_col(ed.cursor) + 1,
                if ed.buf.crlf { "CRLF" } else { "LF" },
            )
        }
        None => String::new(),
    };
    let text = format!(
        "{left}{}{right}",
        " ".repeat((area.width as usize).saturating_sub(left.width() + right.width()))
    );
    f.render_widget(Paragraph::new(text).style(STATUS), area);
}

/// Status bar input. fields are (label, value, editing).
fn draw_prompt(f: &mut Frame, area: Rect, fields: &[(&str, &str, bool)], hint: &str) {
    let mut spans = Vec::new();
    let mut x = area.x;
    let mut cursor = None;
    for (label, value, active) in fields {
        spans.push(Span::styled(
            label.to_string(),
            STATUS.add_modifier(Modifier::BOLD),
        ));
        let style = if *active {
            Style::new().add_modifier(Modifier::UNDERLINED)
        } else {
            DIM
        };
        spans.push(Span::styled(value.to_string(), style));
        x += (label.width() + value.width()) as u16;
        if *active {
            cursor = Some(x);
        }
    }
    spans.push(Span::styled(hint.to_string(), DIM));
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    if let Some(x) = cursor {
        f.set_cursor_position(Position::new(x.min(area.right().saturating_sub(1)), area.y));
    }
}
