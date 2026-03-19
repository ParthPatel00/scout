use anyhow::{Context, Result};
use std::path::Path;
use tantivy::schema::*;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};

use crate::types::CodeUnit;

/// All fields we store in Tantivy.
#[derive(Clone)]
pub struct Schema {
    pub inner: tantivy::schema::Schema,
    pub id: Field,
    pub name: Field,
    pub file_path: Field,
    pub language: Field,
    pub unit_type: Field,
    pub body: Field,
    pub signature: Field,
    pub docstring: Field,
    pub line_start: Field,
    pub line_end: Field,
}

pub fn build_schema() -> Schema {
    let mut builder = SchemaBuilder::new();

    let id = builder.add_i64_field("id", INDEXED | STORED);
    let name = builder.add_text_field("name", TEXT | STORED);
    let file_path = builder.add_text_field("file_path", STRING | STORED);
    let language = builder.add_text_field("language", STRING | STORED);
    let unit_type = builder.add_text_field("unit_type", STRING | STORED);
    let body = builder.add_text_field("body", TEXT);
    let signature = builder.add_text_field("signature", TEXT | STORED);
    let docstring = builder.add_text_field("docstring", TEXT | STORED);
    let line_start = builder.add_i64_field("line_start", STORED);
    let line_end = builder.add_i64_field("line_end", STORED);

    Schema {
        inner: builder.build(),
        id,
        name,
        file_path,
        language,
        unit_type,
        body,
        signature,
        docstring,
        line_start,
        line_end,
    }
}

/// Current schema field count — bump when schema changes to trigger a rebuild.
const SCHEMA_FIELD_COUNT: usize = 10;

/// Open or create a Tantivy index at `dir`.
/// If the on-disk schema has a different number of fields (schema migration),
/// the directory is wiped and the index recreated.
pub fn open_index(dir: &Path) -> Result<(Index, Schema)> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("failed to create tantivy dir {}", dir.display()))?;

    let schema = build_schema();

    // Detect stale schema by field count.
    if dir.join("meta.json").exists() {
        match Index::open_in_dir(dir) {
            Ok(existing) => {
                if existing.schema().num_fields() == SCHEMA_FIELD_COUNT {
                    return Ok((existing, schema));
                }
                // Schema changed — wipe and recreate.
                eprintln!(
                    "note: tantivy schema changed (expected {SCHEMA_FIELD_COUNT} fields, found {}). Recreating index.",
                    existing.schema().num_fields()
                );
                std::fs::remove_dir_all(dir).ok();
                std::fs::create_dir_all(dir)?;
            }
            Err(_) => {
                std::fs::remove_dir_all(dir).ok();
                std::fs::create_dir_all(dir)?;
            }
        }
    }

    let index = Index::create_in_dir(dir, schema.inner.clone())
        .context("failed to create tantivy index")?;
    Ok((index, schema))
}

/// Add or update all units for a single file.
/// Deletes existing docs for that file first, then inserts fresh ones.
pub fn index_file_units(
    writer: &mut IndexWriter,
    schema: &Schema,
    file_path: &str,
    units: &[CodeUnit],
) -> Result<()> {
    // Delete all existing docs for this file path.
    let term = tantivy::Term::from_field_text(schema.file_path, file_path);
    writer.delete_term(term);

    for unit in units {
        let mut doc = TantivyDocument::new();
        doc.add_i64(schema.id, unit.id);
        doc.add_text(schema.name, &unit.name);
        doc.add_text(schema.file_path, &unit.file_path);
        doc.add_text(schema.language, unit.language.as_str());
        doc.add_text(schema.unit_type, unit.unit_type.to_string());
        doc.add_text(schema.body, &unit.body);
        if let Some(sig) = &unit.full_signature {
            doc.add_text(schema.signature, sig);
        }
        if let Some(ds) = &unit.docstring {
            doc.add_text(schema.docstring, ds);
        }
        doc.add_i64(schema.line_start, unit.line_start as i64);
        doc.add_i64(schema.line_end, unit.line_end as i64);
        writer.add_document(doc)?;
    }

    Ok(())
}

/// A single search hit returned from Tantivy.
#[derive(Debug, Clone)]
pub struct Hit {
    pub sqlite_id: i64,
    pub name: String,
    pub file_path: String,
    pub language: String,
    pub unit_type: String,
    pub signature: Option<String>,
    pub docstring: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
}

/// Search the Tantivy index for `query_str`, returning up to `limit` hits.
pub fn search(
    index: &Index,
    schema: &Schema,
    query_str: &str,
    limit: usize,
) -> Result<Vec<Hit>> {
    use tantivy::collector::TopDocs;
    use tantivy::query::QueryParser;

    let reader: IndexReader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::OnCommitWithDelay)
        .try_into()
        .context("failed to build index reader")?;

    let searcher = reader.searcher();

    // Search across name (highest weight), signature, docstring, body.
    let mut query_parser = QueryParser::for_index(
        index,
        vec![
            schema.name,
            schema.signature,
            schema.docstring,
            schema.body,
        ],
    );
    query_parser.set_field_boost(schema.name, 3.0);
    query_parser.set_field_boost(schema.signature, 2.0);

    let query = match query_parser.parse_query(query_str) {
        Ok(q) => q,
        Err(_) => {
            // Fall back to escaped literal search if the query is malformed.
            let escaped = escape_query(query_str);
            query_parser
                .parse_query(&escaped)
                .context("failed to parse search query")?
        }
    };

    let top_docs = searcher
        .search(&query, &TopDocs::with_limit(limit))
        .context("tantivy search failed")?;

    let mut hits = Vec::new();
    for (score, doc_addr) in top_docs {
        let doc: TantivyDocument = searcher.doc(doc_addr)?;

        let get_str = |field: Field| -> String {
            doc.get_first(field)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        };
        let get_i64 = |field: Field| -> i64 {
            doc.get_first(field)
                .and_then(|v| v.as_i64())
                .unwrap_or(0)
        };

        hits.push(Hit {
            sqlite_id: get_i64(schema.id),
            name: get_str(schema.name),
            file_path: get_str(schema.file_path),
            language: get_str(schema.language),
            unit_type: get_str(schema.unit_type),
            signature: {
                let s = get_str(schema.signature);
                if s.is_empty() { None } else { Some(s) }
            },
            docstring: {
                let s = get_str(schema.docstring);
                if s.is_empty() { None } else { Some(s) }
            },
            line_start: get_i64(schema.line_start) as usize,
            line_end: get_i64(schema.line_end) as usize,
            score,
        });
    }

    Ok(hits)
}

/// Escape special Tantivy query characters in a raw string.
fn escape_query(s: &str) -> String {
    let special = ['\\', '+', '-', '!', '(', ')', '{', '}', '[', ']', '^', '"', '~', '*', '?', ':'];
    let mut out = String::with_capacity(s.len() * 2);
    for c in s.chars() {
        if special.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
