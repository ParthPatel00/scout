use anyhow::Result;
use std::path::Path;

use crate::storage::tantivy_store::{self, Hit};
use crate::types::SearchResult;

/// Run a BM25 search against the Tantivy index at `tantivy_dir`.
pub fn search(tantivy_dir: &Path, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let (index, schema) = tantivy_store::open_index(tantivy_dir)?;
    let hits = tantivy_store::search(&index, &schema, query, limit)?;
    Ok(hits.into_iter().map(hit_to_result).collect())
}

fn lang_from_name(s: &str) -> crate::types::Language {
    match s {
        "python" => crate::types::Language::Python,
        "rust" => crate::types::Language::Rust,
        "typescript" => crate::types::Language::TypeScript,
        "javascript" => crate::types::Language::JavaScript,
        "go" => crate::types::Language::Go,
        "java" => crate::types::Language::Java,
        "cpp" => crate::types::Language::Cpp,
        _ => crate::types::Language::Unknown,
    }
}

fn hit_to_result(hit: Hit) -> SearchResult {
    let snippet = hit
        .signature
        .as_deref()
        .or(hit.docstring.as_deref())
        .unwrap_or(&hit.name)
        .to_string();

    // Build a minimal CodeUnit for display purposes.
    let unit = crate::types::CodeUnit {
        id: hit.sqlite_id,
        file_path: hit.file_path,
        language: lang_from_name(&hit.language),
        unit_type: unit_type_from_str(&hit.unit_type),
        name: hit.name,
        full_signature: hit.signature,
        docstring: hit.docstring,
        line_start: hit.line_start,
        line_end: hit.line_end,
        body: String::new(),
        parameters: vec![],
        return_type: None,
        calls: vec![],
        imports: vec![],
        complexity: 0,
        has_embedding: false,
        embedding_model: None,
    };

    SearchResult {
        unit,
        score: hit.score,
        snippet,
    }
}

fn unit_type_from_str(s: &str) -> crate::types::UnitType {
    match s {
        "function" => crate::types::UnitType::Function,
        "method" => crate::types::UnitType::Method,
        "class" => crate::types::UnitType::Class,
        "struct" => crate::types::UnitType::Struct,
        "enum" => crate::types::UnitType::Enum,
        "trait" => crate::types::UnitType::Trait,
        "interface" => crate::types::UnitType::Interface,
        "module" => crate::types::UnitType::Module,
        other => crate::types::UnitType::Other(other.to_string()),
    }
}
