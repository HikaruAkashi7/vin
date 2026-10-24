use std::ops::Range;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ropey::Rope;
use unicode_width::UnicodeWidthChar;

use crate::buffer::Buffer;

pub const TAB_WIDTH: usize = 4;
const MULTI_CLICK: Duration = Duration::from_millis(400);

/// Tabs are fixed width; control chars render as a one-cell '?'.
pub fn char_width(c: char) -> usize {
    match c {
        '\t' => TAB_WIDTH,
        c if c.is_control() => 1,
        c => c.width().unwrap_or(0),
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Position after replacing `range` with `inserted` chars; positions inside the range snap to its start.
fn shift_pos(p: usize, range: &Range<usize>, inserted: usize) -> usize {
    if p >= range.end {
        p - range.len() + inserted
    } else if p > range.start {
        range.start
    } else {
        p
    }
}

/// Editor commands, invoked from keys.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Save,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Outdent,
}

/// Keybindings that terminals can deliver.
fn keymap(k: &KeyEvent) -> Option<Action> {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    Some(match k.code {
        KeyCode::Char(c) if ctrl => match c.to_ascii_lowercase() {
            's' => Action::Save,
            'a' => Action::SelectAll,
            'c' => Action::Copy,
            'x' => Action::Cut,
            'v' => Action::Paste,
            _ => return None,
        },
        KeyCode::BackTab => Action::Outdent,
        _ => return None,
    })
}

pub struct Editor {
    pub buf: Buffer,
    /// Char index, not byte offset.
    pub cursor: usize,
    /// Selection anchor; None means no selection.
    pub anchor: Option<usize>,
    /// Display column kept across vertical moves.
    preferred_col: Option<usize>,
    pub scroll_y: usize,
    pub scroll_x: usize,
    /// Text area from the last draw, for mouse mapping and scrolling.
    pub view: Rect,
    pub message: Option<String>,
    /// Set on successful save so App can refresh the file tree.
    pub saved: bool,
    dragging: bool,
    last_click: Option<(Instant, usize, u8)>,
}

impl Editor {
    pub fn new(buf: Buffer) -> Self {
        Self {
            buf,
            cursor: 0,
            anchor: None,
            preferred_col: None,
            scroll_y: 0,
            scroll_x: 0,
            view: Rect::default(),
            message: None,
            saved: false,
            dragging: false,
            last_click: None,
        }
    }

    pub fn set_path(&mut self, path: PathBuf) {
        self.buf.path = Some(path);
    }

    fn rope(&self) -> &Rope {
        &self.buf.rope
    }

    fn len(&self) -> usize {
        self.rope().len_chars()
    }

    fn char_at(&self, pos: usize) -> Option<char> {
        (pos < self.len()).then(|| self.rope().char(pos))
    }

    fn line_of(&self, pos: usize) -> usize {
        self.rope().char_to_line(pos)
    }

    pub fn cursor_line(&self) -> usize {
        self.line_of(self.cursor)
    }

    fn line_start(&self, line: usize) -> usize {
        self.rope().line_to_char(line)
    }

    fn line_end(&self, line: usize) -> usize {
        self.line_start(line) + self.buf.line_len(line)
    }

    fn line_text(&self, line: usize) -> String {
        self.rope()
            .slice(self.line_start(line)..self.line_end(line))
            .to_string()
    }

    fn indent_len(&self, line: usize) -> usize {
        self.rope()
            .line(line)
            .chars()
            .take(self.buf.line_len(line))
            .take_while(|c| *c == ' ' || *c == '\t')
            .count()
    }

    pub fn display_col(&self, pos: usize) -> usize {
        let start = self.line_start(self.line_of(pos));
        self.rope().slice(start..pos).chars().map(char_width).sum()
    }

    fn pos_at_col(&self, line: usize, col: usize) -> usize {
        let start = self.line_start(line);
        let len = self.buf.line_len(line);
        let mut w = 0;
        for (i, c) in self.rope().line(line).chars().take(len).enumerate() {
            let cw = char_width(c);
            if w + cw > col {
                // Clicking the right half of a char places the cursor after it.
                return start + i + usize::from(col - w >= cw.div_ceil(2) && cw > 0);
            }
            w += cw;
        }
        start + len
    }

    pub fn selection(&self) -> Option<Range<usize>> {
        let a = self.anchor?;
        (a != self.cursor).then(|| a.min(self.cursor)..a.max(self.cursor))
    }

    /// Lines covered by the selection (or the cursor line). A selection ending exactly at a line start excludes that line.
    fn selected_lines(&self) -> Range<usize> {
        match self.selection() {
            Some(r) => {
                let first = self.line_of(r.start);
                let mut last = self.line_of(r.end);
                if last > first && r.end == self.line_start(last) {
                    last -= 1;
                }
                first..last + 1
            }
            None => {
                let l = self.cursor_line();
                l..l + 1
            }
        }
    }

    /// clipboard comes from App because it is shared across tabs.
    pub fn handle_event(&mut self, ev: Event, clipboard: &mut String) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.handle_key(k, clipboard),
            Event::Mouse(m) => self.handle_mouse(m),
            Event::Paste(s) => {
                self.insert_text(&s.replace("\r\n", "\n").replace('\r', "\n"));
                self.scroll_into_view();
            }
            _ => {}
        }
    }

    fn handle_key(&mut self, k: KeyEvent, clipboard: &mut String) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let shift = k.modifiers.contains(KeyModifiers::SHIFT);
        let alt = k.modifiers.contains(KeyModifiers::ALT);

        self.message = None;

        if let Some(action) = keymap(&k) {
            return self.exec(action, clipboard);
        }

        match k.code {
            KeyCode::Char(_) if ctrl || alt => {}
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Enter => self.newline(),
            KeyCode::Tab => self.tab(),
            // Option+Delete (macOS) and Ctrl+Backspace delete by word.
            KeyCode::Backspace if ctrl || alt => self.delete_range(self.word_left()..self.cursor),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete if ctrl || alt => self.delete_range(self.cursor..self.word_right()),
            KeyCode::Delete => self.delete(),
            KeyCode::Esc => self.anchor = None,

            KeyCode::Left => {
                let target = match self.selection() {
                    Some(r) if !shift => r.start,
                    _ if ctrl || alt => self.word_left(),
                    _ => self.cursor.saturating_sub(1),
                };
                self.move_to(target, shift);
            }
            KeyCode::Right => {
                let target = match self.selection() {
                    Some(r) if !shift => r.end,
                    _ if ctrl || alt => self.word_right(),
                    _ => (self.cursor + 1).min(self.len()),
                };
                self.move_to(target, shift);
            }
            KeyCode::Up => self.vertical(-1, shift),
            KeyCode::Down => self.vertical(1, shift),
            KeyCode::PageUp => self.vertical(-(self.view.height.max(1) as isize), shift),
            KeyCode::PageDown => self.vertical(self.view.height.max(1) as isize, shift),
            KeyCode::Home if ctrl => self.move_to(0, shift),
            KeyCode::End if ctrl => self.move_to(self.len(), shift),
            KeyCode::Home => self.move_to(self.smart_home(), shift),
            KeyCode::End => self.move_to(self.line_end(self.cursor_line()), shift),
            _ => {}
        }
        self.scroll_into_view();
    }

    pub fn exec(&mut self, action: Action, clipboard: &mut String) {
        self.message = None;
        match action {
            Action::Save => self.save(),
            Action::Cut => self.cut(clipboard),
            Action::Copy => self.copy(clipboard),
            Action::Paste => self.insert_text(clipboard),
            Action::SelectAll => self.select_all(),
            Action::Outdent => self.outdent(),
        }
        self.scroll_into_view();
    }

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::ScrollDown => {
                self.scroll_y = (self.scroll_y + 3).min(self.buf.last_line());
            }
            MouseEventKind::ScrollUp => self.scroll_y = self.scroll_y.saturating_sub(3),
            MouseEventKind::Down(MouseButton::Left) => {
                let v = self.view;
                if m.row < v.y || m.row >= v.y + v.height {
                    return;
                }
                let pos = self.pos_at_screen(m.column, m.row);
                // Double-click selects a word, triple-click a line.
                let now = Instant::now();
                let count = match self.last_click {
                    Some((t, p, n)) if p == pos && now - t < MULTI_CLICK => n % 3 + 1,
                    _ => 1,
                };
                self.last_click = Some((now, pos, count));
                match count {
                    2 => self.select_word_at(pos),
                    3 => self.select_line_at(pos),
                    _ => self.move_to(pos, m.modifiers.contains(KeyModifiers::SHIFT)),
                }
                self.dragging = true;
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let pos = self.pos_at_screen(m.column, m.row);
                self.move_to(pos, true);
                self.scroll_into_view();
            }
            MouseEventKind::Up(MouseButton::Left) => self.dragging = false,
            _ => {}
        }
    }

    /// Positions outside the text area clamp to its edges.
    fn pos_at_screen(&self, x: u16, y: u16) -> usize {
        let v = self.view;
        let row = y.clamp(v.y, (v.y + v.height).saturating_sub(1)) - v.y;
        let line = (self.scroll_y + row as usize).min(self.buf.last_line());
        let col = x.saturating_sub(v.x) as usize + self.scroll_x;
        self.pos_at_col(line, col)
    }

    pub fn scroll_into_view(&mut self) {
        let (h, w) = (self.view.height as usize, self.view.width as usize);
        if h == 0 || w == 0 {
            return;
        }
        let line = self.cursor_line();
        if line < self.scroll_y {
            self.scroll_y = line;
        } else if line >= self.scroll_y + h {
            self.scroll_y = line + 1 - h;
        }
        let col = self.display_col(self.cursor);
        if col < self.scroll_x {
            self.scroll_x = col;
        } else if col >= self.scroll_x + w {
            self.scroll_x = col + 1 - w;
        }
    }

    fn move_to(&mut self, pos: usize, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = pos;
        self.preferred_col = None;
    }

    fn select(&mut self, range: Range<usize>) {
        self.anchor = Some(range.start);
        self.cursor = range.end;
        self.preferred_col = None;
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let line = self.cursor_line();
        let last = self.buf.last_line();
        let col = self
            .preferred_col
            .unwrap_or_else(|| self.display_col(self.cursor));
        let target = (line as isize + delta).clamp(0, last as isize) as usize;
        // Up on the first line goes to line start, down on the last line to line end.
        let pos = if delta < 0 && line == 0 {
            0
        } else if delta > 0 && line == last {
            self.len()
        } else {
            self.pos_at_col(target, col)
        };
        self.move_to(pos, extend);
        self.preferred_col = Some(col);
    }

    /// First non-whitespace char, or column 0 if already there.
    fn smart_home(&self) -> usize {
        let line = self.cursor_line();
        let start = self.line_start(line);
        let indent = self.indent_len(line);
        if self.cursor == start + indent {
            start
        } else {
            start + indent
        }
    }

    fn word_left(&self) -> usize {
        let r = self.rope();
        let mut i = self.cursor;
        while i > 0 && r.char(i - 1).is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let w = is_word(r.char(i - 1));
            while i > 0 && !r.char(i - 1).is_whitespace() && is_word(r.char(i - 1)) == w {
                i -= 1;
            }
        }
        i
    }

    fn word_right(&self) -> usize {
        let r = self.rope();
        let len = self.len();
        let mut i = self.cursor;
        while i < len && r.char(i).is_whitespace() {
            i += 1;
        }
        if i < len {
            let w = is_word(r.char(i));
            while i < len && !r.char(i).is_whitespace() && is_word(r.char(i)) == w {
                i += 1;
            }
        }
        i
    }

    fn select_word_at(&mut self, pos: usize) {
        let r = self.rope();
        let (mut start, mut end) = (pos, pos);
        while start > 0 && is_word(r.char(start - 1)) {
            start -= 1;
        }
        while end < self.len() && is_word(r.char(end)) {
            end += 1;
        }
        if start == end {
            end = (pos + 1).min(self.line_end(self.line_of(pos)));
        }
        self.select(start..end);
    }

    fn select_line_at(&mut self, pos: usize) {
        let line = self.line_of(pos);
        self.select(self.line_start(line)..self.rope().line_to_char(line + 1));
    }

    fn select_all(&mut self) {
        self.select(0..self.len());
    }

    /// Shifts cursor and anchor to track the edit.
    fn replace_range(&mut self, range: Range<usize>, text: &str) {
        let inserted = text.chars().count();
        self.cursor = shift_pos(self.cursor, &range, inserted);
        self.anchor = self.anchor.map(|a| shift_pos(a, &range, inserted));
        self.buf.rope.remove(range.clone());
        self.buf.rope.insert(range.start, text);
        self.preferred_col = None;
    }

    fn remove_selection(&mut self) {
        if let Some(r) = self.selection() {
            self.buf.rope.remove(r.clone());
            self.cursor = r.start;
        }
        self.anchor = None;
        self.preferred_col = None;
    }

    fn insert_raw(&mut self, text: &str) {
        self.buf.rope.insert(self.cursor, text);
        self.cursor += text.chars().count();
        self.anchor = None;
        self.preferred_col = None;
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() && self.selection().is_none() {
            return;
        }
        self.buf.dirty = true;
        self.remove_selection();
        self.insert_raw(text);
    }

    fn delete_range(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        self.buf.dirty = true;
        self.buf.rope.remove(range.clone());
        self.cursor = range.start;
        self.anchor = None;
        self.preferred_col = None;
    }

    fn insert_char(&mut self, c: char) {
        self.buf.dirty = true;
        self.remove_selection();
        self.buf.rope.insert_char(self.cursor, c);
        self.cursor += 1;
        self.preferred_col = None;
    }

    /// Keeps the current indent, one level deeper after an opening bracket.
    fn newline(&mut self) {
        self.buf.dirty = true;
        self.remove_selection();
        let line = self.cursor_line();
        let start = self.line_start(line);
        let indent: String = self
            .rope()
            .slice(start..self.cursor)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before: String = self.rope().slice(start..self.cursor).to_string();
        let last = before.trim_end().chars().last();
        let next = self.char_at(self.cursor);
        let unit = self.buf.indent.clone();

        let opens = last.is_some_and(|c| matches!(c, '(' | '[' | '{'));
        let closes = matches!(
            (last, next),
            (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}'))
        );
        if closes {
            // Enter inside {|} moves the closing brace down and puts the cursor on the line between.
            self.insert_raw(&format!("\n{indent}{unit}"));
            let pos = self.cursor;
            self.insert_raw(&format!("\n{indent}"));
            self.cursor = pos;
        } else if opens {
            self.insert_raw(&format!("\n{indent}{unit}"));
        } else {
            self.insert_raw(&format!("\n{indent}"));
        }
    }

    /// Indents a multi-line selection; otherwise inserts spaces up to the next tab stop.
    fn tab(&mut self) {
        if self.selected_lines().len() > 1 {
            return self.indent();
        }
        let text = if self.buf.indent == "\t" {
            "\t".to_string()
        } else {
            let w = self.buf.indent.len();
            let col = self.display_col(self.selection().map_or(self.cursor, |r| r.start));
            " ".repeat(w - col % w)
        };
        self.insert_text(&text);
    }

    fn backspace(&mut self) {
        if self.selection().is_some() {
            self.buf.dirty = true;
            self.remove_selection();
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let prev = self.rope().char(self.cursor - 1);
        // Within leading whitespace, delete back to the previous tab stop (editor.useTabStops).
        let line = self.cursor_line();
        let col = self.cursor - self.line_start(line);
        if self.buf.indent != "\t" && prev == ' ' && col <= self.indent_len(line) {
            let w = self.buf.indent.len();
            let n = (col - 1) % w + 1;
            return self.delete_range(self.cursor - n..self.cursor);
        }
        self.buf.dirty = true;
        self.buf.rope.remove(self.cursor - 1..self.cursor);
        self.cursor -= 1;
        self.preferred_col = None;
    }

    fn delete(&mut self) {
        if self.selection().is_some() {
            self.buf.dirty = true;
            self.remove_selection();
        } else if self.cursor < self.len() {
            self.buf.dirty = true;
            self.buf.rope.remove(self.cursor..self.cursor + 1);
            self.preferred_col = None;
        }
    }

    fn indent(&mut self) {
        self.buf.dirty = true;
        let unit = self.buf.indent.clone();
        for line in self.selected_lines().rev() {
            if self.buf.line_len(line) > 0 {
                let s = self.line_start(line);
                self.replace_range(s..s, &unit);
            }
        }
    }

    fn outdent(&mut self) {
        self.buf.dirty = true;
        let w = self.buf.indent.len().max(1);
        for line in self.selected_lines().rev() {
            let s = self.line_start(line);
            let text = self.line_text(line);
            let n = if text.starts_with('\t') {
                1
            } else {
                let spaces = text.len() - text.trim_start_matches(' ').len();
                if spaces.is_multiple_of(w) {
                    spaces.min(w)
                } else {
                    spaces % w
                }
            };
            if n > 0 {
                self.replace_range(s..s + n, "");
            }
        }
    }

    /// With no selection, use the whole current line, as VSCode does.
    fn copy_range(&self) -> Range<usize> {
        self.selection().unwrap_or_else(|| {
            let line = self.cursor_line();
            self.line_start(line)..self.rope().line_to_char(line + 1)
        })
    }

    fn copy(&mut self, clipboard: &mut String) {
        let mut text = self.rope().slice(self.copy_range()).to_string();
        if self.selection().is_none() && !text.ends_with('\n') {
            text.push('\n');
        }
        *clipboard = text;
    }

    fn cut(&mut self, clipboard: &mut String) {
        let range = self.copy_range();
        if range.is_empty() {
            return;
        }
        self.copy(clipboard);
        self.delete_range(range);
    }

    fn save(&mut self) {
        if self.buf.path.is_none() {
            self.message = Some("Cannot save: untitled file has no path".into());
            return;
        }
        self.message = Some(match self.buf.save() {
            Ok(()) => {
                self.saved = true;
                format!("Saved {}", self.buf.name())
            }
            Err(e) => format!("Save failed: {e}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str, path: &str) -> Editor {
        let mut buf = Buffer::empty();
        buf.rope = Rope::from_str(text);
        let mut ed = Editor::new(buf);
        ed.set_path(path.into());
        ed
    }

    fn text(ed: &Editor) -> String {
        ed.buf.rope.to_string()
    }

    #[test]
    fn enter_between_braces_indents() {
        let mut ed = editor("fn a() {}", "a.rs");
        ed.cursor = 8;
        ed.newline();
        assert_eq!(text(&ed), "fn a() {\n    \n}");
        assert_eq!(ed.cursor, 13);
    }

    #[test]
    fn indents_and_outdents_selection() {
        let mut ed = editor("a\nb\n", "a.txt");
        ed.select(0..3);
        ed.tab();
        assert_eq!(text(&ed), "    a\n    b\n");
        ed.outdent();
        assert_eq!(text(&ed), "a\nb\n");
    }

    #[test]
    fn backspace_deletes_tab_stop() {
        let mut ed = editor("        x", "a.rs");
        ed.cursor = 8;
        ed.backspace();
        assert_eq!(text(&ed), "    x");
    }
}
