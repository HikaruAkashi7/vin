use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Position, Rect};

use crate::buffer::Buffer;
use crate::editor::Editor;
use crate::tree::FileTree;

/// How often to refresh the file tree to pick up external changes.
const REFRESH_INTERVAL: Duration = Duration::from_millis(1000);

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Explorer,
    Editor,
}

/// App-wide commands, listed in the command palette.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Command {
    NewFile,
    CloseTab,
    NextTab,
    PrevTab,
    ToggleSidebar,
    FocusExplorer,
    ExplorerRename,
    Quit,
}

/// Text input for Explorer file operations.
pub enum InputKind {
    Rename(PathBuf),
}

pub struct Input {
    pub kind: InputKind,
    pub text: String,
}

impl Input {
    pub fn label(&self) -> &'static str {
        match self.kind {
            InputKind::Rename(_) => "Rename to: ",
        }
    }
}

/// Tab positions recorded at draw time, for click hit-testing.
pub struct TabHit {
    pub x: Range<u16>,
    pub close_x: u16,
    pub idx: usize,
}

pub struct App {
    pub tree: FileTree,
    pub tabs: Vec<Editor>,
    pub active: usize,
    pub focus: Focus,
    pub show_sidebar: bool,
    pub input: Option<Input>,
    pub message: Option<String>,
    pub should_quit: bool,
    quit_armed: bool,
    /// Tab the user tried to close with unsaved changes; closing it again discards them.
    close_armed: Option<usize>,
    clipboard: String,
    last_refresh: Instant,
    // Areas from the last draw, used for mouse hit-testing.
    pub sidebar_area: Rect,
    pub tree_area: Rect,
    pub tabs_area: Rect,
    pub tab_hits: Vec<TabHit>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

impl App {
    pub fn new(root: PathBuf) -> Self {
        Self {
            tree: FileTree::new(root),
            tabs: Vec::new(),
            active: 0,
            focus: Focus::Explorer,
            show_sidebar: true,
            input: None,
            message: None,
            should_quit: false,
            quit_armed: false,
            close_armed: None,
            clipboard: String::new(),
            last_refresh: Instant::now(),
            sidebar_area: Rect::default(),
            tree_area: Rect::default(),
            tabs_area: Rect::default(),
            tab_hits: Vec::new(),
        }
    }

    pub fn active_editor(&self) -> Option<&Editor> {
        self.tabs.get(self.active)
    }

    /// Opens path in a tab, or switches to its tab if already open.
    pub fn open(&mut self, path: PathBuf) {
        let path = std::path::absolute(&path).unwrap_or(path);
        if let Some(i) = self
            .tabs
            .iter()
            .position(|t| t.buf.path.as_deref().is_some_and(|p| same_file(p, &path)))
        {
            self.activate(i);
            return;
        }
        match Buffer::open(&path) {
            Ok(buf) => {
                self.tabs.push(Editor::new(buf));
                self.activate(self.tabs.len() - 1);
            }
            Err(e) => self.message = Some(format!("Cannot open {}: {e}", path.display())),
        }
    }

    fn activate(&mut self, idx: usize) {
        self.active = idx;
        self.focus = Focus::Editor;
        if let Some(path) = self.tabs.get(idx).and_then(|t| t.buf.path.clone()) {
            self.tree.reveal(&path, self.tree_area.height as usize);
        }
    }

    fn new_tab(&mut self) {
        self.tabs.push(Editor::new(Buffer::empty()));
        self.active = self.tabs.len() - 1;
        self.focus = Focus::Editor;
    }

    fn close_tab(&mut self, idx: usize) {
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        if tab.buf.dirty && self.close_armed != Some(idx) {
            self.close_armed = Some(idx);
            self.message = Some(format!(
                "{} has unsaved changes. Close again to discard them.",
                tab.buf.name()
            ));
            return;
        }
        self.close_armed = None;
        self.tabs.remove(idx);
        if self.active > idx || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        if self.tabs.is_empty() {
            if self.show_sidebar {
                self.focus = Focus::Explorer;
            }
        } else {
            self.activate(self.active);
        }
    }

    fn cycle_tab(&mut self, delta: isize) {
        let n = self.tabs.len() as isize;
        if n > 0 {
            self.activate((self.active as isize + delta).rem_euclid(n) as usize);
        }
    }

    fn toggle_sidebar(&mut self) {
        // Hiding it with no tabs open would leave nothing to interact with.
        self.show_sidebar = !self.show_sidebar || self.tabs.is_empty();
        self.focus = if self.show_sidebar {
            Focus::Explorer
        } else {
            Focus::Editor
        };
    }

    fn toggle_focus(&mut self) {
        if self.focus == Focus::Explorer && !self.tabs.is_empty() {
            self.focus = Focus::Editor;
        } else {
            self.show_sidebar = true;
            self.focus = Focus::Explorer;
        }
    }

    fn quit(&mut self) {
        let dirty = self.tabs.iter().filter(|t| t.buf.dirty).count();
        if dirty > 0 && !self.quit_armed {
            self.quit_armed = true;
            self.message = Some(format!(
                "{dirty} file(s) have unsaved changes. Press Ctrl+Q again to quit."
            ));
        } else {
            self.should_quit = true;
        }
    }

    pub fn run(&mut self, cmd: Command) {
        match cmd {
            Command::NewFile => self.new_tab(),
            Command::CloseTab => self.close_tab(self.active),
            Command::NextTab => self.cycle_tab(1),
            Command::PrevTab => self.cycle_tab(-1),
            Command::ToggleSidebar => self.toggle_sidebar(),
            Command::FocusExplorer => self.toggle_focus(),
            Command::ExplorerRename => {
                if let Some(path) = self.tree.selected_path().map(Path::to_path_buf) {
                    let text = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.input = Some(Input {
                        kind: InputKind::Rename(path),
                        text,
                    });
                }
            }
            Command::Quit => self.quit(),
        }
    }

    fn submit_input(&mut self) {
        let Some(input) = self.input.take() else {
            return;
        };
        let name = input.text.trim();
        if name.is_empty() {
            return;
        }
        let InputKind::Rename(from) = &input.kind;
        let to = from
            .parent()
            .map_or_else(|| PathBuf::from(name), |p| p.join(name));
        let result = if to.exists() {
            Err(std::io::Error::other(format!(
                "{} already exists",
                to.display()
            )))
        } else {
            fs::rename(from, &to).map(|_| {
                self.retarget_tabs(from, &to);
                self.tree.refresh();
                self.tree.reveal(&to, self.tree_area.height as usize);
            })
        };
        if let Err(e) = result {
            self.message = Some(format!("Failed: {e}"));
        }
    }

    /// Updates the paths of tabs showing the renamed file or anything under it.
    fn retarget_tabs(&mut self, from: &Path, to: &Path) {
        for tab in &mut self.tabs {
            let Some(path) = tab.buf.path.clone() else {
                continue;
            };
            if let Ok(rest) = path.strip_prefix(from) {
                let new = if rest.as_os_str().is_empty() {
                    to.to_path_buf()
                } else {
                    to.join(rest)
                };
                tab.set_path(new);
            }
        }
    }

    /// Called periodically: refreshes the tree to pick up external changes.
    pub fn tick(&mut self) {
        if self.last_refresh.elapsed() < REFRESH_INTERVAL {
            return;
        }
        self.last_refresh = Instant::now();
        self.tree.refresh();
    }

    pub fn handle_event(&mut self, ev: Event) {
        match ev {
            Event::Key(k) if k.kind != KeyEventKind::Release => self.handle_key(k),
            Event::Mouse(m) => self.handle_mouse(m),
            Event::Paste(s) => {
                if let Some(input) = &mut self.input {
                    input.text.extend(s.chars().filter(|c| !c.is_control()));
                } else if self.focus == Focus::Editor {
                    if let Some(ed) = self.tabs.get_mut(self.active) {
                        ed.handle_event(Event::Paste(s), &mut self.clipboard);
                    }
                }
            }
            _ => {}
        }
        if let Some(ed) = self.tabs.get_mut(self.active) {
            if std::mem::take(&mut ed.saved) {
                self.tree.refresh();
                if let Some(path) = ed.buf.path.clone() {
                    self.tree.reveal(&path, self.tree_area.height as usize);
                }
            }
        }
    }

    fn handle_key(&mut self, k: KeyEvent) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let key = match k.code {
            KeyCode::Char(c) => Some(c.to_ascii_lowercase()),
            _ => None,
        };
        if !(ctrl && key == Some('q')) {
            self.quit_armed = false;
        }
        if !(ctrl && key == Some('w')) {
            self.close_armed = None;
        }
        self.message = None;

        if self.input.is_some() {
            return self.handle_input_key(k, ctrl);
        }

        // Shortcuts that work regardless of focus.
        let global = match (key, k.code) {
            (Some('q'), _) if ctrl => Some(Command::Quit),
            (Some('w'), _) if ctrl => Some(Command::CloseTab),
            (Some('n'), _) if ctrl => Some(Command::NewFile),
            (Some('b'), _) if ctrl => Some(Command::ToggleSidebar),
            (Some('e'), _) if ctrl => Some(Command::FocusExplorer),
            (_, KeyCode::PageUp) if ctrl => Some(Command::PrevTab),
            (_, KeyCode::PageDown) if ctrl => Some(Command::NextTab),
            _ => None,
        };
        if let Some(cmd) = global {
            return self.run(cmd);
        }

        match self.focus {
            Focus::Explorer => match k.code {
                KeyCode::Esc if !self.tabs.is_empty() => self.focus = Focus::Editor,
                KeyCode::F(2) => self.run(Command::ExplorerRename),
                code => {
                    if let Some(path) = self.tree.handle_key(code, self.tree_area.height as usize) {
                        self.open(path);
                    }
                }
            },
            Focus::Editor => {
                if let Some(ed) = self.tabs.get_mut(self.active) {
                    ed.handle_event(Event::Key(k), &mut self.clipboard);
                }
            }
        }
    }

    fn handle_input_key(&mut self, k: KeyEvent, ctrl: bool) {
        let Some(input) = &mut self.input else { return };
        match k.code {
            KeyCode::Esc => self.input = None,
            KeyCode::Enter => self.submit_input(),
            KeyCode::Backspace => {
                input.text.pop();
            }
            KeyCode::Char(c) if !ctrl => input.text.push(c),
            _ => {}
        }
    }

    fn handle_mouse(&mut self, m: MouseEvent) {
        let pos = Position::new(m.column, m.row);

        let in_sidebar = self.show_sidebar && self.sidebar_area.contains(pos);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if in_sidebar {
                    self.focus = Focus::Explorer;
                    self.click_tree(pos);
                } else if self.tabs_area.contains(pos) {
                    self.click_tab(pos.x);
                } else if let Some(ed) = self.tabs.get_mut(self.active) {
                    if ed.view.contains(pos) {
                        self.focus = Focus::Editor;
                    }
                    ed.handle_mouse(m);
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if in_sidebar => {
                self.tree.scroll_by(if m.kind == MouseEventKind::ScrollUp {
                    -3
                } else {
                    3
                });
            }
            _ => {
                if let Some(ed) = self.tabs.get_mut(self.active) {
                    ed.handle_mouse(m);
                }
            }
        }
    }

    fn click_tree(&mut self, pos: Position) {
        if !self.tree_area.contains(pos) {
            return;
        }
        let idx = self.tree.scroll + (pos.y - self.tree_area.y) as usize;
        if let Some(path) = self.tree.activate(idx) {
            self.open(path);
        }
    }

    fn click_tab(&mut self, x: u16) {
        let Some(hit) = self.tab_hits.iter().find(|h| h.x.contains(&x)) else {
            return;
        };
        let (idx, close) = (hit.idx, hit.close_x == x);
        if close {
            self.close_tab(idx);
        } else {
            self.close_armed = None;
            self.activate(idx);
        }
    }
}
