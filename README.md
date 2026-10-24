# Vin

A lightweight, modeless TUI editor with VSCode-like keybindings, written in Rust.
No modes to learn — keys type text, and the shortcuts you already know just work.

The name is a wink: **Vi**m's terminal home, with **VS Code** living *in* it.

This is a minimal core: an Explorer sidebar, tabs, and the editing essentials.
Larger features (syntax highlighting, fuzzy file open, find/replace, undo …) are on
the roadmap and built up from here.

## Install

```sh
cargo install --path .
```

## Usage

```sh
vin            # open the current folder
vin ~/project  # open a folder
vin a.rs b.rs  # open files directly (sidebar hidden)
```

## Features

- Explorer sidebar and tabs, with a VSCode-inspired look
- Core editing: type, move, select (keyboard and mouse)
- Smart Enter indent, indentation detection, indent/outdent, tab stops
- Copy / cut / paste (whole line when nothing is selected)
- Save

## Keybindings

### Workbench

| Key | Action |
|---|---|
| `Ctrl+E` | Focus Explorer / Editor |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+N` | New file |
| `Ctrl+W` | Close tab (press twice to discard unsaved changes) |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next tab |
| `Ctrl+Q` | Quit (press twice to discard unsaved changes) |

### Explorer

| Key | Action |
|---|---|
| `↑` `↓` | Move |
| `Enter` | Open file / toggle folder |
| `←` `→` | Collapse / expand |
| `F2` | Rename |

### Editor

| Key | Action |
|---|---|
| `Ctrl+S` | Save (untitled buffers have no path and cannot be saved yet) |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / Cut / Paste (whole line when nothing is selected) |
| `Ctrl+A` | Select all |
| `Tab` / `Shift+Tab` | Indent / outdent |
| `Ctrl+←` / `Ctrl+→` | Move by word |
| `Ctrl+Backspace` / `Alt+Backspace` | Delete word |
| `Home` / `End` | Line start (smart) / end |
| Mouse | Click, drag to select, double-click word, triple-click line, wheel to scroll |

## Roadmap

- [ ] Syntax highlighting
- [ ] Quick Open with fuzzy scoring + Command Palette + Go to Line
- [ ] Undo / redo
- [ ] Incremental find and replace
- [ ] Auto-closing brackets/quotes, surround selection
- [ ] Line comments, move/copy/delete lines
- [ ] Automatic reload when an open file changes on disk
- [ ] System-clipboard copy via OSC 52
- [ ] Explorer: new file / new folder
- [ ] Multi-cursor, search across files, LSP, git status, word wrap

## Acknowledgements

Keybindings and the overall look are modeled on
[Visual Studio Code](https://github.com/microsoft/vscode)
(MIT License, Copyright (c) Microsoft Corporation).
