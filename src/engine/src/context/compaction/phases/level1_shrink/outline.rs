//! Tree-sitter based code outline extraction for `read_file` tool results.
//!
//! When compacting context, source code from `read_file` can be replaced with
//! a structural outline (function signatures, class/struct declarations) that
//! preserves semantic information while using far fewer tokens.

use tree_sitter::Parser;

// ---------------------------------------------------------------------------
// Language spec — declares which AST nodes are declarations vs containers
// ---------------------------------------------------------------------------

/// Describes how to extract an outline for a specific language.
struct OutlineSpec {
    language: tree_sitter::Language,
    /// Leaf declaration nodes — extract signature line, fold body.
    declaration_kinds: &'static [&'static str],
    /// Container nodes — keep signature, recurse into children.
    container_kinds: &'static [&'static str],
}

/// Return the outline spec for a file extension, if supported.
fn spec_for_extension(ext: &str) -> Option<OutlineSpec> {
    let ext_lower = ext.to_lowercase();
    match ext_lower.as_str() {
        "rs" => Some(OutlineSpec {
            language: tree_sitter_rust::LANGUAGE.into(),
            declaration_kinds: &[
                "function_item",
                "struct_item",
                "enum_item",
                "type_item",
                "const_item",
                "static_item",
                "macro_definition",
                "use_declaration",
            ],
            container_kinds: &["impl_item", "trait_item", "mod_item"],
        }),
        "py" => Some(OutlineSpec {
            language: tree_sitter_python::LANGUAGE.into(),
            declaration_kinds: &["function_definition", "decorated_definition"],
            container_kinds: &["class_definition"],
        }),
        "js" | "jsx" => Some(OutlineSpec {
            language: tree_sitter_javascript::LANGUAGE.into(),
            declaration_kinds: &[
                "function_declaration",
                "method_definition",
                "lexical_declaration",
                "variable_declaration",
                "import_statement",
            ],
            container_kinds: &["class_declaration", "export_statement"],
        }),
        "ts" => Some(OutlineSpec {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            declaration_kinds: &[
                "function_declaration",
                "method_definition",
                "lexical_declaration",
                "variable_declaration",
                "import_statement",
                "type_alias_declaration",
                "interface_declaration",
            ],
            container_kinds: &["class_declaration", "export_statement"],
        }),
        "tsx" => Some(OutlineSpec {
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
            declaration_kinds: &[
                "function_declaration",
                "method_definition",
                "lexical_declaration",
                "variable_declaration",
                "import_statement",
                "type_alias_declaration",
                "interface_declaration",
            ],
            container_kinds: &["class_declaration", "export_statement"],
        }),
        "go" => Some(OutlineSpec {
            language: tree_sitter_go::LANGUAGE.into(),
            declaration_kinds: &[
                "function_declaration",
                "method_declaration",
                "type_declaration",
                "const_declaration",
                "var_declaration",
                "import_declaration",
            ],
            container_kinds: &[],
        }),
        "java" => Some(OutlineSpec {
            language: tree_sitter_java::LANGUAGE.into(),
            declaration_kinds: &[
                "method_declaration",
                "constructor_declaration",
                "field_declaration",
                "import_declaration",
            ],
            container_kinds: &[
                "class_declaration",
                "interface_declaration",
                "enum_declaration",
            ],
        }),
        "c" | "h" => Some(OutlineSpec {
            language: tree_sitter_c::LANGUAGE.into(),
            declaration_kinds: &[
                "function_definition",
                "declaration",
                "preproc_include",
                "preproc_def",
                "type_definition",
            ],
            container_kinds: &[],
        }),
        "cpp" | "hpp" | "cc" | "cxx" => Some(OutlineSpec {
            language: tree_sitter_cpp::LANGUAGE.into(),
            declaration_kinds: &[
                "function_definition",
                "declaration",
                "preproc_include",
                "preproc_def",
                "type_definition",
                "template_declaration",
            ],
            container_kinds: &["class_specifier", "namespace_definition"],
        }),
        "cs" => Some(OutlineSpec {
            language: tree_sitter_c_sharp::LANGUAGE.into(),
            declaration_kinds: &[
                "method_declaration",
                "constructor_declaration",
                "field_declaration",
                "property_declaration",
                "using_directive",
            ],
            container_kinds: &[
                "class_declaration",
                "interface_declaration",
                "namespace_declaration",
                "enum_declaration",
            ],
        }),
        "rb" => Some(OutlineSpec {
            language: tree_sitter_ruby::LANGUAGE.into(),
            declaration_kinds: &["method", "singleton_method"],
            container_kinds: &["class", "module"],
        }),
        "php" => Some(OutlineSpec {
            language: tree_sitter_php::LANGUAGE_PHP.into(),
            declaration_kinds: &[
                "function_definition",
                "method_declaration",
                "property_declaration",
                "use_declaration",
            ],
            container_kinds: &[
                "class_declaration",
                "interface_declaration",
                "namespace_definition",
            ],
        }),
        "swift" => Some(OutlineSpec {
            language: tree_sitter_swift::LANGUAGE.into(),
            declaration_kinds: &[
                "function_declaration",
                "property_declaration",
                "import_declaration",
                "typealias_declaration",
            ],
            container_kinds: &[
                "class_declaration",
                "struct_declaration",
                "enum_declaration",
                "protocol_declaration",
                "extension_declaration",
            ],
        }),
        "kt" | "kts" => Some(OutlineSpec {
            language: tree_sitter_kotlin_ng::LANGUAGE.into(),
            declaration_kinds: &[
                "function_declaration",
                "property_declaration",
                "import_header",
            ],
            container_kinds: &[
                "class_declaration",
                "object_declaration",
                "interface_declaration",
            ],
        }),
        "scala" | "sc" => Some(OutlineSpec {
            language: tree_sitter_scala::LANGUAGE.into(),
            declaration_kinds: &[
                "function_definition",
                "val_definition",
                "var_definition",
                "import_declaration",
                "type_definition",
            ],
            container_kinds: &["class_definition", "object_definition", "trait_definition"],
        }),
        "sh" | "bash" => Some(OutlineSpec {
            language: tree_sitter_bash::LANGUAGE.into(),
            declaration_kinds: &["function_definition", "variable_assignment"],
            container_kinds: &[],
        }),
        "lua" => Some(OutlineSpec {
            language: tree_sitter_lua::LANGUAGE.into(),
            declaration_kinds: &["function_declaration", "function_definition_statement"],
            container_kinds: &[],
        }),
        "ex" | "exs" => Some(OutlineSpec {
            language: tree_sitter_elixir::LANGUAGE.into(),
            declaration_kinds: &["call"],
            container_kinds: &[],
        }),
        "hs" => Some(OutlineSpec {
            language: tree_sitter_haskell::LANGUAGE.into(),
            declaration_kinds: &[
                "function",
                "signature",
                "type_alias",
                "newtype",
                "adt",
                "class",
                "instance",
                "import",
            ],
            container_kinds: &[],
        }),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Read-file output parsing
// ---------------------------------------------------------------------------

/// Parsed representation of `read_file` tool output.
struct ReadFileOutput {
    /// Source code lines with line-number prefixes stripped.
    source_lines: Vec<String>,
    /// Total line count from the header (for the outline header).
    total_lines: usize,
}

/// Parse the numbered output from `read_file` into header + raw source lines.
fn parse_read_file_output(text: &str) -> Option<ReadFileOutput> {
    let mut lines = text.lines();
    let header = lines.next()?;

    // Validate it looks like a read_file header
    if !header.starts_with('[') || !header.ends_with(']') {
        return None;
    }

    let mut source_lines = Vec::new();
    let mut total_lines = 0;

    for line in lines {
        // Strip line-number prefix: "   42 | code here" → "code here"
        if let Some(pipe_pos) = line.find(" | ") {
            let prefix = &line[..pipe_pos];
            if prefix.trim().chars().all(|c| c.is_ascii_digit()) {
                source_lines.push(line[pipe_pos + 3..].to_string());
                total_lines += 1;
                continue;
            }
        }
        // Lines without the expected prefix — keep as-is
        source_lines.push(line.to_string());
        total_lines += 1;
    }

    if source_lines.is_empty() {
        return None;
    }

    Some(ReadFileOutput {
        source_lines,
        total_lines,
    })
}

// ---------------------------------------------------------------------------
// Outline extraction
// ---------------------------------------------------------------------------

/// Extract a structural outline from a `read_file` tool result.
///
/// Returns `None` if the extension is unsupported, parsing fails, or the
/// outline would not be shorter than the original.
pub fn extract_outline_from_read_file_output(text: &str, ext: &str, path: &str) -> Option<String> {
    let spec = spec_for_extension(ext)?;
    let parsed = parse_read_file_output(text)?;
    let source = parsed.source_lines.join("\n");

    let outline_body = extract_outline_from_source(&source, &spec)?;

    if outline_body.is_empty() {
        return None;
    }

    let result = format!(
        "[Structural outline of {} · {} lines]\n{}",
        path, parsed.total_lines, outline_body
    );

    Some(result)
}

/// Extract outline from raw source code using tree-sitter.
fn extract_outline_from_source(source: &str, spec: &OutlineSpec) -> Option<String> {
    let mut parser = Parser::new();
    parser.set_language(&spec.language).ok()?;
    let tree = parser.parse(source, None)?;
    let root = tree.root_node();

    let lines: Vec<&str> = source.lines().collect();
    let mut output = String::new();

    extract_node_outline(root, spec, &lines, &mut output, 0);

    if output.is_empty() {
        return None;
    }

    Some(output)
}

/// Recursively extract outline from an AST node.
fn extract_node_outline(
    node: tree_sitter::Node,
    spec: &OutlineSpec,
    lines: &[&str],
    output: &mut String,
    indent: usize,
) {
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if !child.is_named() {
            continue;
        }

        let kind = child.kind();
        let start_line = child.start_position().row;
        let end_line = child.end_position().row;
        let span = end_line - start_line + 1;
        let indent_str = "  ".repeat(indent);

        if spec.container_kinds.contains(&kind) {
            // Container: show signature line(s), recurse into children
            let sig = extract_signature(child, lines);
            if span <= 3 {
                // Short container — show entirely
                for line in lines
                    .iter()
                    .take(end_line.min(lines.len() - 1) + 1)
                    .skip(start_line)
                {
                    output.push_str(&indent_str);
                    output.push_str(line);
                    output.push('\n');
                }
            } else {
                output.push_str(&indent_str);
                output.push_str(&sig);
                output.push('\n');
                // Recurse into container children
                extract_node_outline(child, spec, lines, output, indent + 1);
                // Close brace/end for languages that use them
                if let Some(last_line) = lines.get(end_line) {
                    let trimmed = last_line.trim();
                    if trimmed == "}" || trimmed == "end" || trimmed.starts_with('}') {
                        output.push_str(&indent_str);
                        output.push_str(trimmed);
                        output.push('\n');
                    }
                }
            }
        } else if spec.declaration_kinds.contains(&kind) {
            // Declaration: show signature, fold body
            if span <= 3 {
                // Short declaration — show entirely
                for line in lines
                    .iter()
                    .take(end_line.min(lines.len() - 1) + 1)
                    .skip(start_line)
                {
                    output.push_str(&indent_str);
                    output.push_str(line);
                    output.push('\n');
                }
            } else {
                let sig = extract_signature(child, lines);
                output.push_str(&indent_str);
                output.push_str(&sig);
                output.push_str(" ...");
                output.push('\n');
            }
        } else if start_line == end_line {
            // Single-line top-level item (e.g. use/import) — keep as-is
            if indent == 0 {
                if let Some(line) = lines.get(start_line) {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        output.push_str(trimmed);
                        output.push('\n');
                    }
                }
            }
        } else if child.named_child_count() > 0 {
            // Multi-line unknown node (e.g. declaration_list, body) —
            // recurse through it to find declarations inside.
            extract_node_outline(child, spec, lines, output, indent);
        }
    }
}

/// Extract the signature (first line) of an AST node.
fn extract_signature(node: tree_sitter::Node, lines: &[&str]) -> String {
    let start_line = node.start_position().row;
    lines
        .get(start_line)
        .map(|l| l.trim_end().to_string())
        .unwrap_or_default()
}
