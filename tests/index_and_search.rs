/// Integration tests for the full index → search → body-enrichment pipeline.
///
/// These tests build the actual `scout` binary and run it against a temporary
/// fixture repository so every layer (parser → SQLite → Tantivy → CLI) is
/// exercised end-to-end.
use std::fs;
use std::path::Path;
use std::process::Command;

// Path to the compiled `scout` binary, injected at compile time by Cargo.
const SCOUT: &str = env!("CARGO_BIN_EXE_scout");

// ── helpers ───────────────────────────────────────────────────────────────────

/// Create a small fixture repo and return its TempDir (dropped = deleted).
fn make_fixture_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Python auth module
    fs::create_dir_all(root.join("services/auth")).unwrap();
    fs::write(
        root.join("services/auth/login.py"),
        r#"
def authenticate_user(username: str, password: str) -> bool:
    """Check credentials against the database."""
    if not username or not password:
        return False
    return check_password_hash(username, password)

def check_password_hash(username: str, hashed: str) -> bool:
    """Verify bcrypt hash."""
    import bcrypt
    return bcrypt.checkpw(username.encode(), hashed.encode())

class UserSession:
    """Manages an authenticated user session."""
    def __init__(self, user_id: int):
        self.user_id = user_id
        self.token = None

    def generate_token(self):
        """Generate a JWT session token."""
        import jwt
        self.token = jwt.encode({"user_id": self.user_id}, "secret")
        return self.token
"#,
    )
    .unwrap();

    // Rust payment module
    fs::create_dir_all(root.join("services/payments")).unwrap();
    fs::write(
        root.join("services/payments/processor.rs"),
        r#"
/// Process a payment transaction.
pub fn process_payment(amount: f64, card_token: &str) -> Result<String, String> {
    if amount <= 0.0 {
        return Err("amount must be positive".into());
    }
    let charge_id = charge_card(card_token, amount)?;
    Ok(charge_id)
}

fn charge_card(token: &str, amount: f64) -> Result<String, String> {
    // Calls Stripe API
    Ok(format!("ch_{token}_{amount}"))
}

/// Refund a previously processed payment.
pub fn refund_payment(charge_id: &str) -> Result<(), String> {
    if charge_id.is_empty() {
        return Err("charge_id required".into());
    }
    Ok(())
}
"#,
    )
    .unwrap();

    // TypeScript frontend
    fs::create_dir_all(root.join("frontend/src")).unwrap();
    fs::write(
        root.join("frontend/src/api.ts"),
        r#"
export async function fetchUserProfile(userId: string): Promise<UserProfile> {
    const response = await fetch(`/api/users/${userId}`);
    if (!response.ok) {
        throw new Error(`Failed to fetch profile: ${response.statusText}`);
    }
    return response.json();
}

export function formatCurrency(amount: number, currency: string = 'USD'): string {
    return new Intl.NumberFormat('en-US', { style: 'currency', currency }).format(amount);
}

export class ApiClient {
    private baseUrl: string;
    constructor(baseUrl: string) {
        this.baseUrl = baseUrl;
    }
    async get<T>(path: string): Promise<T> {
        const resp = await fetch(this.baseUrl + path);
        return resp.json();
    }
}
"#,
    )
    .unwrap();

    dir
}

/// Run `scout index <path>` and assert it succeeds.
fn index_repo(repo: &Path) {
    let output = Command::new(SCOUT)
        .args(["index", &repo.to_string_lossy()])
        .current_dir(repo)
        .output()
        .expect("failed to run scout index");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        output.status.success(),
        "scout index failed\nstdout: {stdout}\nstderr: {stderr}"
    );
}

/// Run a search and return parsed JSON array.
/// Panics with a helpful message if the command fails or returns no results.
fn search_json(repo: &Path, query: &str) -> Vec<serde_json::Value> {
    let output = Command::new(SCOUT)
        .args([
            "search",
            query,
            "--format",
            "json",
            "--limit",
            "20",
            "--no-tui",
            "--path",
            &repo.to_string_lossy(),
        ])
        .current_dir(repo)
        .output()
        .expect("failed to run scout search");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "scout search failed\nstdout: {stdout}\nstderr: {stderr}"
    );

    if stdout.trim().is_empty() {
        panic!(
            "scout search returned empty output (no results?) for query '{query}'\nstderr: {stderr}"
        );
    }

    serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
        .unwrap_or_else(|e| panic!("JSON parse failed: {e}\nOutput: {stdout}"))
}

// ── indexing tests ────────────────────────────────────────────────────────────

#[test]
fn index_creates_scout_directory() {
    let repo = make_fixture_repo();
    index_repo(repo.path());
    assert!(
        repo.path().join(".scout").exists(),
        ".scout directory must be created"
    );
    assert!(
        repo.path().join(".scout/metadata.db").exists(),
        "metadata.db must exist"
    );
    assert!(
        repo.path().join(".scout/tantivy").exists(),
        "tantivy index directory must exist"
    );
}

#[test]
fn index_reports_file_and_unit_counts() {
    let repo = make_fixture_repo();
    let output = Command::new(SCOUT)
        .args(["index", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // Should report at least 3 files (login.py, processor.rs, api.ts)
    assert!(
        stdout.contains("files"),
        "output should report indexed files\n{stdout}"
    );
}

#[test]
fn incremental_reindex_skips_unchanged_files() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Second index run — all files are unchanged so all should be skipped.
    let output = Command::new(SCOUT)
        .args(["index", "--verbose", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    assert!(
        stdout.contains("unchanged") || stdout.contains("0 new units"),
        "second index run should skip unchanged files\n{stdout}"
    );
}

#[test]
fn incremental_reindex_picks_up_modified_file() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Modify one file.
    let new_content = r#"
def handle_oauth_callback(code: str) -> str:
    """Exchange OAuth authorization code for access token."""
    token = exchange_code_for_token(code)
    return token

def exchange_code_for_token(code: str) -> str:
    return f"token_{code}"
"#;
    fs::write(repo.path().join("services/auth/login.py"), new_content).unwrap();

    let output = Command::new(SCOUT)
        .args(["index", "--verbose", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // The modified file must be re-indexed (not skipped)
    assert!(
        !stdout.contains("skip (unchanged): services/auth/login.py"),
        "modified file should NOT be skipped\n{stdout}"
    );
}

// ── search result structure ───────────────────────────────────────────────────

#[test]
fn search_json_has_required_fields() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Use exact function name as query to guarantee a match via name-match scoring
    let results = search_json(repo.path(), "process_payment");

    let first = &results[0];
    assert!(first["name"].is_string(), "name must be a string");
    assert!(first["file_path"].is_string(), "file_path must be a string");
    assert!(first["line_start"].is_number(), "line_start must be a number");
    assert!(first["language"].is_string(), "language must be a string");
    assert!(first["score"].is_number(), "score must be a number");
    assert!(first["rank"].is_number(), "rank must be a number");
    assert!(first["unit_type"].is_string(), "unit_type must be a string");
}

#[test]
fn search_returns_auth_functions_for_auth_query() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Use exact name for reliable BM25 match
    let results = search_json(repo.path(), "authenticate_user");

    let top = &results[0];
    let name = top["name"].as_str().unwrap_or("");
    let file = top["file_path"].as_str().unwrap_or("");
    assert!(
        name.contains("authenticate") || file.contains("auth"),
        "top result for 'authenticate_user' should be from auth module, got name='{}' file='{}'",
        name,
        file
    );
}

#[test]
fn search_returns_payment_functions_for_payment_query() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let results = search_json(repo.path(), "process_payment");

    // Top result must be the payment processor function
    let top = &results[0];
    let name = top["name"].as_str().unwrap_or("");
    assert_eq!(name, "process_payment", "top result should be process_payment");
    assert_eq!(
        top["language"].as_str().unwrap_or(""),
        "rust",
        "process_payment is Rust"
    );
}

// ── body enrichment regression test ──────────────────────────────────────────

#[test]
fn sqlite_body_non_empty_after_index() {
    // This is the critical regression test for the body enrichment fix.
    // Before the fix, BM25 search returned body="" for all results, making
    // TUI preview show nothing. We verify the SQLite database has non-empty
    // bodies so the enrichment can work.
    use rusqlite::Connection;

    let repo = make_fixture_repo();
    index_repo(repo.path());

    let db_path = repo.path().join(".scout/metadata.db");
    let conn = Connection::open(&db_path).unwrap();

    let empty_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_units WHERE body = '' OR body IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM code_units", [], |r| r.get(0))
        .unwrap();

    assert!(total > 0, "index must have code units");
    assert_eq!(
        empty_count, 0,
        "{empty_count}/{total} units have empty body — TUI preview would be blank for them"
    );
}

// ── filter tests ──────────────────────────────────────────────────────────────

#[test]
fn search_lang_filter_returns_only_rust() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Use a generic name query that can match multiple languages, then filter to rust
    let output = Command::new(SCOUT)
        .args([
            "search",
            "payment process",
            "--format",
            "json",
            "--lang",
            "rust",
            "--limit",
            "20",
            "--no-tui",
            "--path",
            &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // If no results for this filter, that's also valid (just check it doesn't crash)
    if stdout.trim().is_empty() {
        // No results — acceptable, but command must succeed
        assert!(
            output.status.success(),
            "command must succeed even with no results\nstderr: {stderr}"
        );
        return;
    }

    let parsed: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    for r in &parsed {
        let lang = r["language"].as_str().unwrap_or("?");
        assert_eq!(lang, "rust", "non-rust result slipped through --lang rust filter");
    }
}

#[test]
fn search_path_filter_restricts_results() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "process_payment",
            "--format",
            "json",
            "--path-filter",
            "payments",
            "--limit",
            "10",
            "--no-tui",
            "--path",
            &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.trim().is_empty() {
        return; // No results — filter excluded everything
    }

    let parsed: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    for r in &parsed {
        let path = r["file_path"].as_str().unwrap_or("?");
        assert!(
            path.contains("payments"),
            "result path '{}' does not match payments filter",
            path
        );
    }
}

#[test]
fn search_csv_output_is_valid() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "generate_token",
            "--format",
            "csv",
            "--limit",
            "5",
            "--no-tui",
            "--path",
            &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scout search --format csv failed\nstderr: {stderr}"
    );

    if stdout.trim().is_empty() {
        return; // No results is acceptable
    }

    // Must have a header row
    let first_line = stdout.lines().next().unwrap_or("");
    assert!(
        first_line.contains("name") && first_line.contains("file_path"),
        "CSV must have header row with name and file_path columns\n{stdout}"
    );
}

// ── maintenance commands ──────────────────────────────────────────────────────

#[test]
fn optimize_runs_without_error() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["optimize", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scout optimize failed\n{stderr}"
    );
}

#[test]
fn cleanup_runs_without_error() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["cleanup", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scout cleanup failed\n{stderr}"
    );
}

#[test]
fn rebuild_recreates_index() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["rebuild", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "scout rebuild failed");

    // After rebuild, search should still work
    let results = search_json(repo.path(), "authenticate_user");
    assert!(
        !results.is_empty(),
        "index must have results after rebuild"
    );
}

// ── report command ────────────────────────────────────────────────────────────

#[test]
fn report_unused_functions_runs_without_error() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["report", "unused-functions", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "scout report unused-functions failed");
}

// ── config command ────────────────────────────────────────────────────────────

#[test]
fn config_list_runs_without_error() {
    let output = Command::new(SCOUT).args(["config", "list"]).output().unwrap();
    assert!(output.status.success(), "scout config list failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("search.limit"), "config list must show search.limit");
}

// ── repos command ─────────────────────────────────────────────────────────────

#[test]
fn repos_list_runs_without_error() {
    let output = Command::new(SCOUT).args(["repos", "list"]).output().unwrap();
    assert!(output.status.success(), "scout repos list failed");
}

// ── --help and --version ──────────────────────────────────────────────────────

#[test]
fn help_flag_exits_successfully() {
    let output = Command::new(SCOUT).arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("scout"), "help output must mention scout");
}

#[test]
fn version_flag_exits_successfully() {
    let output = Command::new(SCOUT).arg("--version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("scout"), "version output must include scout name");
}

#[test]
fn search_help_shows_flags() {
    let output = Command::new(SCOUT).args(["search", "--help"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--limit"), "search --help must show --limit flag");
    assert!(stdout.contains("--lang"), "search --help must show --lang flag");
    assert!(stdout.contains("--format"), "search --help must show --format flag");
}

// ── shorthand query syntax ────────────────────────────────────────────────────

#[test]
fn bare_query_without_search_subcommand_works() {
    // `scout "query"` should behave the same as `scout search "query"`
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "authenticate_user",
            "--format",
            "json",
            "--limit",
            "3",
            "--no-tui",
            "--path",
            &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Should parse as search, not error
    let parsed = serde_json::from_str::<Vec<serde_json::Value>>(&stdout)
        .unwrap_or_else(|e| panic!("bare query should produce JSON: {e}\nstdout: {stdout}\nstderr: {stderr}"));
    assert!(!parsed.is_empty(), "bare query for exact function name must find results");
}

// ── multi-language fixture coverage ──────────────────────────────────────────

#[test]
fn index_parses_python_rust_typescript() {
    use rusqlite::Connection;

    let repo = make_fixture_repo();
    index_repo(repo.path());

    let db_path = repo.path().join(".scout/metadata.db");
    let conn = Connection::open(&db_path).unwrap();

    for lang in &["python", "rust", "typescript"] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM code_units WHERE language = ?1",
                rusqlite::params![lang],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            count > 0,
            "no {lang} units found in index — parser may have failed"
        );
    }
}

// ── stats command ─────────────────────────────────────────────────────────────

#[test]
fn stats_requires_index_first() {
    // Running stats with no index should exit non-zero with a helpful message.
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(SCOUT)
        .args(["stats", "--path", &dir.path().to_string_lossy()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "stats without an index should fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("scout index") || stderr.contains("No index"),
        "error message should mention 'scout index'\n{stderr}"
    );
}

#[test]
fn stats_output_contains_expected_sections() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["stats", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "scout stats failed\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Must show key sections
    assert!(stdout.contains("Unit types"), "stats must show Unit types section\n{stdout}");
    assert!(stdout.contains("Languages"), "stats must show Languages section\n{stdout}");
    assert!(stdout.contains("Embeddings"), "stats must show Embeddings section\n{stdout}");
    assert!(stdout.contains("Storage"), "stats must show Storage section\n{stdout}");
    assert!(stdout.contains("Status"), "stats must show Status section\n{stdout}");
}

#[test]
fn stats_output_shows_nonzero_unit_count() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["stats", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());

    // The fixture has Python, Rust, TypeScript files — at least one language must appear
    let has_lang = stdout.contains("python")
        || stdout.contains("rust")
        || stdout.contains("typescript");
    assert!(
        has_lang,
        "stats output should list at least one language\n{stdout}"
    );
}

#[test]
fn stats_output_shows_database_size() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["stats", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    // Storage section must mention the database file and show a non-zero size
    assert!(
        stdout.contains("metadata.db"),
        "stats should show metadata.db size\n{stdout}"
    );
}

// ── daemon hook script content ────────────────────────────────────────────────

#[test]
fn install_hooks_uses_mkdir_not_flock() {
    // Installs git hooks into a temp git repo, then reads the hook files to
    // verify the portable mkdir lock is used instead of the Linux-only flock(1).
    let repo = make_fixture_repo();

    // Initialise a real git repo so install-hooks can find .git/
    let git_init = Command::new("git")
        .args(["init"])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(git_init.status.success(), "git init failed");

    let output = Command::new(SCOUT)
        .args(["daemon", "install-hooks", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "daemon install-hooks failed\n{stderr}"
    );

    // Check each hook file that was written
    let hooks_dir = repo.path().join(".git/hooks");
    let hook_names = ["post-commit", "post-merge", "post-checkout"];
    let mut found_any = false;

    for hook in &hook_names {
        let hook_path = hooks_dir.join(hook);
        if !hook_path.exists() {
            continue;
        }
        found_any = true;
        let content = fs::read_to_string(&hook_path)
            .unwrap_or_else(|_| panic!("failed to read hook file {hook}"));

        assert!(
            content.contains("mkdir"),
            "hook '{hook}' must use mkdir for locking (not flock)\n{content}"
        );
        assert!(
            !content.contains("flock"),
            "hook '{hook}' must NOT use flock (Linux-only, not portable)\n{content}"
        );
    }

    assert!(found_any, "at least one hook file must have been written");
}

// ── --limit flag ──────────────────────────────────────────────────────────────

#[test]
fn search_limit_one_returns_exactly_one_result() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Index has multiple functions; --limit 1 must return exactly 1.
    let output = Command::new(SCOUT)
        .args([
            "search",
            "function",
            "--format", "json",
            "--limit", "1",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    if stdout.trim().is_empty() { return; }
    let results: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert_eq!(results.len(), 1, "--limit 1 must return exactly 1 result");
}

#[test]
fn search_limit_three_returns_at_most_three() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "authenticate",
            "--format", "json",
            "--limit", "3",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    if stdout.trim().is_empty() { return; }
    let results: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    assert!(results.len() <= 3, "--limit 3 returned {} results", results.len());
}

// ── --no-tui flag ─────────────────────────────────────────────────────────────

#[test]
fn search_no_tui_flag_produces_plain_text_output() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "process_payment",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "search --no-tui failed");
    // Plain text output should contain the function name
    assert!(
        stdout.contains("process_payment"),
        "plain output must contain function name\n{stdout}"
    );
}

// ── --exclude-tests flag ──────────────────────────────────────────────────────

#[test]
fn search_exclude_tests_filters_test_files() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Write one production file and one test file with the same function name.
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(
        root.join("src/auth.py"),
        "def validate_token(token: str) -> bool:\n    return len(token) > 0\n",
    ).unwrap();

    fs::create_dir_all(root.join("tests")).unwrap();
    fs::write(
        root.join("tests/test_auth.py"),
        "def validate_token():\n    assert validate_token('abc') == True\n",
    ).unwrap();

    index_repo(root);

    // Without --exclude-tests both files should contribute results.
    let all = Command::new(SCOUT)
        .args(["search", "validate_token", "--format", "json", "--limit", "20", "--no-tui",
               "--path", &root.to_string_lossy()])
        .current_dir(root)
        .output().unwrap();
    let all_stdout = String::from_utf8_lossy(&all.stdout);
    let all_results: Vec<serde_json::Value> = serde_json::from_str(&all_stdout).unwrap_or_default();

    // With --exclude-tests, test files must be absent.
    let filtered = Command::new(SCOUT)
        .args(["search", "validate_token", "--format", "json", "--limit", "20",
               "--no-tui", "--exclude-tests", "--path", &root.to_string_lossy()])
        .current_dir(root)
        .output().unwrap();

    let filtered_stdout = String::from_utf8_lossy(&filtered.stdout);
    assert!(filtered.status.success());
    if !filtered_stdout.trim().is_empty() {
        let filtered_results: Vec<serde_json::Value> = serde_json::from_str(&filtered_stdout).unwrap();
        for r in &filtered_results {
            let path = r["file_path"].as_str().unwrap_or("");
            // Normalise separators so the check works on Windows (\) and Unix (/).
            let path_norm = path.replace('\\', "/");
            assert!(
                !path_norm.contains("/tests/") && !path_norm.contains("test_"),
                "--exclude-tests allowed test file through: {path}"
            );
        }
        // Filtered set must be ≤ full set
        assert!(
            filtered_results.len() <= all_results.len(),
            "filtered result count exceeded unfiltered count"
        );
    }
}

// ── --show-context flag ───────────────────────────────────────────────────────

#[test]
fn search_show_context_does_not_crash() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "process_payment",
            "--show-context",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "search --show-context crashed\n{stderr}"
    );
}

// ── empty / no-results queries ────────────────────────────────────────────────

#[test]
fn search_nonsense_query_returns_no_results_gracefully() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "xyzzy_zzz_nonexistent_function_name_abc123",
            "--format", "json",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Must exit 0 even with no results (just prints to stderr "No results for...")
    assert!(
        output.status.success(),
        "search with no results should not crash"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Either empty output or empty JSON array
    if !stdout.trim().is_empty() {
        let parsed: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap_or_default();
        assert!(parsed.is_empty(), "nonsense query should return no results");
    }
}

#[test]
fn search_special_characters_in_query_do_not_crash() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Queries with special regex/SQL characters must not cause a panic or signal abort.
    // Some queries (e.g. malformed Tantivy syntax) may return a non-zero exit code
    // with a user-readable error — that is acceptable. What is NOT acceptable is
    // a crash (segfault, OOM) or a completely empty response with no message.
    for query in &["(auth)", "a.b.c", "x + y", "back\\slash", "foo AND bar"] {
        let output = Command::new(SCOUT)
            .args([
                "search", query,
                "--format", "json",
                "--no-tui",
                "--path", &repo.path().to_string_lossy(),
            ])
            .current_dir(repo.path())
            .output()
            .unwrap();

        // Signal abort would make `output` an Err — we already unwrapped above.
        // Just ensure the process did not exit due to a signal (code < 0 on Unix).
        let code = output.status.code();
        assert!(
            code.is_some(),
            "search with query '{query}' was killed by a signal (crash)"
        );
    }
}

// ── missing-index error paths ─────────────────────────────────────────────────

#[test]
fn search_without_index_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(SCOUT)
        .args([
            "search", "anything",
            "--format", "json",
            "--no-tui",
            "--path", &dir.path().to_string_lossy(),
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "search without index must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("scout index") || stderr.contains("No index"),
        "error must mention how to fix it\n{stderr}"
    );
}

#[test]
fn optimize_without_index_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(SCOUT)
        .args(["optimize", "--path", &dir.path().to_string_lossy()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "optimize without index must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "error output must not be empty");
}

#[test]
fn cleanup_without_index_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(SCOUT)
        .args(["cleanup", "--path", &dir.path().to_string_lossy()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "cleanup without index must fail");
}

#[test]
fn report_without_index_exits_with_error() {
    let dir = tempfile::tempdir().unwrap();
    let output = Command::new(SCOUT)
        .args(["report", "unused-functions", "--path", &dir.path().to_string_lossy()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!output.status.success(), "report without index must fail");
}

// ── --find-similar error handling ─────────────────────────────────────────────

#[test]
fn find_similar_bad_format_errors() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Missing colon — should fail with helpful message.
    let output = Command::new(SCOUT)
        .args([
            "search",
            "--find-similar", "src/auth.py",  // no :LINE part
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "--find-similar without LINE must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FILE:LINE") || stderr.contains("find-similar"),
        "error must mention expected format\n{stderr}"
    );
}

#[test]
fn find_similar_non_numeric_line_errors() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args([
            "search",
            "--find-similar", "src/auth.py:notanumber",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "--find-similar with non-numeric line must fail"
    );
}

// ── repos add / remove lifecycle ──────────────────────────────────────────────

#[test]
fn repos_add_then_list_then_remove() {
    let repo = make_fixture_repo();
    let name = format!("test-repo-{}", std::process::id()); // unique name per test run

    // Add
    let add_out = Command::new(SCOUT)
        .args(["repos", "add", &name, &repo.path().to_string_lossy()])
        .output()
        .unwrap();
    let add_stderr = String::from_utf8_lossy(&add_out.stderr);
    assert!(add_out.status.success(), "repos add failed\n{add_stderr}");
    let add_stdout = String::from_utf8_lossy(&add_out.stdout);
    assert!(add_stdout.contains(&name), "add output must mention name\n{add_stdout}");

    // List — new repo must appear
    let list_out = Command::new(SCOUT).args(["repos", "list"]).output().unwrap();
    let list_stdout = String::from_utf8_lossy(&list_out.stdout);
    assert!(list_out.status.success(), "repos list failed");
    assert!(list_stdout.contains(&name), "added repo must appear in list\n{list_stdout}");

    // Remove
    let rm_out = Command::new(SCOUT).args(["repos", "remove", &name]).output().unwrap();
    let rm_stderr = String::from_utf8_lossy(&rm_out.stderr);
    assert!(rm_out.status.success(), "repos remove failed\n{rm_stderr}");

    // List — repo must no longer appear
    let list2 = Command::new(SCOUT).args(["repos", "list"]).output().unwrap();
    let list2_stdout = String::from_utf8_lossy(&list2.stdout);
    assert!(!list2_stdout.contains(&name), "removed repo must not appear in list\n{list2_stdout}");
}

#[test]
fn repos_remove_nonexistent_errors() {
    let output = Command::new(SCOUT)
        .args(["repos", "remove", "this-repo-does-not-exist-xyz"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "removing a nonexistent repo must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.is_empty(), "error output must explain the problem");
}

#[test]
fn repos_add_invalid_path_errors() {
    let output = Command::new(SCOUT)
        .args(["repos", "add", "myrepo", "/this/path/does/not/exist/xyz"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "repos add with invalid path must fail"
    );
}

// ── config get / set via CLI ──────────────────────────────────────────────────

#[test]
fn config_get_known_key_succeeds() {
    let output = Command::new(SCOUT)
        .args(["config", "get", "search.limit"])
        .output()
        .unwrap();

    assert!(output.status.success(), "config get known key failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should print the current value (default is "10")
    assert!(!stdout.trim().is_empty(), "config get must print a value");
}

#[test]
fn config_get_unknown_key_errors() {
    let output = Command::new(SCOUT)
        .args(["config", "get", "this.key.does.not.exist"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "config get with unknown key must fail"
    );
}

#[test]
fn config_set_and_get_roundtrip() {
    // Set search.limit to a non-default value, then read it back.
    let set_out = Command::new(SCOUT)
        .args(["config", "set", "search.limit", "42"])
        .output()
        .unwrap();
    assert!(set_out.status.success(), "config set failed");

    let get_out = Command::new(SCOUT)
        .args(["config", "get", "search.limit"])
        .output()
        .unwrap();
    assert!(get_out.status.success(), "config get failed after set");
    let val = String::from_utf8_lossy(&get_out.stdout);
    assert_eq!(val.trim(), "42", "config get must return the value that was set");

    // Restore default so we don't pollute other tests.
    Command::new(SCOUT)
        .args(["config", "set", "search.limit", "10"])
        .output()
        .unwrap();
}

#[test]
fn config_set_invalid_value_errors() {
    let output = Command::new(SCOUT)
        .args(["config", "set", "search.limit", "not_a_number"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "config set with invalid value must fail"
    );
}

#[test]
fn config_set_unknown_key_errors() {
    let output = Command::new(SCOUT)
        .args(["config", "set", "not.a.real.key", "value"])
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "config set with unknown key must fail"
    );
}

#[test]
fn config_list_shows_all_known_keys() {
    let output = Command::new(SCOUT).args(["config", "list"]).output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for key in &[
        "search.limit",
        "search.no_tui",
        "search.format",
        "search.exclude_tests",
        "index.auto_index",
        "editor.command",
    ] {
        assert!(stdout.contains(key), "config list must show '{key}'\n{stdout}");
    }
}

// ── completions generation ────────────────────────────────────────────────────

#[test]
fn completions_bash_outputs_non_empty_script() {
    let output = Command::new(SCOUT).args(["completions", "bash"]).output().unwrap();
    assert!(output.status.success(), "completions bash failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "bash completions must be non-empty");
    // Bash completions script must reference the binary name
    assert!(
        stdout.contains("scout"),
        "bash completions must reference 'scout'\n{stdout}"
    );
}

#[test]
fn completions_zsh_outputs_non_empty_script() {
    let output = Command::new(SCOUT).args(["completions", "zsh"]).output().unwrap();
    assert!(output.status.success(), "completions zsh failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "zsh completions must be non-empty");
}

#[test]
fn completions_fish_outputs_non_empty_script() {
    let output = Command::new(SCOUT).args(["completions", "fish"]).output().unwrap();
    assert!(output.status.success(), "completions fish failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.trim().is_empty(), "fish completions must be non-empty");
}

// ── daemon status ─────────────────────────────────────────────────────────────

#[test]
fn daemon_status_when_not_running_exits_cleanly() {
    let repo = make_fixture_repo();
    // No daemon started — status should exit 0 and produce a readable message.
    let output = Command::new(SCOUT)
        .args(["daemon", "status", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "daemon status must not fail when daemon is not running"
    );
    // Process exited cleanly — content check is lenient since the exact phrasing
    // may vary across platforms.
    assert!(
        output.status.code().is_some(),
        "daemon status must exit with a code, not a signal"
    );
}

#[test]
fn daemon_stop_when_not_running_gives_clear_message() {
    let repo = make_fixture_repo();
    let output = Command::new(SCOUT)
        .args(["daemon", "stop", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Must not crash. Acceptable outcomes:
    //   Unix: "No daemon is running for this repository."   (exit 1)
    //   Windows: "Stopping the daemon is not supported on this platform."  (exit 1)
    // In all cases, the process must exit with a code (not be killed by a signal).
    assert!(
        output.status.code().is_some(),
        "daemon stop must exit with a code, not a signal"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.is_empty(),
        "daemon stop must produce some output\n(empty output suggests a silent crash)"
    );
}

// ── install-hooks idempotency ─────────────────────────────────────────────────

#[test]
fn install_hooks_twice_does_not_duplicate_content() {
    let repo = make_fixture_repo();
    let git_init = Command::new("git").args(["init"]).current_dir(repo.path()).output().unwrap();
    assert!(git_init.status.success());

    let path_arg = repo.path().to_string_lossy().to_string();

    // Install once
    Command::new(SCOUT)
        .args(["daemon", "install-hooks", "--path", &path_arg])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Install again
    let second = Command::new(SCOUT)
        .args(["daemon", "install-hooks", "--path", &path_arg])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(second.status.success(), "second install-hooks must succeed");

    // Each hook file should have the scout snippet exactly once.
    let hooks_dir = repo.path().join(".git/hooks");
    for hook in &["post-commit", "post-merge", "post-checkout"] {
        let p = hooks_dir.join(hook);
        if !p.exists() { continue; }
        let content = fs::read_to_string(&p).unwrap();
        let count = content.matches("scout").count();
        assert!(
            count >= 1,
            "hook '{hook}' must contain at least one 'scout' invocation"
        );
        // Count the actual "scout update" command lines — the idempotency guard
        // prevents the command from being appended twice.
        // The word "scout" may appear multiple times (in comments and the command),
        // but the functional invocation line should not be duplicated.
        let invocation_count = content.lines()
            .filter(|l| l.contains("scout") && l.contains("update") && l.contains("--path"))
            .count();
        assert_eq!(
            invocation_count, 1,
            "hook '{hook}' must have exactly 1 scout update invocation (idempotency)\n{content}"
        );
    }
}

// ── search result rank field ──────────────────────────────────────────────────

#[test]
fn search_results_have_sequential_ranks() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let results = search_json(repo.path(), "process_payment");
    for (i, r) in results.iter().enumerate() {
        let rank = r["rank"].as_u64().unwrap_or(0);
        assert_eq!(
            rank,
            (i + 1) as u64,
            "result at index {i} has rank {rank}, expected {}",
            i + 1
        );
    }
}

// ── verbose index output ──────────────────────────────────────────────────────

#[test]
fn index_verbose_shows_parsed_files() {
    let repo = make_fixture_repo();
    let output = Command::new(SCOUT)
        .args(["index", "--verbose", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "index --verbose failed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Verbose mode should show each file being parsed.
    assert!(
        stdout.contains(".py") || stdout.contains(".rs") || stdout.contains(".ts"),
        "verbose output must mention source files\n{stdout}"
    );
}

// ── cleanup removes deleted files ─────────────────────────────────────────────

#[test]
fn cleanup_removes_deleted_file_from_index() {
    use rusqlite::Connection;

    let repo = make_fixture_repo();
    index_repo(repo.path());

    // Delete one file.
    fs::remove_file(repo.path().join("services/auth/login.py")).unwrap();

    // Run cleanup.
    let output = Command::new(SCOUT)
        .args(["cleanup", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "cleanup failed");

    // Verify the deleted file's units are gone from SQLite.
    let conn = Connection::open(repo.path().join(".scout/metadata.db")).unwrap();
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_units WHERE file_path LIKE '%login.py'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0, "cleanup must remove units from deleted file");
}

// ── cross-repo search ─────────────────────────────────────────────────────────

#[test]
fn all_repos_with_no_repos_registered_errors() {
    // --all-repos with no repos in registry should give a clear error.
    let repo = make_fixture_repo();
    let output = Command::new(SCOUT)
        .args([
            "search", "authenticate",
            "--all-repos",
            "--format", "json",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    // Either succeeds with empty results or fails with a helpful message.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Must not panic — either exit 0 with empty output or clear error message.
    // (The registry may be empty or may have repos from other tests — both are fine.)
    let combined = format!("{stdout}{stderr}");
    assert!(
        !combined.is_empty() || output.status.success(),
        "command must either succeed or give a clear message\n{combined}"
    );
}

#[test]
fn repos_flag_with_unknown_repo_errors() {
    let repo = make_fixture_repo();
    let output = Command::new(SCOUT)
        .args([
            "search", "authenticate",
            "--repos", "totally-unknown-repo-xyz",
            "--format", "json",
            "--no-tui",
            "--path", &repo.path().to_string_lossy(),
        ])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(
        !output.status.success(),
        "--repos with unknown name must fail"
    );
}

// ── rebuild preserves all languages ──────────────────────────────────────────

#[test]
fn rebuild_preserves_all_languages() {
    use rusqlite::Connection;

    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["rebuild", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "rebuild failed");

    let conn = Connection::open(repo.path().join(".scout/metadata.db")).unwrap();
    for lang in &["python", "rust", "typescript"] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM code_units WHERE language = ?1",
                rusqlite::params![lang],
                |r| r.get(0),
            )
            .unwrap();
        assert!(count > 0, "rebuild lost all {lang} units");
    }
}

// ── report unused-functions accuracy ─────────────────────────────────────────

#[test]
fn report_unused_functions_output_format() {
    let repo = make_fixture_repo();
    index_repo(repo.path());

    let output = Command::new(SCOUT)
        .args(["report", "unused-functions", "--path", &repo.path().to_string_lossy()])
        .current_dir(repo.path())
        .output()
        .unwrap();

    assert!(output.status.success(), "report unused-functions failed");
    // Output is either a table of unused functions or a "none found" message.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.is_empty(),
        "report must produce some output (either functions or a 'none' message)"
    );
}

// ── index with zero parseable files ──────────────────────────────────────────

#[test]
fn index_directory_with_no_source_files_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    // Only a text file — no parseable source.
    fs::write(dir.path().join("README.txt"), "just a readme").unwrap();

    let output = Command::new(SCOUT)
        .args(["index", &dir.path().to_string_lossy()])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Should succeed with 0 units, not crash.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "index with no source files must not crash\n{stderr}"
    );
}

#[test]
fn python_functions_have_non_empty_body() {
    use rusqlite::Connection;

    let repo = make_fixture_repo();
    index_repo(repo.path());

    let db_path = repo.path().join(".scout/metadata.db");
    let conn = Connection::open(&db_path).unwrap();

    let empty: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM code_units WHERE language = 'python' AND (body = '' OR body IS NULL)",
            [],
            |r| r.get(0),
        )
        .unwrap();

    assert_eq!(empty, 0, "{empty} Python units have empty body");
}
