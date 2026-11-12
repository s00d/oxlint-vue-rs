//! `oxlint-vue-rs` CLI: lints `.vue` files (template + script rules oxlint lacks) with the same
//! `.oxlintrc.json`. `--with-oxlint` also runs oxlint on the same paths and merges both outputs.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;

use oxlint_vue_rs::config::{self, Config, RuleSetting};
use oxlint_vue_rs::{
    ConfiguredRule, FileOptions, Severity, apply_fixes, configure, lint_source, rules,
};
use serde_json::{Value, json};

const HELP: &str = "oxlint-vue-rs — eslint-plugin-vue rules for oxlint projects

Usage: oxlint-vue-rs [OPTIONS] [PATH]... [-- OXLINT_ARGS...]

Options:
  -c, --config <FILE>        oxlint config (default: nearest .oxlintrc.json)
  -A, --allow <RULE>         turn a rule off   (vue-rs/<rule>, vue/<rule> or <rule>)
  -W, --warn <RULE>          warn on a rule    (`all` = every implemented rule)
  -D, --deny <RULE>          error on a rule   (`all` = every implemented rule)
      --fix                  apply fixes (up to 10 passes per file)
      --fix-suggestions      also apply the first suggestion of each diagnostic
  -f, --format <FMT>         default | json | unix | github
      --quiet                report errors only
      --deny-warnings        exit 1 on warnings
      --max-warnings <N>     exit 1 when warnings exceed N
      --ignore-pattern <P>   extra ignore glob (repeatable)
      --no-ignore            do not read .gitignore / ignorePatterns
      --hidden               also lint files in hidden (dot) directories
      --no-inline-config     ignore eslint-disable / oxlint-disable comments
      --threads <N>          worker threads (default: all cores)
      --with-oxlint          also run oxlint (from node_modules/.bin or PATH) and merge results;
                             arguments after `--` go to oxlint
      --rules                list rules and implementation status
      --print-config         print the resolved rule configuration
  -h, --help                 this help
  -V, --version              version";

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    cli_rules: Vec<(String, Option<Severity>)>,
    fix: bool,
    fix_suggestions: bool,
    format: String,
    quiet: bool,
    deny_warnings: bool,
    max_warnings: Option<usize>,
    ignore_patterns: Vec<String>,
    no_ignore: bool,
    hidden: bool,
    no_inline_config: bool,
    threads: Option<usize>,
    with_oxlint: bool,
    oxlint_args: Vec<String>,
    list_rules: bool,
    print_config: bool,
    paths: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut a = Args {
        format: "default".into(),
        ..Default::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with('-') => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut val = |name: &str| {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match flag.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("oxlint-vue-rs {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-c" | "--config" => a.config = Some(val("--config")?.into()),
            "-A" | "--allow" => a.cli_rules.push((val("-A")?, None)),
            "-W" | "--warn" => a.cli_rules.push((val("-W")?, Some(Severity::Warn))),
            "-D" | "--deny" => a.cli_rules.push((val("-D")?, Some(Severity::Error))),
            "--fix" => a.fix = true,
            "--fix-suggestions" => {
                a.fix = true;
                a.fix_suggestions = true;
            }
            "-f" | "--format" => a.format = val("--format")?,
            "--quiet" => a.quiet = true,
            "--deny-warnings" => a.deny_warnings = true,
            "--max-warnings" => {
                a.max_warnings = Some(
                    val("--max-warnings")?
                        .parse()
                        .map_err(|_| "--max-warnings: not a number")?,
                )
            }
            "--ignore-pattern" => a.ignore_patterns.push(val("--ignore-pattern")?),
            "--no-ignore" => a.no_ignore = true,
            "--hidden" => a.hidden = true,
            "--no-inline-config" => a.no_inline_config = true,
            "--threads" => {
                a.threads = Some(
                    val("--threads")?
                        .parse()
                        .map_err(|_| "--threads: not a number")?,
                )
            }
            "--with-oxlint" => a.with_oxlint = true,
            "--rules" => a.list_rules = true,
            "--print-config" => a.print_config = true,
            "--" => {
                a.oxlint_args = it.by_ref().collect();
            }
            f if f.starts_with('-') && f.len() > 1 => return Err(format!("unknown option {f}")),
            _ => a.paths.push(arg.into()),
        }
    }
    if a.paths.is_empty() {
        a.paths.push(".".into());
    }
    Ok(a)
}

/// One reported problem (ours or oxlint's), already located.
#[derive(Debug, Clone)]
struct Report {
    file: PathBuf,
    line: u32,
    column: u32,
    end_line: u32,
    end_column: u32,
    offset: u32,
    length: u32,
    severity: Severity,
    code: String,
    message: String,
    fixable: bool,
}

fn rule_name(raw: &str, prefixes: &[String]) -> String {
    match raw.split_once('/') {
        Some((p, n)) if p == "vue" || prefixes.iter().any(|x| x == p) => n.to_string(),
        _ => raw.to_string(),
    }
}

fn effective_rules(
    cfg: &Config,
    file: &Path,
    cli: &[(String, Option<Severity>)],
) -> Vec<(String, RuleSetting)> {
    let mut rules = cfg.rules_for(file);
    for (raw, sev) in cli {
        let name = rule_name(raw, &cfg.prefixes);
        if name == "all" {
            for d in rules::ALL {
                rules.retain(|(n, _)| n != d.name);
                rules.push((
                    d.name.to_string(),
                    RuleSetting {
                        severity: *sev,
                        options: Vec::new(),
                    },
                ));
            }
            continue;
        }
        let opts = rules
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, s)| s.options.clone())
            .unwrap_or_default();
        rules.retain(|(n, _)| *n != name);
        rules.push((
            name,
            RuleSetting {
                severity: *sev,
                options: opts,
            },
        ));
    }
    rules
}

fn instantiate(
    rules: &[(String, RuleSetting)],
    settings: &Value,
    warn_unknown: &Mutex<Vec<String>>,
) -> Vec<ConfiguredRule> {
    let mut out = Vec::new();
    for (name, s) in rules {
        let Some(sev) = s.severity else { continue };
        match configure(name, sev, &s.options, settings) {
            Ok(Some(r)) => out.push(r),
            Ok(None) => {}
            Err(e) => {
                let mut w = warn_unknown.lock().unwrap();
                if !w.contains(&e) {
                    w.push(e);
                }
            }
        }
    }
    out
}

fn collect_files(args: &Args, cfg: &Config) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for p in &args.paths {
        if p.is_file() {
            files.push(p.clone());
            continue;
        }
        let mut wb = ignore::WalkBuilder::new(p);
        wb.hidden(!args.hidden)
            .git_ignore(!args.no_ignore)
            .git_exclude(!args.no_ignore)
            .ignore(!args.no_ignore)
            .require_git(false);
        for e in wb.build().filter_map(Result::ok) {
            let path = e.path();
            if e.file_type().is_some_and(|t| t.is_file())
                && path.extension().is_some_and(|x| x == "vue")
            {
                files.push(path.to_path_buf());
            }
        }
    }
    if !args.no_ignore {
        files.retain(|f| !cfg.is_ignored(f));
    }
    if !args.ignore_patterns.is_empty() {
        files.retain(|f| {
            let s = f.to_string_lossy().replace('\\', "/");
            !args.ignore_patterns.iter().any(|g| {
                config::glob_match(g, &s)
                    || config::glob_match(&format!("{}/**", g.trim_end_matches('/')), &s)
            })
        });
    }
    files.sort();
    files.dedup();
    files
}

fn lint_file(
    path: &Path,
    args: &Args,
    cfg: &Config,
    unknown: &Mutex<Vec<String>>,
) -> Result<Vec<Report>, String> {
    let mut source =
        std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let rules = instantiate(
        &effective_rules(cfg, path, &args.cli_rules),
        &cfg.settings,
        unknown,
    );
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let opts = FileOptions {
        settings: cfg.settings.clone(),
        globals: cfg.globals.clone(),
        template_directives: true,
        prefixes: cfg.prefixes.clone(),
        no_inline_config: args.no_inline_config,
        ..Default::default()
    };
    let mut diags = lint_source(&source, path, &opts, &rules);
    if args.fix {
        let original = source.clone();
        for _ in 0..10 {
            let mut ds = diags.clone();
            if args.fix_suggestions {
                for d in &mut ds {
                    if d.fix.is_none()
                        && let Some(s) = d.suggestions.first()
                    {
                        d.fix = Some(s.fix.clone());
                    }
                }
            }
            match apply_fixes(&source, &ds) {
                Some(next) if next != source => {
                    source = next;
                    diags = lint_source(&source, path, &opts, &rules);
                }
                _ => break,
            }
        }
        if source != original {
            std::fs::write(path, &source).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    let lines = oxlint_vue_rs::LineIndex::new(&source);
    let prefix = cfg
        .prefixes
        .first()
        .cloned()
        .unwrap_or_else(|| config::DEFAULT_PREFIX.into());
    Ok(diags
        .into_iter()
        .map(|d| {
            let [line, column, end_line, end_column] = d.location(&lines, &source);
            Report {
                file: path.to_path_buf(),
                line,
                column,
                end_line,
                end_column,
                offset: d.span.start,
                length: d.span.end - d.span.start,
                severity: d.severity,
                code: format!("{prefix}/{}", d.rule),
                message: d.message,
                fixable: d.fix.is_some(),
            }
        })
        .collect())
}

fn find_oxlint(start: &Path) -> PathBuf {
    let mut d = Some(start);
    while let Some(cur) = d {
        let p = cur.join("node_modules/.bin/oxlint");
        if p.is_file() {
            return p;
        }
        d = cur.parent();
    }
    PathBuf::from("oxlint")
}

/// Run oxlint with `--format json` and convert its diagnostics.
fn run_oxlint(args: &Args, cwd: &Path) -> Result<Vec<Report>, String> {
    let bin = find_oxlint(cwd);
    let mut cmd = Command::new(&bin);
    cmd.arg("--format").arg("json");
    if let Some(c) = &args.config {
        cmd.arg("--config").arg(c);
    }
    if args.fix {
        cmd.arg("--fix");
    }
    if args.quiet {
        cmd.arg("--quiet");
    }
    cmd.args(&args.oxlint_args).args(&args.paths);
    let out = cmd
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))?;
    let text = String::from_utf8_lossy(&out.stdout);
    let v: Value = serde_json::from_str(text.trim()).map_err(|e| {
        format!(
            "oxlint output is not JSON ({e}): {}",
            String::from_utf8_lossy(&out.stderr)
        )
    })?;
    let mut reports = Vec::new();
    for d in v["diagnostics"].as_array().into_iter().flatten() {
        let label = &d["labels"][0]["span"];
        let line = label["line"].as_u64().unwrap_or(0) as u32;
        let column = label["column"].as_u64().unwrap_or(0) as u32;
        reports.push(Report {
            file: PathBuf::from(d["filename"].as_str().unwrap_or("")),
            line,
            column,
            end_line: line,
            end_column: column + label["length"].as_u64().unwrap_or(0) as u32,
            offset: label["offset"].as_u64().unwrap_or(0) as u32,
            length: label["length"].as_u64().unwrap_or(0) as u32,
            severity: if d["severity"] == "error" {
                Severity::Error
            } else {
                Severity::Warn
            },
            code: d["code"].as_str().unwrap_or("oxlint").to_string(),
            message: d["message"].as_str().unwrap_or("").to_string(),
            fixable: false,
        });
    }
    Ok(reports)
}

fn print(reports: &[Report], format: &str, files: usize) {
    let (errors, warnings) = reports.iter().fold((0, 0), |(e, w), r| {
        if r.severity == Severity::Error {
            (e + 1, w)
        } else {
            (e, w + 1)
        }
    });
    match format {
        "json" => {
            let diags: Vec<Value> = reports
                .iter()
                .map(|r| {
                    json!({
                        "message": r.message,
                        "code": r.code,
                        "severity": if r.severity == Severity::Error { "error" } else { "warning" },
                        "filename": r.file.to_string_lossy(),
                        "fixable": r.fixable,
                        "labels": [{"span": {"offset": r.offset, "length": r.length, "line": r.line, "column": r.column, "endLine": r.end_line, "endColumn": r.end_column}}],
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"diagnostics": diags, "number_of_files": files})
                )
                .unwrap()
            );
        }
        "unix" => {
            for r in reports {
                let sev = if r.severity == Severity::Error {
                    "Error"
                } else {
                    "Warning"
                };
                println!(
                    "{}:{}:{}: {} [{sev}/{}]",
                    r.file.display(),
                    r.line,
                    r.column,
                    r.message,
                    r.code
                );
            }
        }
        "github" => {
            for r in reports {
                let sev = if r.severity == Severity::Error {
                    "error"
                } else {
                    "warning"
                };
                let msg = r
                    .message
                    .replace('%', "%25")
                    .replace('\r', "%0D")
                    .replace('\n', "%0A");
                println!(
                    "::{sev} file={},line={},col={},endLine={},endColumn={},title={}::{msg}",
                    r.file.display(),
                    r.line,
                    r.column,
                    r.end_line,
                    r.end_column,
                    r.code
                );
            }
        }
        _ => {
            let mut cur: Option<&Path> = None;
            for r in reports {
                if cur != Some(r.file.as_path()) {
                    if cur.is_some() {
                        println!();
                    }
                    println!("{}", r.file.display());
                    cur = Some(&r.file);
                }
                let sev = if r.severity == Severity::Error {
                    "error  "
                } else {
                    "warning"
                };
                println!(
                    "  {:>4}:{:<4} {sev}  {}  {}",
                    r.line, r.column, r.message, r.code
                );
            }
            if !reports.is_empty() {
                println!();
            }
            println!(
                "Found {warnings} warning{} and {errors} error{} in {files} file{}.",
                s(warnings),
                s(errors),
                s(files)
            );
        }
    }
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cfg_path = args.config.clone().or_else(|| config::find_config(&cwd));
    let cfg = match &cfg_path {
        Some(p) => match Config::load(p) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        },
        None => Config {
            base: cwd.clone(),
            prefixes: vec![config::DEFAULT_PREFIX.into()],
            settings: json!({}),
            ..Default::default()
        },
    };

    if args.list_rules {
        for d in rules::ALL {
            let ok = (d.create)(&oxlint_vue_rs::RuleOptions {
                options: &[],
                settings: &Value::Null,
            })
            .is_some();
            println!(
                "{}/{:<50} {}",
                cfg.prefixes[0],
                d.name,
                if ok { "implemented" } else { "not implemented" }
            );
        }
        return ExitCode::SUCCESS;
    }
    if args.print_config {
        let rules = effective_rules(&cfg, &cwd.join("x.vue"), &args.cli_rules);
        let obj: serde_json::Map<String, Value> = rules
            .into_iter()
            .map(|(n, s)| {
                let sev = match s.severity {
                    None => "off",
                    Some(Severity::Warn) => "warn",
                    Some(Severity::Error) => "error",
                };
                let mut v = vec![Value::from(sev)];
                v.extend(s.options);
                (format!("{}/{n}", cfg.prefixes[0]), Value::Array(v))
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({"config": cfg_path, "prefixes": cfg.prefixes, "rules": obj})
            )
            .unwrap()
        );
        return ExitCode::SUCCESS;
    }

    let files = collect_files(&args, &cfg);
    let unknown = Mutex::new(Vec::new());
    let threads = args
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()))
        .max(1);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Mutex<Vec<Report>> = Mutex::new(Vec::new());
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    std::thread::scope(|sc| {
        for _ in 0..threads.min(files.len().max(1)) {
            sc.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(f) = files.get(i) else { break };
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        lint_file(f, &args, &cfg, &unknown)
                    })) {
                        Ok(Ok(r)) => results.lock().unwrap().extend(r),
                        Ok(Err(e)) => failures.lock().unwrap().push(e),
                        Err(_) => failures.lock().unwrap().push(format!(
                            "{}: internal error (panic) — please report",
                            f.display()
                        )),
                    }
                }
            });
        }
    });
    let mut reports = results.into_inner().unwrap();
    let mut file_count = files.len();
    if args.with_oxlint {
        match run_oxlint(&args, &cwd) {
            Ok(r) => {
                let extra: std::collections::HashSet<_> =
                    r.iter().map(|x| x.file.clone()).collect();
                file_count = file_count.max(extra.len());
                reports.extend(r);
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if args.quiet {
        reports.retain(|r| r.severity == Severity::Error);
    }
    reports.sort_by(|a, b| (&a.file, a.line, a.column).cmp(&(&b.file, b.line, b.column)));
    for e in unknown.into_inner().unwrap() {
        eprintln!("warning: {e}");
    }
    let fails = failures.into_inner().unwrap();
    for f in &fails {
        eprintln!("error: {f}");
    }
    print(&reports, &args.format, file_count);
    let errors = reports
        .iter()
        .filter(|r| r.severity == Severity::Error)
        .count();
    let warnings = reports.len() - errors;
    let too_many =
        args.max_warnings.is_some_and(|m| warnings > m) || (args.deny_warnings && warnings > 0);
    if errors > 0 || too_many || !fails.is_empty() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
