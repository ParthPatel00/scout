use serde::{Deserialize, Serialize};

/// The type of a code unit extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitType {
    Function,
    Method,
    Class,
    Struct,
    Enum,
    Trait,
    Interface,
    Module,
    Other(String),
}

impl std::fmt::Display for UnitType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnitType::Function => write!(f, "function"),
            UnitType::Method => write!(f, "method"),
            UnitType::Class => write!(f, "class"),
            UnitType::Struct => write!(f, "struct"),
            UnitType::Enum => write!(f, "enum"),
            UnitType::Trait => write!(f, "trait"),
            UnitType::Interface => write!(f, "interface"),
            UnitType::Module => write!(f, "module"),
            UnitType::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Programming language of a source file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    TypeScript,
    JavaScript,
    Go,
    Java,
    Cpp,
    Unknown,
}

impl Language {
    pub fn from_extension(ext: &str) -> Self {
        match ext {
            "py" => Language::Python,
            "rs" => Language::Rust,
            "ts" | "tsx" => Language::TypeScript,
            "js" | "jsx" | "mjs" | "cjs" => Language::JavaScript,
            "go" => Language::Go,
            "java" => Language::Java,
            "cpp" | "cc" | "cxx" | "c" | "h" | "hpp" => Language::Cpp,
            _ => Language::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Go => "go",
            Language::Java => "java",
            Language::Cpp => "cpp",
            Language::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single parsed code unit (function, class, method, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeUnit {
    /// Unique ID assigned by the database (0 until persisted).
    pub id: i64,
    pub file_path: String,
    pub language: Language,
    pub unit_type: UnitType,
    pub name: String,
    pub full_signature: Option<String>,
    pub docstring: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub body: String,
    /// JSON array of parameter names.
    pub parameters: Vec<String>,
    pub return_type: Option<String>,
    /// Names of functions/methods this unit calls.
    pub calls: Vec<String>,
    /// Import paths used by this unit.
    pub imports: Vec<String>,
    pub complexity: u32,
    pub has_embedding: bool,
    pub embedding_model: Option<String>,
}

impl CodeUnit {
    pub fn new(
        file_path: impl Into<String>,
        language: Language,
        unit_type: UnitType,
        name: impl Into<String>,
        line_start: usize,
        line_end: usize,
        body: impl Into<String>,
    ) -> Self {
        Self {
            id: 0,
            file_path: file_path.into(),
            language,
            unit_type,
            name: name.into(),
            full_signature: None,
            docstring: None,
            line_start,
            line_end,
            body: body.into(),
            parameters: vec![],
            return_type: None,
            calls: vec![],
            imports: vec![],
            complexity: 1,
            has_embedding: false,
            embedding_model: None,
        }
    }
}

/// Per-file tracking record for incremental reindexing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRecord {
    pub file_path: String,
    pub file_hash: String,
    pub last_indexed: i64,
    pub needs_reindex: bool,
}

/// An edge in the call graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallEdge {
    pub caller_id: i64,
    pub callee_name: String,
    pub line_number: usize,
}

/// Index metadata stored in `.codesearch/metadata.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexMetadata {
    pub version: u32,
    pub created_at: i64,
    pub last_updated: i64,
    pub checksum: String,
    pub num_files: usize,
    pub num_units: usize,
}

impl IndexMetadata {
    pub const CURRENT_VERSION: u32 = 1;

    pub fn new() -> Self {
        let now = chrono::Utc::now().timestamp();
        Self {
            version: Self::CURRENT_VERSION,
            created_at: now,
            last_updated: now,
            checksum: String::new(),
            num_files: 0,
            num_units: 0,
        }
    }
}

impl Default for IndexMetadata {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a search query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub unit: CodeUnit,
    pub score: f32,
    pub snippet: String,
    /// Set for cross-repo results; None for single-repo searches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_name: Option<String>,
}
