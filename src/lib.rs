//! Static FastMCP registration evidence for the public target scanner.

use lekalo_adapter_sdk::ids::sha256_hex;
use lekalo_adapter_sdk::json::{canonical, Value};
use ruff_python_ast::{Expr, ExprCall, PySourceType, Stmt};
use ruff_source_file::LineIndex;
use ruff_text_size::Ranged;
use std::collections::{BTreeMap, BTreeSet};

pub struct Source<'a> {
    pub path: &'a str,
    pub module: &'a str,
    pub text: &'a str,
    pub digest: &'a str,
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn object<const N: usize>(pairs: [(&str, Value); N]) -> Value {
    Value::Object(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn tool_name<'a>(call: &'a ExprCall, default: &'a str) -> Option<&'a str> {
    if !call.arguments.args.is_empty() {
        return None;
    }
    let name = match &*call.arguments.keywords {
        [] => default,
        [kw] if kw.arg.as_ref()?.as_str() == "name" => {
            let Expr::StringLiteral(literal) = &kw.value else {
                return None;
            };
            literal.value.to_str()
        }
        _ => return None,
    };
    (!name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b)))
    .then_some(name)
}

fn module_facts(
    source: &Source<'_>,
    remote_fastmcp: bool,
    remote_mcp: bool,
    native: &impl Fn(&str, &str, &str) -> Option<String>,
) -> Option<Vec<Value>> {
    let parsed = ruff_python_parser::parse_unchecked_source(source.text, PySourceType::Python);
    if !parsed.errors().is_empty() {
        return None;
    }
    let body = &parsed.syntax().body;
    let mut constructors = BTreeSet::new();
    let mut servers = BTreeMap::new();
    let mut names = BTreeSet::new();
    let mut tool_slots = BTreeSet::new();
    let mut facts = Vec::new();
    let lines = LineIndex::from_source_text(source.text);
    for statement in body {
        match statement {
            Stmt::ImportFrom(row) if row.level == 0 => {
                let module = row.module.as_ref()?.as_str();
                for alias in &row.names {
                    let from = alias.name.as_str();
                    let name = alias.asname.as_ref().map_or(from, |n| n.as_str());
                    if from == "*" || !names.insert(name) {
                        return None;
                    }
                    if from == "FastMCP" {
                        match module {
                            "fastmcp" if remote_fastmcp => {
                                constructors.insert(name);
                            }
                            "mcp.server.fastmcp" if remote_mcp => {
                                constructors.insert(name);
                            }
                            _ => return None,
                        }
                    }
                }
            }
            Stmt::Import(row) => {
                for alias in &row.names {
                    let module = alias.name.as_str();
                    let name = alias.asname.as_ref().map_or_else(
                        || module.split('.').next().unwrap_or(module),
                        |n| n.as_str(),
                    );
                    if !names.insert(name) {
                        return None;
                    }
                }
            }
            Stmt::Assign(row) => {
                let [Expr::Name(target)] = row.targets.as_slice() else {
                    return None;
                };
                if !names.insert(target.id.as_str()) {
                    return None;
                }
                match row.value.as_ref() {
                    Expr::StringLiteral(_)
                    | Expr::NumberLiteral(_)
                    | Expr::BooleanLiteral(_)
                    | Expr::NoneLiteral(_) => (),
                    Expr::Call(call) => {
                        let Expr::Name(callee) = call.func.as_ref() else {
                            return None;
                        };
                        if !constructors.contains(callee.id.as_str())
                            || !call.arguments.keywords.is_empty()
                            || call.arguments.args.len() > 1
                            || call
                                .arguments
                                .args
                                .iter()
                                .any(|a| !matches!(a, Expr::StringLiteral(_)))
                        {
                            return None;
                        }
                        let id = native(
                            source.path,
                            &format!("{}.{}", source.module, target.id),
                            "variable",
                        )?;
                        servers.insert(target.id.as_str(), id);
                    }
                    _ => return None,
                }
            }
            Stmt::FunctionDef(row) => {
                if !names.insert(row.name.as_str()) {
                    return None;
                }
                let plain_type = |ty: &Expr| matches!(ty, Expr::Name(_));
                if row.type_params.is_some()
                    || row.parameters.vararg.is_some()
                    || row.parameters.kwarg.is_some()
                    || row.returns.as_deref().is_some_and(|ty| !plain_type(ty))
                    || row.parameters.iter_non_variadic_params().any(|p| {
                        p.default.is_some()
                            || p.parameter
                                .annotation
                                .as_deref()
                                .is_some_and(|ty| !plain_type(ty))
                    })
                {
                    return None;
                }
                let decorator = match row.decorator_list.as_slice() {
                    [] => continue,
                    [decorator] => decorator,
                    _ => return None,
                };
                let Expr::Call(call) = &decorator.expression else {
                    return None;
                };
                let Expr::Attribute(method) = call.func.as_ref() else {
                    return None;
                };
                let Expr::Name(receiver) = method.value.as_ref() else {
                    return None;
                };
                if method.attr.as_str() != "tool" {
                    return None;
                }
                let server = servers.get(receiver.id.as_str())?;
                let name = tool_name(call, row.name.as_str())?;
                if !tool_slots.insert((receiver.id.as_str(), name)) {
                    return None;
                }
                let handler = native(
                    source.path,
                    &format!("{}.{}", source.module, row.name),
                    "function",
                )?;
                let slot = canonical(&Value::Array(vec![text(server), text(name)]));
                let line = lines.line_index(decorator.start()).get() as i64;
                facts.push(object([
                    ("subject", Value::Str(handler)),
                    ("key", text(&sha256_hex(&slot))),
                    ("references", object([("server", text(server))])),
                    (
                        "sources",
                        Value::Array(vec![object([
                            ("path", text(source.path)),
                            ("source_digest", text(source.digest)),
                        ])]),
                    ),
                    (
                        "payload",
                        object([
                            ("name", text(name)),
                            ("registration_path", text(source.path)),
                            ("registration_line", Value::Int(line)),
                        ]),
                    ),
                ]));
            }
            Stmt::Expr(row) if matches!(row.value.as_ref(), Expr::StringLiteral(_)) => {}
            _ => return None,
        }
    }
    Some(facts)
}

/// The runtime provides native anchors. The framework package supplies
/// the extraction policy, coverage and payload schema.
pub fn evidence_group(
    sources: &[Source<'_>],
    pins: &Value,
    partial: bool,
    native: impl Fn(&str, &str, &str) -> Option<String>,
) -> Result<Value, &'static str> {
    let shadows = |package: &str| {
        sources.iter().any(|s| {
            s.module == package
                || s.module
                    .strip_prefix(package)
                    .is_some_and(|m| m.starts_with('.'))
        })
    };
    let remote_fastmcp = !shadows("fastmcp");
    let remote_mcp = !shadows("mcp");
    let mut partial = partial;
    let mut facts = Vec::new();
    for source in sources.iter().filter(|s| s.path.ends_with(".py")) {
        match module_facts(source, remote_fastmcp, remote_mcp, &native) {
            Some(rows) => facts.extend(rows),
            None => partial = true,
        }
    }
    Ok(object([
        ("family", text("fastmcp-tools")),
        (
            "package_digest",
            pins.get("package_digest").cloned().ok_or("evidence-pins")?,
        ),
        (
            "schema_digest",
            pins.get("schema_digest").cloned().ok_or("evidence-pins")?,
        ),
        (
            "coverage",
            text(if partial { "partial" } else { "complete" }),
        ),
        ("facts", Value::Array(facts)),
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_tools_use_native_anchors_and_report_dynamic_scope() {
        let source = "from mcp.server.fastmcp import FastMCP as MCP\nserver = MCP('demo')\n@server.tool(name='find')\ndef lookup(query: str):\n    return query\n";
        let files = |text| {
            [Source {
                path: "src/server.py",
                module: "server",
                text,
                digest: "sha256:source",
            }]
        };
        let lookup = |path: &str, name: &str, kind: &str| {
            (path == "src/server.py").then_some(())?;
            match (name, kind) {
                ("server.server", "variable") => Some("native-server".to_owned()),
                ("server.lookup", "function") => Some("native-handler".to_owned()),
                _ => None,
            }
        };
        let pins = object([
            ("package_digest", text("sha256:package")),
            ("schema_digest", text("sha256:schema")),
        ]);
        let group = evidence_group(&files(source), &pins, false, lookup).unwrap();
        assert_eq!(
            group.get("coverage").and_then(Value::as_str),
            Some("complete")
        );
        let facts = group.get("facts").and_then(Value::as_array).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(
            facts[0].get("subject").and_then(Value::as_str),
            Some("native-handler")
        );
        assert_eq!(
            facts[0]
                .get("references")
                .and_then(|v| v.get("server"))
                .and_then(Value::as_str),
            Some("native-server")
        );
        let dynamic = format!("{source}\ninstall(server)\n");
        let group = evidence_group(&files(&dynamic), &pins, false, lookup).unwrap();
        assert_eq!(
            group.get("coverage").and_then(Value::as_str),
            Some("partial")
        );
        assert!(group
            .get("facts")
            .and_then(Value::as_array)
            .unwrap()
            .is_empty());
    }
}
