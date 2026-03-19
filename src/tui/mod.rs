pub mod ui;

use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::types::SearchResult;

/// Application state for the interactive TUI.
pub struct App {
    pub results: Vec<SearchResult>,
    pub query: String,
    /// Absolute path to the repository root (for opening files in editor).
    pub repo_root: PathBuf,
    /// Index of the currently highlighted result.
    pub selected: usize,
    /// Vertical scroll offset of the preview pane.
    pub preview_scroll: u16,
    /// Set when the user wants to open a result in their editor.
    /// Contains (file_path, line). TUI exits after setting this.
    pub open_in_editor: Option<(String, usize)>,
    /// Whether the user has requested to quit.
    pub should_quit: bool,
}

impl App {
    pub fn new(query: String, results: Vec<SearchResult>, repo_root: PathBuf) -> Self {
        Self {
            results,
            query,
            repo_root,
            selected: 0,
            preview_scroll: 0,
            open_in_editor: None,
            should_quit: false,
        }
    }

    pub fn next(&mut self) {
        if !self.results.is_empty() {
            self.selected = (self.selected + 1).min(self.results.len() - 1);
            self.preview_scroll = 0;
        }
    }

    pub fn previous(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            self.preview_scroll = 0;
        }
    }

    pub fn scroll_preview_down(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_add(3);
    }

    pub fn scroll_preview_up(&mut self) {
        self.preview_scroll = self.preview_scroll.saturating_sub(3);
    }

    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected)
    }

    /// Queue the selected result for editor opening. Exits the TUI.
    pub fn open_selected(&mut self) {
        if let Some(r) = self.selected_result() {
            self.open_in_editor = Some((r.unit.file_path.clone(), r.unit.line_start));
            self.should_quit = true;
        }
    }

    /// Open selected result in editor without exiting the TUI.
    pub fn open_selected_stay(&mut self) {
        if let Some(r) = self.selected_result() {
            let file = r.unit.file_path.clone();
            let line = r.unit.line_start;
            let root = self.repo_root.clone();
            // Open non-blocking (best-effort; errors are silently ignored so TUI stays up).
            let _ = crate::editor::open(&file, line, &root);
        }
    }
}

/// Launch the interactive TUI. Returns when the user quits or opens a result.
pub fn run(query: String, results: Vec<SearchResult>, repo_root: PathBuf) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(query, results, repo_root.clone());

    loop {
        terminal.draw(|f| ui::render(f, &app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Char('j') | KeyCode::Down => app.next(),
                    KeyCode::Char('k') | KeyCode::Up => app.previous(),
                    KeyCode::Char('d') | KeyCode::PageDown => app.scroll_preview_down(),
                    KeyCode::Char('u') | KeyCode::PageUp => app.scroll_preview_up(),
                    // Enter — open in editor and exit TUI.
                    KeyCode::Enter => app.open_selected(),
                    // o — open in editor, stay in TUI (for GUI editors).
                    KeyCode::Char('o') => app.open_selected_stay(),
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // If Enter was pressed, open the file in the editor now that the terminal is restored.
    if let Some((file, line)) = app.open_in_editor {
        crate::editor::open(&file, line, &repo_root)?;
    }

    Ok(())
}
