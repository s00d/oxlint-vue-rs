//! vue/no-restricted-call-after-await

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use oxc::ast::AstKind;
use oxc::ast::ast::*;
use oxc::semantic::NodeId;
use serde_json::Value;

use crate::rule::{Ctx, LintFile, Rule, RuleOptions};
use crate::utils::reference_tracker::{TraceMap, esm_calls};
use crate::utils::vue_component as vc;

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts
        .options
        .iter()
        .filter_map(|o| {
            let module = o["module"].as_str()?.to_string();
            let path = match &o["path"] {
                Value::String(s) => vec![s.clone()],
                Value::Array(a) => a
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                _ => vec!["default".to_string()],
            };
            Some(Restriction {
                module,
                path,
                message: o["message"].as_str().map(str::to_string),
            })
        })
        .collect();
    Some(Box::new(NoRestrictedCallAfterAwait { options }))
}

struct Restriction {
    module: String,
    path: Vec<String>,
    message: Option<String>,
}

struct NoRestrictedCallAfterAwait {
    options: Vec<Restriction>,
}

/// Lexical `path.join`-style normalization (`.` / `..`).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// `safeRequireResolve`: a directory resolves to its package `main` (or `index.js`); anything else
/// is kept as is.
fn safe_resolve(p: &Path) -> PathBuf {
    let p = normalize(p);
    if p.is_dir() {
        let main = std::fs::read_to_string(p.join("package.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v["main"].as_str().map(str::to_string))
            .unwrap_or_else(|| "index.js".into());
        return normalize(&p.join(main));
    }
    p
}

/// `normalizeModules`: the import sources of this file that refer to `module`.
fn normalize_modules(file: &LintFile, module: &str) -> Vec<String> {
    let module_path = if module.starts_with('.') {
        // ponytail: ESLint resolves against `context.cwd`; we use the process cwd.
        safe_resolve(&std::env::current_dir().unwrap_or_default().join(module))
    } else if Path::new(module).is_absolute() {
        safe_resolve(Path::new(module))
    } else {
        return vec![module.to_string()];
    };
    let dir = file.path.parent().unwrap_or(Path::new(""));
    let Some(program) = file.program else {
        return Vec::new();
    };
    program
        .body
        .iter()
        .filter_map(|s| match s {
            Statement::ImportDeclaration(i) => i.source.value.as_str(),
            _ => None,
        })
        .filter(|src| src.starts_with('.') && safe_resolve(&dir.join(src)) == module_path)
        .map(str::to_string)
        .collect()
}

impl Rule for NoRestrictedCallAfterAwait {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        let file = ctx.file;
        let Some(sem) = file.semantic else { return };
        let mut restricted: HashMap<NodeId, String> = HashMap::new();
        for opt in &self.options {
            for module in normalize_modules(file, &opt.module) {
                let map = TraceMap::call_path(&opt.path);
                let message = opt.message.clone().unwrap_or_else(|| {
                    let mut parts = vec![format!("import(\"{module}\")")];
                    parts.extend(opt.path.iter().cloned());
                    format!(
                        "`{}` is forbidden after an `await` expression.",
                        parts.join(".")
                    )
                });
                for hit in esm_calls(file, &[(module.as_str(), &map)]) {
                    restricted.insert(hit.call.node_id(), message.clone());
                }
            }
        }
        if restricted.is_empty() {
            return;
        }
        let setups: Vec<NodeId> = vc::vue_objects(file)
            .iter()
            .flat_map(|o| vc::setup_functions(o.obj))
            .map(|f| f.node_id())
            .collect();
        let innermost_fn = |id: NodeId| {
            sem.nodes()
                .ancestors(id)
                .find(|n| {
                    matches!(
                        n.kind(),
                        AstKind::Function(_) | AstKind::ArrowFunctionExpression(_)
                    )
                })
                .map(|n| n.id())
        };
        // Await expressions directly inside each setup function (start offsets).
        let mut awaits: HashMap<NodeId, Vec<u32>> = HashMap::new();
        for n in sem.nodes().iter() {
            if let AstKind::AwaitExpression(a) = n.kind()
                && let Some(f) = innermost_fn(n.id()).filter(|f| setups.contains(f))
            {
                awaits.entry(f).or_default().push(a.span.start);
            }
        }
        for n in sem.nodes().iter() {
            let AstKind::CallExpression(call) = n.kind() else {
                continue;
            };
            let Some(msg) = restricted.get(&n.id()) else {
                continue;
            };
            let after_await = innermost_fn(n.id())
                .and_then(|f| awaits.get(&f))
                .is_some_and(|a| a.iter().any(|s| *s < call.span.start));
            if after_await {
                ctx.report_id(call.span, "restricted", msg.clone());
            }
        }
    }
}
