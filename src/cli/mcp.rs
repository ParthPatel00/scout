//! `scout mcp` — stdio MCP server for Claude Code and other MCP clients.
//!
//! Implements the Model Context Protocol (JSON-RPC 2.0 over stdio).
//! Exposes two tools:
//!   - scout_search: hybrid semantic + BM25 search
//!   - scout_index:  index a repository so it can be searched
//!
//! Configure in ~/.claude.json or .mcp.json:
//!   { "mcpServers": { "scout": { "command": "scout", "args": ["mcp"] } } }

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use serde_json::{json, Value};

use crate::index;
use crate::search::{hybrid, SearchFilter};

pub struct McpArgs {
    pub path: PathBuf,
}

pub fn run(args: McpArgs) -> Result<()> {
    // Spawn a background thread to check for updates without blocking the server.
    let update_notice: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let notice_flag = Arc::clone(&update_notice);
    std::thread::spawn(move || {
        if let Ok(latest) = crate::cli::update::fetch_latest_version() {
            if crate::cli::update::is_newer(&latest, env!("CARGO_PKG_VERSION")) {
                *notice_flag.lock().unwrap() = Some(format!(
                    "Scout v{latest} is available. Run `scout update` in your terminal, \
                     then restart Claude Code to load the new version."
                ));
            }
        }
    });

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    serve(
        BufReader::new(stdin.lock()),
        &mut stdout.lock(),
        &args.path,
        update_notice,
    )
}

// ── Server loop ───────────────────────────────────────────────────────────────

fn serve(
    reader: impl BufRead,
    writer: &mut impl Write,
    default_path: &Path,
    update_notice: Arc<Mutex<Option<String>>>,
) -> Result<()> {
    for line in reader.lines() {
        let line = match line {
            Ok(l) if l.trim().is_empty() => continue,
            Ok(l) => l,
            Err(_) => break,
        };

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let err = make_error(Value::Null, -32700, &format!("Parse error: {e}"));
                write_msg(writer, &err)?;
                continue;
            }
        };

        // JSON-RPC 2.0: requests have "id", notifications do not.
        // Notifications MUST NOT receive a response.
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");

        match (method, id) {
            // Notifications — no response.
            (_, None) => continue,

            // initialize: respond then send our own initialized notification.
            ("initialize", Some(id)) => {
                write_msg(writer, &handle_initialize(id))?;
            }

            ("tools/list", Some(id)) => {
                write_msg(writer, &handle_tools_list(id))?;
            }

            ("tools/call", Some(id)) => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let response = handle_tool_call(id, &params, default_path, &update_notice);
                write_msg(writer, &response)?;
            }

            (method, Some(id)) => {
                let err = make_error(id, -32601, &format!("Method not found: {method}"));
                write_msg(writer, &err)?;
            }
        }
    }
    Ok(())
}

fn write_msg(writer: &mut impl Write, value: &Value) -> Result<()> {
    let line = serde_json::to_string(value)?;
    writer.write_all(line.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

// ── Protocol handlers ─────────────────────────────────────────────────────────

fn handle_initialize(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "scout",
                "version": env!("CARGO_PKG_VERSION")
            }
        }
    })
}

fn handle_tools_list(id: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "scout_search",
                    "description": "Semantic + keyword hybrid search over an indexed codebase. \
                        Returns ranked functions, methods, and classes matching the query. \
                        Use this instead of grep or glob for any conceptual or keyword code search — \
                        one call replaces many grep iterations and understands what code does, \
                        not just what it says.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "Natural language or keyword query, e.g. 'JWT token validation' or 'database connection pool'"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Max results to return (default: 10)",
                                "default": 10
                            },
                            "lang": {
                                "type": "string",
                                "description": "Filter by language: python, rust, go, java, typescript, javascript, cpp"
                            },
                            "path_filter": {
                                "type": "string",
                                "description": "Restrict to files whose path contains this substring, e.g. 'src/auth'"
                            },
                            "semantic": {
                                "type": "boolean",
                                "description": "Force pure vector search (requires embedding model). Default false uses hybrid BM25+vector.",
                                "default": false
                            },
                            "path": {
                                "type": "string",
                                "description": "Repository root path (default: directory at server start)"
                            }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "scout_index",
                    "description": "Index a codebase so it can be searched with scout_search. \
                        Call this if scout_search returns an error about no index being found.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {
                                "type": "string",
                                "description": "Repository root path to index (default: directory at server start)"
                            }
                        }
                    }
                }
            ]
        }
    })
}

fn handle_tool_call(
    id: Value,
    params: &Value,
    default_path: &Path,
    update_notice: &Arc<Mutex<Option<String>>>,
) -> Value {
    let tool = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    match tool {
        "scout_search" => call_scout_search(id, &args, default_path, update_notice),
        "scout_index"  => call_scout_index(id, &args, default_path),
        _ => make_error(id, -32602, &format!("Unknown tool: {tool}")),
    }
}

// ── Tool: scout_search ────────────────────────────────────────────────────────

fn call_scout_search(
    id: Value,
    args: &Value,
    default_path: &Path,
    update_notice: &Arc<Mutex<Option<String>>>,
) -> Value {
    let query = match args.get("query").and_then(|q| q.as_str()) {
        Some(q) => q.to_string(),
        None => return make_tool_error(id, "Missing required parameter: query"),
    };

    let limit = args.get("limit").and_then(|l| l.as_u64()).map(|l| l as usize).unwrap_or(10);
    let lang = args.get("lang").and_then(|l| l.as_str()).map(str::to_string);
    let path_prefix = args.get("path_filter").and_then(|p| p.as_str()).map(str::to_string);
    let semantic = args.get("semantic").and_then(|s| s.as_bool()).unwrap_or(false);

    let repo_root = args.get("path")
        .and_then(|p| p.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_path.to_path_buf());

    let root = match repo_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return make_tool_error(id, &format!("Invalid path: {e}")),
    };

    let idx_dir = match index::index_dir(&root) {
        Ok(d) => d,
        Err(e) => return make_tool_error(id, &format!("Could not resolve index directory: {e}")),
    };

    let tantivy_dir = idx_dir.join("tantivy");
    let vector_path = idx_dir.join("vectors.bin");

    if !tantivy_dir.join("meta.json").exists() {
        return make_tool_error(
            id,
            &format!(
                "No index found at {}. Call scout_index first, or run `scout index` in the repository.",
                root.display()
            ),
        );
    }

    let filter = SearchFilter {
        lang,
        path_prefix,
        modified_since: None,
        exclude_tests: false,
    };

    let model = crate::ml::model::load_model().ok();

    let results = if semantic {
        match &model {
            Some(m) => hybrid::search_semantic_only(&vector_path, &query, limit, &filter, m.as_ref()),
            None => return make_tool_error(
                id,
                "Semantic search requires the embedding model. \
                 Run `scout index --download-model` to download it (~500 MB), then re-index.",
            ),
        }
    } else {
        hybrid::search(&tantivy_dir, &vector_path, &query, limit, &filter, model.as_deref())
    };

    let results = match results {
        Ok(r) => r,
        Err(e) => return make_tool_error(id, &format!("Search error: {e}")),
    };

    let mut text = if results.is_empty() {
        format!("No results for query: {query}")
    } else {
        format_results_for_llm(&results)
    };

    // Append update notice if the background check found a newer version.
    if let Ok(guard) = update_notice.lock() {
        if let Some(notice) = &*guard {
            text.push_str("\n---\n");
            text.push_str(notice);
        }
    }

    make_tool_result(id, &text)
}

// ── Tool: scout_index ─────────────────────────────────────────────────────────

fn call_scout_index(id: Value, args: &Value, default_path: &Path) -> Value {
    let repo_root = args.get("path")
        .and_then(|p| p.as_str())
        .map(PathBuf::from)
        .unwrap_or_else(|| default_path.to_path_buf());

    // NOTE: cli::index::run() emits progress via println! to stdout, which will
    // temporarily interleave with the MCP JSON-RPC stream while indexing runs.
    // This is acceptable for this infrequent setup tool — MCP clients discard
    // lines that are not valid JSON-RPC. The final response below is always valid.
    match crate::cli::index::run(crate::cli::index::IndexArgs {
        path: repo_root,
        verbose: false,
    }) {
        Ok(()) => make_tool_result(id, "Indexing complete. You can now use scout_search."),
        Err(e) => make_tool_error(id, &format!("Indexing failed: {e}")),
    }
}

// ── Output formatting ─────────────────────────────────────────────────────────

fn format_results_for_llm(results: &[crate::types::SearchResult]) -> String {
    let mut out = format!("{} result(s):\n\n", results.len());

    for (i, r) in results.iter().enumerate() {
        let u = &r.unit;

        out.push_str(&format!(
            "[{}] {}:{}  {}  {}\n",
            i + 1,
            u.file_path,
            u.line_start,
            u.unit_type,
            u.language,
        ));

        if let Some(sig) = &u.full_signature {
            let first = sig.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                out.push_str(&format!("    {first}\n"));
            }
        }

        if let Some(doc) = &u.docstring {
            let first = doc.lines().next().unwrap_or("").trim();
            if !first.is_empty() {
                let truncated = if first.len() > 120 {
                    format!("{}...", &first[..120])
                } else {
                    first.to_string()
                };
                out.push_str(&format!("    # {truncated}\n"));
            }
        }

        if !u.body.is_empty() {
            let snippet: Vec<&str> = u.body
                .lines()
                .skip(1)
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty()
                        && !t.starts_with("//")
                        && !t.starts_with('#')
                        && !t.starts_with("/*")
                        && !t.starts_with('*')
                })
                .take(3)
                .collect();
            for l in snippet {
                out.push_str(&format!("    {}\n", l.trim()));
            }
        }

        out.push('\n');
    }

    out
}

// ── Response helpers ──────────────────────────────────────────────────────────

fn make_tool_result(id: Value, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": text }]
        }
    })
}

fn make_tool_error(id: Value, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "content": [{ "type": "text", "text": message }],
            "isError": true
        }
    })
}

fn make_error(id: Value, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}
