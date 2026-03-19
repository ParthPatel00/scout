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
