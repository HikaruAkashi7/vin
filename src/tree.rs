use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use ratatui::crossterm::event::KeyCode;

/// Hidden from the Explorer, matching VSCode's defaults.
const HIDDEN: &[&str] = &[".git", ".DS_Store"];

pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub depth: usize,
}

/// Sidebar file tree. Only expanded dirs are read; entries are flattened in display order.
pub struct FileTree {
    pub root: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub scroll: usize,
    expanded: HashSet<PathBuf>,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Self {
            root,
            entries: Vec::new(),
            selected: 0,
            scroll: 0,
            expanded: HashSet::new(),
        };
        tree.refresh();
        tree
    }

    pub fn root_name(&self) -> String {
        self.root
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.root.display().to_string())
    }

    /// Re-reads from disk, keeping the selected path when possible.
    pub fn refresh(&mut self) {
        let prev = self.entries.get(self.selected).map(|e| e.path.clone());
        let mut entries = Vec::new();
        self.collect(&self.root, 0, &mut entries);
        self.entries = entries;
        self.selected = prev
            .and_then(|p| self.entries.iter().position(|e| e.path == p))
            .unwrap_or(self.selected)
            .min(self.entries.len().saturating_sub(1));
    }

    fn collect(&self, dir: &Path, depth: usize, out: &mut Vec<Entry>) {
        let Ok(read) = fs::read_dir(dir) else { return };
        let mut items: Vec<Entry> = read
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if HIDDEN.contains(&name.as_str()) {
                    return None;
                }
                let path = e.path();
                Some(Entry {
                    is_dir: path.is_dir(),
                    path,
                    name,
                    depth,
                })
            })
            .collect();
        // Directories first, then case-insensitive name order.
        items.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        for e in items {
            let recurse = e.is_dir && self.expanded.contains(&e.path);
            let path = e.path.clone();
            out.push(e);
            if recurse {
                self.collect(&path, depth + 1, out);
            }
        }
    }

    pub fn is_expanded(&self, path: &Path) -> bool {
        self.expanded.contains(path)
    }

    /// Expands parents and selects path (VSCode's explorer.autoReveal).
    pub fn reveal(&mut self, path: &Path, height: usize) {
        let Ok(rel) = path.strip_prefix(&self.root) else {
            return;
        };
        let mut dir = self.root.clone();
        let parents: Vec<_> = rel
            .parent()
            .map(|p| p.components().collect())
            .unwrap_or_default();
        for c in parents {
            dir.push(c);
            self.expanded.insert(dir.clone());
        }
        self.refresh();
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.selected = i;
            self.scroll_into_view(height);
        }
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.entries.get(self.selected).map(|e| e.path.as_path())
    }

    /// Toggles a directory, or returns the path of a file.
    pub fn activate(&mut self, idx: usize) -> Option<PathBuf> {
        let e = self.entries.get(idx)?;
        self.selected = idx;
        if !e.is_dir {
            return Some(e.path.clone());
        }
        let path = e.path.clone();
        if !self.expanded.remove(&path) {
            self.expanded.insert(path);
        }
        self.refresh();
        None
    }

    /// Returns the path of a file to open, if any.
    pub fn handle_key(&mut self, code: KeyCode, height: usize) -> Option<PathBuf> {
        let last = self.entries.len().saturating_sub(1);
        let mut opened = None;
        match code {
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(last),
            KeyCode::PageUp => self.selected = self.selected.saturating_sub(height.max(1)),
            KeyCode::PageDown => self.selected = (self.selected + height.max(1)).min(last),
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Enter | KeyCode::Char(' ') => opened = self.activate(self.selected),
            KeyCode::Right => {
                if let Some(e) = self.entries.get(self.selected) {
                    if e.is_dir && !self.is_expanded(&e.path) {
                        self.activate(self.selected);
                    }
                }
            }
            KeyCode::Left => self.collapse_or_parent(),
            _ => {}
        }
        self.scroll_into_view(height);
        opened
    }

    fn collapse_or_parent(&mut self) {
        let Some(e) = self.entries.get(self.selected) else {
            return;
        };
        if e.is_dir && self.is_expanded(&e.path) {
            self.activate(self.selected);
        } else if e.depth > 0 {
            let depth = e.depth;
            if let Some(i) = self.entries[..self.selected]
                .iter()
                .rposition(|p| p.depth < depth)
            {
                self.selected = i;
            }
        }
    }

    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.entries.len().saturating_sub(1) as isize;
        self.scroll = (self.scroll as isize + delta).clamp(0, max.max(0)) as usize;
    }

    pub fn scroll_into_view(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
    }
}
