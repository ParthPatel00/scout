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

/// Index metadata stored in `.scout/metadata.json`.
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Language::from_extension ───────────────────────────────────────────────

    #[test]
    fn from_extension_python() {
        assert_eq!(Language::from_extension("py"), Language::Python);
    }

    #[test]
    fn from_extension_rust() {
        assert_eq!(Language::from_extension("rs"), Language::Rust);
    }

    #[test]
    fn from_extension_typescript_variants() {
        assert_eq!(Language::from_extension("ts"), Language::TypeScript);
        assert_eq!(Language::from_extension("tsx"), Language::TypeScript);
    }

    #[test]
    fn from_extension_javascript_variants() {
        assert_eq!(Language::from_extension("js"), Language::JavaScript);
        assert_eq!(Language::from_extension("jsx"), Language::JavaScript);
        assert_eq!(Language::from_extension("mjs"), Language::JavaScript);
        assert_eq!(Language::from_extension("cjs"), Language::JavaScript);
    }

    #[test]
    fn from_extension_go() {
        assert_eq!(Language::from_extension("go"), Language::Go);
    }

    #[test]
    fn from_extension_java() {
        assert_eq!(Language::from_extension("java"), Language::Java);
    }

    #[test]
    fn from_extension_cpp_variants() {
        for ext in &["cpp", "cc", "cxx", "c", "h", "hpp"] {
            assert_eq!(Language::from_extension(ext), Language::Cpp, "ext={ext}");
        }
    }

    #[test]
    fn from_extension_unknown_for_unrecognised() {
        assert_eq!(Language::from_extension("rb"), Language::Unknown);
        assert_eq!(Language::from_extension("swift"), Language::Unknown);
        assert_eq!(Language::from_extension(""), Language::Unknown);
        assert_eq!(Language::from_extension("txt"), Language::Unknown);
    }

    // ── Language::as_str / Display ────────────────────────────────────────────

    #[test]
    fn language_as_str_all_variants() {
        assert_eq!(Language::Python.as_str(), "python");
        assert_eq!(Language::Rust.as_str(), "rust");
        assert_eq!(Language::TypeScript.as_str(), "typescript");
        assert_eq!(Language::JavaScript.as_str(), "javascript");
        assert_eq!(Language::Go.as_str(), "go");
        assert_eq!(Language::Java.as_str(), "java");
        assert_eq!(Language::Cpp.as_str(), "cpp");
        assert_eq!(Language::Unknown.as_str(), "unknown");
    }

    #[test]
    fn language_display_matches_as_str() {
        for lang in &[
            Language::Python,
            Language::Rust,
            Language::TypeScript,
            Language::JavaScript,
            Language::Go,
            Language::Java,
            Language::Cpp,
            Language::Unknown,
        ] {
            assert_eq!(lang.to_string(), lang.as_str());
        }
    }

    // ── UnitType::Display ─────────────────────────────────────────────────────

    #[test]
    fn unit_type_display_all_variants() {
        assert_eq!(UnitType::Function.to_string(), "function");
        assert_eq!(UnitType::Method.to_string(), "method");
        assert_eq!(UnitType::Class.to_string(), "class");
        assert_eq!(UnitType::Struct.to_string(), "struct");
        assert_eq!(UnitType::Enum.to_string(), "enum");
        assert_eq!(UnitType::Trait.to_string(), "trait");
        assert_eq!(UnitType::Interface.to_string(), "interface");
        assert_eq!(UnitType::Module.to_string(), "module");
        assert_eq!(UnitType::Other("macro".into()).to_string(), "macro");
    }

    // ── CodeUnit::new ─────────────────────────────────────────────────────────

    #[test]
    fn code_unit_new_defaults() {
        let u = CodeUnit::new(
            "src/auth.rs",
            Language::Rust,
            UnitType::Function,
            "authenticate",
            10,
            25,
            "fn authenticate() {}",
        );
        assert_eq!(u.id, 0);
        assert_eq!(u.file_path, "src/auth.rs");
        assert_eq!(u.language, Language::Rust);
        assert_eq!(u.unit_type, UnitType::Function);
        assert_eq!(u.name, "authenticate");
        assert_eq!(u.line_start, 10);
        assert_eq!(u.line_end, 25);
        assert_eq!(u.body, "fn authenticate() {}");
        assert!(u.full_signature.is_none());
        assert!(u.docstring.is_none());
        assert!(u.parameters.is_empty());
        assert!(u.return_type.is_none());
        assert!(u.calls.is_empty());
        assert!(u.imports.is_empty());
        assert_eq!(u.complexity, 1);
        assert!(!u.has_embedding);
        assert!(u.embedding_model.is_none());
    }

    // ── IndexMetadata::new ────────────────────────────────────────────────────

    #[test]
    fn index_metadata_new_version_is_current() {
        let m = IndexMetadata::new();
        assert_eq!(m.version, IndexMetadata::CURRENT_VERSION);
    }

    #[test]
    fn index_metadata_new_starts_with_zero_counts() {
        let m = IndexMetadata::new();
        assert_eq!(m.num_files, 0);
        assert_eq!(m.num_units, 0);
        assert!(m.checksum.is_empty());
    }

    #[test]
    fn index_metadata_default_equals_new() {
        let a = IndexMetadata::new();
        let b = IndexMetadata::default();
        assert_eq!(a.version, b.version);
        assert_eq!(a.num_files, b.num_files);
        assert_eq!(a.num_units, b.num_units);
    }

    // ── SearchResult JSON serialization ───────────────────────────────────────

    #[test]
    fn search_result_repo_name_omitted_when_none() {
        let unit = CodeUnit::new("f.rs", Language::Rust, UnitType::Function, "foo", 1, 5, "fn foo() {}");
        let r = SearchResult { unit, score: 0.9, snippet: "fn foo".into(), repo_name: None };
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("repo_name"), "repo_name must be omitted when None: {json}");
    }

    #[test]
    fn search_result_repo_name_present_when_some() {
        let unit = CodeUnit::new("f.rs", Language::Rust, UnitType::Function, "foo", 1, 5, "fn foo() {}");
        let r = SearchResult { unit, score: 0.9, snippet: "fn foo".into(), repo_name: Some("backend".into()) };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("repo_name"), "repo_name must be present when Some: {json}");
        assert!(json.contains("backend"), "repo_name value must appear: {json}");
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
