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
    #[allow(dead_code)]
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
    /// Editor command override from config.
    pub editor_cmd: Option<String>,
}

impl App {
    pub fn new(
        query: String,
        results: Vec<SearchResult>,
        repo_root: PathBuf,
        editor_cmd: Option<String>,
    ) -> Self {
        Self {
            results,
            query,
            repo_root,
            selected: 0,
            preview_scroll: 0,
            open_in_editor: None,
            should_quit: false,
            editor_cmd,
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
            let cmd = self.editor_cmd.as_deref();
            // Open non-blocking (best-effort; errors are silently ignored so TUI stays up).
            let _ = crate::editor::open_with(&file, line, &root, cmd);
        }
    }
}

/// Launch the interactive TUI. Returns when the user quits or opens a result.
pub fn run(
    query: String,
    results: Vec<SearchResult>,
    repo_root: PathBuf,
    editor_cmd: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(query, results, repo_root.clone(), editor_cmd);

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
        crate::editor::open_with(&file, line, &repo_root, app.editor_cmd.as_deref())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CodeUnit, Language, SearchResult, UnitType};
    use std::path::PathBuf;

    fn make_result(name: &str, file: &str, line: usize, body: &str) -> SearchResult {
        let mut unit = CodeUnit::new(
            file,
            Language::Rust,
            UnitType::Function,
            name,
            line,
            line + 5,
            body,
        );
        unit.id = 1;
        SearchResult { unit, score: 1.0, snippet: name.to_string(), repo_name: None }
    }

    fn make_app(n: usize) -> App {
        let results = (0..n)
            .map(|i| make_result(&format!("fn_{i}"), &format!("src/file{i}.rs"), i + 1, "fn body() {}"))
            .collect();
        App::new("query".into(), results, PathBuf::from("/tmp"), None)
    }

    // ── navigation ────────────────────────────────────────────────────────────

    #[test]
    fn next_advances_selection() {
        let mut app = make_app(3);
        assert_eq!(app.selected, 0);
        app.next();
        assert_eq!(app.selected, 1);
        app.next();
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn next_clamps_at_last() {
        let mut app = make_app(2);
        app.next();
        app.next(); // try to go past last
        app.next();
        assert_eq!(app.selected, 1); // clamped
    }

    #[test]
    fn previous_decrements_selection() {
        let mut app = make_app(3);
        app.selected = 2;
        app.previous();
        assert_eq!(app.selected, 1);
        app.previous();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn previous_clamps_at_zero() {
        let mut app = make_app(3);
        app.previous();
        app.previous();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn navigation_resets_preview_scroll() {
        let mut app = make_app(3);
        app.preview_scroll = 10;
        app.next();
        assert_eq!(app.preview_scroll, 0);

        app.preview_scroll = 7;
        app.previous();
        assert_eq!(app.preview_scroll, 0);
    }

    #[test]
    fn next_on_empty_results_does_not_panic() {
        let mut app = make_app(0);
        app.next();
        app.previous();
        assert_eq!(app.selected, 0);
    }

    // ── preview scroll ────────────────────────────────────────────────────────

    #[test]
    fn scroll_preview_down_increments() {
        let mut app = make_app(1);
        app.scroll_preview_down();
        assert_eq!(app.preview_scroll, 3);
        app.scroll_preview_down();
        assert_eq!(app.preview_scroll, 6);
    }

    #[test]
    fn scroll_preview_up_decrements_no_underflow() {
        let mut app = make_app(1);
        app.preview_scroll = 3;
        app.scroll_preview_up();
        assert_eq!(app.preview_scroll, 0);
        app.scroll_preview_up(); // saturating_sub, must not underflow
        assert_eq!(app.preview_scroll, 0);
    }

    // ── selected_result ───────────────────────────────────────────────────────

    #[test]
    fn selected_result_returns_current() {
        let app = make_app(3);
        assert_eq!(app.selected_result().unwrap().unit.name, "fn_0");
    }

    #[test]
    fn selected_result_returns_none_when_empty() {
        let app = make_app(0);
        assert!(app.selected_result().is_none());
    }

    // ── open_selected ─────────────────────────────────────────────────────────

    #[test]
    fn open_selected_sets_editor_target_and_quit_flag() {
        let mut app = make_app(3);
        app.selected = 1;
        app.open_selected();
        assert!(app.should_quit);
        let (file, line) = app.open_in_editor.unwrap();
        assert_eq!(file, "src/file1.rs");
        assert_eq!(line, 2); // line_start = i + 1 for i=1
    }

    #[test]
    fn open_selected_on_empty_does_not_panic() {
        let mut app = make_app(0);
        app.open_selected(); // must not panic
        assert!(!app.should_quit);
        assert!(app.open_in_editor.is_none());
    }

    // ── preview body vs signature ─────────────────────────────────────────────

    #[test]
    fn result_body_is_non_empty_for_preview() {
        // Verify that make_result sets a non-empty body (tests the preview fix assumption)
        let r = make_result("authenticate", "src/auth.rs", 10, "fn authenticate() { check() }");
        assert!(!r.unit.body.is_empty(), "body must be non-empty for preview to work");
    }
}
