mod app;
mod buffer;
mod editor;
mod tree;
mod ui;

use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::supports_keyboard_enhancement;
use ratatui::DefaultTerminal;

use app::{App, Focus};

const USAGE: &str = "usage: vin [FOLDER | FILE...]";
/// Idle tick, to pick up external changes and finished syntax loading.
const TICK: Duration = Duration::from_millis(250);

fn main() -> Result<()> {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    match args.first().and_then(|a| a.to_str()) {
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("vin {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        _ => {}
    }

    // A single folder argument becomes the workspace and the base for relative paths.
    let files = match args.as_slice() {
        [dir] if dir.is_dir() => {
            std::env::set_current_dir(dir)
                .with_context(|| format!("failed to open {}", dir.display()))?;
            Vec::new()
        }
        _ => args,
    };
    let mut app = App::new(std::env::current_dir()?);
    // When opening a file directly, show only the editor.
    if !files.is_empty() {
        app.show_sidebar = false;
        for f in files {
            app.open(f);
        }
        if app.tabs.is_empty() {
            app.show_sidebar = true;
            app.focus = Focus::Explorer;
        }
    }

    let mut terminal = ratatui::init();
    // Lets kitty-protocol terminals distinguish keys like Ctrl+Shift+Z.
    let enhanced = supports_keyboard_enhancement().unwrap_or(false);
    let _ = execute!(stdout(), EnableMouseCapture, EnableBracketedPaste);
    if enhanced {
        let _ = execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        );
    }

    let result = run(&mut terminal, &mut app);

    if enhanced {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
    let _ = execute!(stdout(), DisableBracketedPaste, DisableMouseCapture);
    ratatui::restore();
    result
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, app))?;
        if event::poll(TICK)? {
            app.handle_event(event::read()?);
            // Drain queued input (pastes, fast key repeat) before redrawing.
            while !app.should_quit && event::poll(Duration::ZERO)? {
                app.handle_event(event::read()?);
            }
        }
        app.tick();
    }
    Ok(())
}
