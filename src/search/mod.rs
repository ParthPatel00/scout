pub mod bm25;
pub mod cross_repo;
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
    // Normalise OS path separators so patterns work on both Windows (\) and Unix (/).
    let lower = path.replace('\\', "/").to_ascii_lowercase();
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── SearchFilter::is_empty ────────────────────────────────────────────────

    #[test]
    fn is_empty_default() {
        assert!(SearchFilter::default().is_empty());
    }

    #[test]
    fn is_empty_false_when_lang_set() {
        let f = SearchFilter { lang: Some("rust".into()), ..Default::default() };
        assert!(!f.is_empty());
    }

    #[test]
    fn is_empty_false_when_path_prefix_set() {
        let f = SearchFilter { path_prefix: Some("src/".into()), ..Default::default() };
        assert!(!f.is_empty());
    }

    #[test]
    fn is_empty_false_when_modified_since_set() {
        let f = SearchFilter { modified_since: Some(0), ..Default::default() };
        assert!(!f.is_empty());
    }

    #[test]
    fn is_empty_false_when_exclude_tests() {
        let f = SearchFilter { exclude_tests: true, ..Default::default() };
        assert!(!f.is_empty());
    }

    // ── SearchFilter::matches_lang ────────────────────────────────────────────

    #[test]
    fn matches_lang_no_filter_always_true() {
        let f = SearchFilter::default();
        assert!(f.matches_lang("rust"));
        assert!(f.matches_lang("python"));
    }

    #[test]
    fn matches_lang_exact_match() {
        let f = SearchFilter { lang: Some("go".into()), ..Default::default() };
        assert!(f.matches_lang("go"));
        assert!(!f.matches_lang("rust"));
        assert!(!f.matches_lang("Go")); // case-sensitive
    }

    // ── SearchFilter::matches_path ────────────────────────────────────────────

    #[test]
    fn matches_path_no_filter_always_true() {
        let f = SearchFilter::default();
        assert!(f.matches_path("src/anything.rs"));
    }

    #[test]
    fn matches_path_prefix_filter() {
        let f = SearchFilter { path_prefix: Some("src/auth".into()), ..Default::default() };
        assert!(f.matches_path("src/auth/login.rs"));
        assert!(!f.matches_path("src/payments/checkout.rs"));
    }

    #[test]
    fn matches_path_exclude_tests() {
        let f = SearchFilter { exclude_tests: true, ..Default::default() };
        assert!(!f.matches_path("src/tests/auth_test.rs"));
        assert!(!f.matches_path("src/auth_test.go"));
        assert!(f.matches_path("src/auth.rs"));
    }

    #[test]
    fn matches_path_prefix_and_exclude_tests_combined() {
        let f = SearchFilter {
            path_prefix: Some("src/".into()),
            exclude_tests: true,
            ..Default::default()
        };
        assert!(f.matches_path("src/auth.rs"));
        assert!(!f.matches_path("src/tests/auth.rs"));
        assert!(!f.matches_path("lib/auth.rs")); // prefix not matched
    }

    // ── is_test_file ──────────────────────────────────────────────────────────

    #[test]
    fn is_test_file_detects_patterns() {
        assert!(is_test_file("src/tests/auth.rs"));
        assert!(is_test_file("src/auth_test.go"));
        assert!(is_test_file("src/auth.test.ts"));
        assert!(is_test_file("src/auth.spec.js"));
        assert!(is_test_file("src/spec/login.py"));
        assert!(is_test_file("test_auth.py"));
        assert!(is_test_file("src/test_helper.py"));
    }

    #[test]
    fn is_test_file_excludes_non_test_files() {
        assert!(!is_test_file("src/auth.rs"));
        assert!(!is_test_file("src/attestation.rs")); // contains "test" but not in pattern
        assert!(!is_test_file("src/utils/helpers.ts"));
        assert!(!is_test_file("src/context/provider.ts")); // "context" has no test pattern
    }

    #[test]
    fn is_test_file_slash_test_slash() {
        assert!(is_test_file("src/test/helpers.rs"));
    }

    #[test]
    fn is_test_file_slash_tests_slash() {
        assert!(is_test_file("project/tests/unit/auth.py"));
    }

    #[test]
    fn is_test_file_slash_spec_slash() {
        assert!(is_test_file("src/spec/auth_spec.rb"));
    }

    #[test]
    fn is_test_file_slash_specs_slash() {
        assert!(is_test_file("src/specs/login_spec.js"));
    }

    #[test]
    fn is_test_file_dot_test_dot() {
        assert!(is_test_file("src/auth.test.js"));
        assert!(is_test_file("src/auth.test.ts"));
        assert!(is_test_file("src/auth.test.py"));
    }

    #[test]
    fn is_test_file_dot_spec_dot() {
        assert!(is_test_file("src/auth.spec.ts"));
        assert!(is_test_file("components/Login.spec.jsx"));
    }

    #[test]
    fn is_test_file_underscore_test_dot() {
        assert!(is_test_file("src/auth_test.go"));
        assert!(is_test_file("handlers/payment_test.go"));
    }

    #[test]
    fn is_test_file_test_underscore_prefix() {
        assert!(is_test_file("test_auth.py"));
        assert!(is_test_file("test_payments.py"));
    }

    #[test]
    fn is_test_file_slash_test_underscore_infix() {
        assert!(is_test_file("src/test_helpers/util.rs"));
        assert!(is_test_file("src/test_utils.go"));
    }

    // Windows backslash paths must be handled identically to forward-slash paths.
    #[test]
    fn is_test_file_windows_backslash_tests_dir() {
        assert!(is_test_file("project\\tests\\unit\\auth.py"));
    }

    #[test]
    fn is_test_file_windows_backslash_test_dir() {
        assert!(is_test_file("src\\test\\helpers.rs"));
    }

    #[test]
    fn is_test_file_windows_backslash_test_underscore() {
        assert!(is_test_file("src\\test_utils.go"));
    }

    #[test]
    fn is_test_file_windows_backslash_non_test_file() {
        assert!(!is_test_file("services\\auth\\login.rs"));
        assert!(!is_test_file("src\\payments\\processor.rs"));
    }

    #[test]
    fn is_test_file_case_insensitive() {
        // Patterns are matched after to_ascii_lowercase
        assert!(is_test_file("src/Tests/auth.rs"));
        assert!(is_test_file("src/AUTH_TEST.go"));
        assert!(is_test_file("src/Auth.Test.ts"));
    }

    #[test]
    fn is_test_file_regular_files_not_matched() {
        assert!(!is_test_file("src/auth/login.rs"));
        assert!(!is_test_file("services/payments/processor.rs"));
        assert!(!is_test_file("frontend/src/api.ts"));
        assert!(!is_test_file("main.go"));
        assert!(!is_test_file("latest_data.json")); // "test" substring but no pattern match
    }

    // ── matches_lang edge cases ────────────────────────────────────────────────

    #[test]
    fn matches_lang_case_sensitive() {
        let f = SearchFilter { lang: Some("python".into()), ..Default::default() };
        assert!(f.matches_lang("python"));
        assert!(!f.matches_lang("Python"));
        assert!(!f.matches_lang("PYTHON"));
    }

    #[test]
    fn matches_lang_empty_string() {
        let f = SearchFilter { lang: Some("".into()), ..Default::default() };
        // Empty filter matches only empty language strings.
        assert!(f.matches_lang(""));
        assert!(!f.matches_lang("rust"));
    }

    // ── matches_path edge cases ────────────────────────────────────────────────

    #[test]
    fn matches_path_empty_prefix_matches_everything() {
        let f = SearchFilter { path_prefix: Some("".into()), ..Default::default() };
        assert!(f.matches_path("src/anything.rs")); // empty string is contained in every string
    }

    #[test]
    fn matches_path_prefix_is_substring_not_prefix() {
        // path_prefix is a substring match, not a strict path prefix.
        let f = SearchFilter { path_prefix: Some("auth".into()), ..Default::default() };
        assert!(f.matches_path("src/auth/login.rs"));
        assert!(f.matches_path("services/authentication/handler.py")); // "auth" is a substring
        assert!(!f.matches_path("src/payments/checkout.rs"));
    }

    #[test]
    fn matches_path_combine_lang_path_both_must_match() {
        let f = SearchFilter {
            lang: Some("rust".into()),
            path_prefix: Some("payments".into()),
            ..Default::default()
        };
        // matches_path and matches_lang are independent — test both together at call site
        assert!(f.matches_path("services/payments/processor.rs"));
        assert!(f.matches_lang("rust"));
        assert!(!f.matches_path("services/auth/login.rs"));
        assert!(!f.matches_lang("python"));
    }
}
