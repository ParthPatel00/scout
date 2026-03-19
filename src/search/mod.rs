pub mod bm25;
pub mod hybrid;
pub mod rrf;

/// Filters applied to search results.
#[derive(Debug, Clone, Default)]
pub struct SearchFilter {
    /// Restrict to a specific language (e.g. "python", "rust").
    pub lang: Option<String>,
    /// Restrict to files whose path contains this substring.
    pub path_prefix: Option<String>,
    /// Restrict to files indexed within the last N seconds.
    pub modified_since: Option<i64>,
    /// Exclude test files (paths containing test/spec patterns).
    pub exclude_tests: bool,
}

impl SearchFilter {
    pub fn is_empty(&self) -> bool {
        self.lang.is_none()
            && self.path_prefix.is_none()
            && self.modified_since.is_none()
            && !self.exclude_tests
    }

    pub fn matches_path(&self, file_path: &str) -> bool {
        if let Some(prefix) = &self.path_prefix {
            if !file_path.contains(prefix.as_str()) {
                return false;
            }
        }
        if self.exclude_tests && is_test_file(file_path) {
            return false;
        }
        true
    }

    pub fn matches_lang(&self, lang: &str) -> bool {
        if let Some(filter_lang) = &self.lang {
            return lang == filter_lang.as_str();
        }
        true
    }
}

pub fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.contains("/spec/")
        || lower.contains("/specs/")
        || lower.contains("_test.")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.starts_with("test_")
        || lower.contains("/test_")
}
