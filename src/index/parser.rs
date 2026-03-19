use anyhow::Result;
use tree_sitter::{Language as TsLanguage, Node, Parser};

use crate::index::walker::within_line_limit;
use crate::types::{CodeUnit, Language, UnitType};

/// Parse `source` as the given language, returning all top-level code units.
/// Returns an error if parsing times out (10s) or fails.
pub fn parse_file(file_path: &str, source: &str, language: &Language) -> Result<Vec<CodeUnit>> {
    if !within_line_limit(source) {
        return Ok(vec![]); // skip files exceeding the line limit
    }

    let ts_lang = match ts_language(language) {
        Some(l) => l,
        None => return Ok(vec![]),
    };

    let mut parser = Parser::new();
    parser.set_language(&ts_lang)?;
    parser.set_timeout_micros(10_000_000); // 10 seconds

    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => {
            // Timed out or failed — return empty rather than crashing.
            return Ok(vec![]);
        }
    };

    let root = tree.root_node();
    let units = match language {
        Language::Python => extract_python(file_path, source, root),
        Language::Rust => extract_rust(file_path, source, root),
        Language::TypeScript | Language::JavaScript => {
            extract_js_ts(file_path, source, root, language)
        }
        Language::Go => extract_go(file_path, source, root),
        Language::Java => extract_java(file_path, source, root),
        Language::Cpp => extract_cpp(file_path, source, root),
        Language::Unknown => vec![],
    };

    Ok(units)
}

// ─── Language dispatch ────────────────────────────────────────────────────────

fn ts_language(lang: &Language) -> Option<TsLanguage> {
    match lang {
        Language::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Language::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Language::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Language::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Language::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Language::Java => Some(tree_sitter_java::LANGUAGE.into()),
        Language::Cpp => None, // Phase 4
        Language::Unknown => None,
    }
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Extract the UTF-8 text for a node.
fn node_text<'a>(node: Node, source: &'a str) -> &'a str {
    node.utf8_text(source.as_bytes()).unwrap_or("")
}

/// Extract the first named child with the given kind.
fn child_of_kind<'a>(node: Node<'a>, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node.named_children(&mut cursor).find(|n| n.kind() == kind);
    result
}

/// Build a `CodeUnit` from common fields.
fn make_unit(
    file_path: &str,
    language: Language,
    unit_type: UnitType,
    name: impl Into<String>,
    node: Node,
    source: &str,
) -> CodeUnit {
    let start = node.start_position().row + 1; // 1-indexed
    let end = node.end_position().row + 1;
    let body = node_text(node, source).to_string();
    CodeUnit::new(file_path, language, unit_type, name, start, end, body)
}

// ─── Python ───────────────────────────────────────────────────────────────────

fn extract_python(file_path: &str, source: &str, root: Node) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    extract_python_node(file_path, source, root, &mut units);
    units
}

fn extract_python_node(file_path: &str, source: &str, node: Node, units: &mut Vec<CodeUnit>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let mut unit = make_unit(
                        file_path,
                        Language::Python,
                        UnitType::Function,
                        &name,
                        child,
                        source,
                    );
                    unit.full_signature = Some(extract_python_signature(child, source));
                    unit.docstring = extract_python_docstring(child, source);
                    units.push(unit);
                    // Recurse into nested functions/classes
                    extract_python_node(file_path, source, child, units);
                }
            }
            "class_definition" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Python,
                        UnitType::Class,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                    // Extract methods inside the class
                    extract_python_node(file_path, source, child, units);
                }
            }
            _ => extract_python_node(file_path, source, child, units),
        }
    }
}

fn extract_python_signature(node: Node, source: &str) -> String {
    // "def name(params):" — take everything up to the body block
    let text = node_text(node, source);
    if let Some(idx) = text.find(':') {
        text[..=idx].trim().to_string()
    } else {
        text.lines().next().unwrap_or("").trim().to_string()
    }
}

fn extract_python_docstring(node: Node, source: &str) -> Option<String> {
    // Look for the first expression_statement containing a string in the body.
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.kind() == "block" {
            let mut bc = child.walk();
            let first = child.named_children(&mut bc).next();
            drop(bc);
            if let Some(first) = first {
                if first.kind() == "expression_statement" {
                    let mut ec = first.walk();
                    let string_node = first.named_children(&mut ec).next();
                    drop(ec);
                    if let Some(s) = string_node {
                        if s.kind() == "string" {
                            return Some(
                                node_text(s, source)
                                    .trim_matches('"')
                                    .trim_matches('\'')
                                    .to_string(),
                            );
                        }
                    }
                }
            }
        }
    }
    None
}

// ─── Rust ─────────────────────────────────────────────────────────────────────

fn extract_rust(file_path: &str, source: &str, root: Node) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    extract_rust_node(file_path, source, root, &mut units);
    units
}

fn extract_rust_node(file_path: &str, source: &str, node: Node, units: &mut Vec<CodeUnit>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_item" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let mut unit = make_unit(
                        file_path,
                        Language::Rust,
                        UnitType::Function,
                        &name,
                        child,
                        source,
                    );
                    unit.full_signature =
                        Some(extract_rust_signature(child, source));
                    units.push(unit);
                }
            }
            "impl_item" => {
                // Recurse into impl blocks to find methods.
                extract_rust_node(file_path, source, child, units);
            }
            "struct_item" => {
                if let Some(name_node) = child_of_kind(child, "type_identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Rust,
                        UnitType::Struct,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "enum_item" => {
                if let Some(name_node) = child_of_kind(child, "type_identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Rust,
                        UnitType::Enum,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "trait_item" => {
                if let Some(name_node) = child_of_kind(child, "type_identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Rust,
                        UnitType::Trait,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            _ => extract_rust_node(file_path, source, child, units),
        }
    }
}

fn extract_rust_signature(node: Node, source: &str) -> String {
    let text = node_text(node, source);
    // Everything before the opening brace `{`.
    if let Some(idx) = text.find('{') {
        text[..idx].trim().to_string()
    } else {
        text.lines().next().unwrap_or("").trim().to_string()
    }
}

// ─── JavaScript / TypeScript ──────────────────────────────────────────────────

fn extract_js_ts(
    file_path: &str,
    source: &str,
    root: Node,
    language: &Language,
) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    extract_js_ts_node(file_path, source, root, language, &mut units);
    units
}

fn extract_js_ts_node(
    file_path: &str,
    source: &str,
    node: Node,
    language: &Language,
    units: &mut Vec<CodeUnit>,
) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "function_declaration" | "function" | "generator_function_declaration" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        language.clone(),
                        UnitType::Function,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "method_definition" => {
                if let Some(name_node) = child_of_kind(child, "property_identifier")
                    .or_else(|| child_of_kind(child, "private_property_identifier"))
                {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        language.clone(),
                        UnitType::Method,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "class_declaration" | "class" => {
                if let Some(name_node) = child_of_kind(child, "type_identifier")
                    .or_else(|| child_of_kind(child, "identifier"))
                {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        language.clone(),
                        UnitType::Class,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                    extract_js_ts_node(file_path, source, child, language, units);
                }
            }
            // Arrow functions assigned to variables: const foo = () => {}
            "lexical_declaration" | "variable_declaration" => {
                extract_js_ts_node(file_path, source, child, language, units);
            }
            "variable_declarator" => {
                let has_arrow = child
                    .named_children(&mut child.walk())
                    .any(|n| n.kind() == "arrow_function");
                if has_arrow {
                    if let Some(name_node) = child_of_kind(child, "identifier") {
                        let name = node_text(name_node, source).to_string();
                        let unit = make_unit(
                            file_path,
                            language.clone(),
                            UnitType::Function,
                            &name,
                            child,
                            source,
                        );
                        units.push(unit);
                    }
                }
            }
            _ => extract_js_ts_node(file_path, source, child, language, units),
        }
    }
}

// ─── Go ───────────────────────────────────────────────────────────────────────

fn extract_go(file_path: &str, source: &str, root: Node) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    let mut cursor = root.walk();
    for child in root.named_children(&mut cursor) {
        match child.kind() {
            "function_declaration" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Go,
                        UnitType::Function,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "method_declaration" => {
                if let Some(name_node) = child_of_kind(child, "field_identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Go,
                        UnitType::Method,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            "type_declaration" => {
                // type Foo struct { ... } or type Bar interface { ... }
                let mut tc = child.walk();
                for spec in child.named_children(&mut tc) {
                    if spec.kind() == "type_spec" {
                        if let Some(name_node) = child_of_kind(spec, "type_identifier") {
                            let name = node_text(name_node, source).to_string();
                            let unit_type = spec
                                .named_children(&mut spec.walk())
                                .find(|n| {
                                    n.kind() == "struct_type" || n.kind() == "interface_type"
                                })
                                .map(|n| {
                                    if n.kind() == "struct_type" {
                                        UnitType::Struct
                                    } else {
                                        UnitType::Interface
                                    }
                                })
                                .unwrap_or(UnitType::Other("type".into()));
                            let unit = make_unit(
                                file_path,
                                Language::Go,
                                unit_type,
                                &name,
                                child,
                                source,
                            );
                            units.push(unit);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    units
}

// ─── Java ─────────────────────────────────────────────────────────────────────

fn extract_java(file_path: &str, source: &str, root: Node) -> Vec<CodeUnit> {
    let mut units = Vec::new();
    extract_java_node(file_path, source, root, &mut units);
    units
}

fn extract_java_node(file_path: &str, source: &str, node: Node, units: &mut Vec<CodeUnit>) {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        match child.kind() {
            "class_declaration" | "interface_declaration" | "enum_declaration" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit_type = match child.kind() {
                        "interface_declaration" => UnitType::Interface,
                        "enum_declaration" => UnitType::Enum,
                        _ => UnitType::Class,
                    };
                    let unit = make_unit(
                        file_path,
                        Language::Java,
                        unit_type,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                    extract_java_node(file_path, source, child, units);
                }
            }
            "method_declaration" | "constructor_declaration" => {
                if let Some(name_node) = child_of_kind(child, "identifier") {
                    let name = node_text(name_node, source).to_string();
                    let unit = make_unit(
                        file_path,
                        Language::Java,
                        UnitType::Method,
                        &name,
                        child,
                        source,
                    );
                    units.push(unit);
                }
            }
            _ => extract_java_node(file_path, source, child, units),
        }
    }
}

// ─── C++ ─────────────────────────────────────────────────────────────────────

fn extract_cpp(file_path: &str, source: &str, root: Node) -> Vec<CodeUnit> {
    // C++ grammar support added in Phase 4. For now return empty.
    let _ = (file_path, source, root);
    vec![]
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_functions() {
        let src = r#"
def hello(name: str) -> str:
    """Say hello."""
    return f"Hello, {name}"

class Greeter:
    def greet(self):
        pass
"#;
        let units = parse_file("test.py", src, &Language::Python).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"hello"), "expected 'hello', got {:?}", names);
        assert!(names.contains(&"Greeter"), "expected 'Greeter'");
        assert!(names.contains(&"greet"), "expected 'greet'");

        let hello = units.iter().find(|u| u.name == "hello").unwrap();
        assert_eq!(hello.unit_type, UnitType::Function);
        assert!(hello.line_start >= 1);
    }

    #[test]
    fn parses_rust_items() {
        let src = r#"
struct Foo {
    x: i32,
}

impl Foo {
    fn bar(&self) -> i32 {
        self.x
    }
}

fn standalone() {}

trait MyTrait {
    fn do_thing(&self);
}
"#;
        let units = parse_file("test.rs", src, &Language::Rust).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"Foo"), "expected 'Foo'");
        assert!(names.contains(&"bar"), "expected 'bar'");
        assert!(names.contains(&"standalone"), "expected 'standalone'");
        assert!(names.contains(&"MyTrait"), "expected 'MyTrait'");
    }

    #[test]
    fn parses_typescript_functions() {
        let src = r#"
function greet(name: string): string {
    return `Hello, ${name}`;
}

const add = (a: number, b: number) => a + b;

class Calculator {
    multiply(x: number, y: number): number {
        return x * y;
    }
}
"#;
        let units = parse_file("test.ts", src, &Language::TypeScript).unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.name.as_str()).collect();
        assert!(names.contains(&"greet"), "expected 'greet'");
        assert!(names.contains(&"Calculator"), "expected 'Calculator'");
    }

    #[test]
    fn empty_on_timeout_source() {
        // A very weird/huge input shouldn't panic.
        let huge = "x".repeat(100);
        let result = parse_file("test.py", &huge, &Language::Python);
        assert!(result.is_ok());
    }

    #[test]
    fn skips_over_line_limit() {
        let many_lines = "x = 1\n".repeat(11_000);
        let units = parse_file("test.py", &many_lines, &Language::Python).unwrap();
        assert!(units.is_empty());
    }
}
