//! `oxlint-vue-rs` CLI: lints `.vue` files (template + script rules oxlint lacks) with the same
//! `.oxlintrc.json`. When oxlint is installed (node_modules/.bin or PATH) it also runs oxlint on the same
//! paths and merges both outputs, so one command replaces `oxlint` (`--no-oxlint` to skip it).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::sync::Mutex;

use oxlint_vue_rs::config::{self, Config, RuleSetting};
use oxlint_vue_rs::{
    ConfiguredRule, FileOptions, LineIndex, PARSING_ERROR, Severity, apply_fixes, configure,
    lint_source, rules,
};
use serde_json::{Value, json};

/// Print a line to stdout; a closed pipe (`| head`) is not an error.
fn say(s: &str) {
    let _ = writeln!(std::io::stdout().lock(), "{s}");
}

const HELP: &str = "oxlint-vue-rs — eslint-plugin-vue rules for oxlint projects

Usage: oxlint-vue-rs [OPTIONS] [PATH]... [-- OXLINT_ARGS...]

Options:
  -c, --config <FILE>        oxlint config for every file (default: the nearest .oxlintrc.json
                             of each file, like oxlint's nested configs)
      --disable-nested-config
                             only use the config found from the current directory upwards
  -A, --allow <RULE>         turn a rule off   (vue-rs/<rule>, vue/<rule> or <rule>; other
                             names go to oxlint when it runs)
  -W, --warn <RULE>          warn on a rule    (`all` = every implemented rule)
  -D, --deny <RULE>          error on a rule   (`all` = every implemented rule)
      --fix                  apply fixes (until stable, up to 50 passes per file)
      --fix-suggestions      also apply the first suggestion of each diagnostic
  -f, --format <FMT>         default | json | unix | github
      --quiet                report errors only
      --deny-warnings        exit 1 on warnings
      --max-warnings <N>     exit 1 when warnings exceed N
      --ignore-pattern <P>   extra gitignore-style pattern, relative to the current directory
                             (repeatable; also passed to oxlint)
      --no-ignore            do not read .gitignore / ignorePatterns (also passed to oxlint)
      --hidden               also lint files in hidden (dot) directories
      --no-inline-config     ignore eslint-disable / oxlint-disable comments
      --threads <N>          worker threads (default: all cores)
      --no-oxlint            do not run oxlint (by default it runs when found in node_modules/.bin
                             or PATH, with the same paths/config, and the outputs are merged)
      --with-oxlint          require oxlint (error when it is not found); arguments after `--` go to oxlint
      --rules                list rules and implementation status
      --print-config         print the resolved rule configuration
  -h, --help                 this help
  -V, --version              version";

#[derive(Default)]
struct Args {
    config: Option<PathBuf>,
    disable_nested_config: bool,
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
    /// `None` = auto (run oxlint when found), `Some(true)` = required, `Some(false)` = never.
    with_oxlint: Option<bool>,
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
                say(HELP);
                std::process::exit(0);
            }
            "-V" | "--version" => {
                say(&format!("oxlint-vue-rs {}", env!("CARGO_PKG_VERSION")));
                std::process::exit(0);
            }
            "-c" | "--config" => a.config = Some(val("--config")?.into()),
            "--disable-nested-config" => a.disable_nested_config = true,
            "-A" | "--allow" => a.cli_rules.push((val("-A")?, None)),
            "-W" | "--warn" => a.cli_rules.push((val("-W")?, Some(Severity::Warn))),
            "-D" | "--deny" => a.cli_rules.push((val("-D")?, Some(Severity::Error))),
            "--fix" => a.fix = true,
            "--fix-suggestions" => {
                a.fix = true;
                a.fix_suggestions = true;
            }
            "-f" | "--format" => {
                a.format = val("--format")?;
                if !matches!(a.format.as_str(), "default" | "json" | "unix" | "github") {
                    return Err(format!("--format: unknown format {:?}", a.format));
                }
            }
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
            "--with-oxlint" => a.with_oxlint = Some(true),
            "--no-oxlint" => a.with_oxlint = Some(false),
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
    /// oxlint's `help` / `url` (passed through in `--format json`).
    help: Option<String>,
    url: Option<String>,
}

fn rule_name(raw: &str, prefixes: &[String]) -> String {
    match raw.split_once('/') {
        Some((p, n)) if p == "vue" || prefixes.iter().any(|x| x == p) => n.to_string(),
        _ => raw.to_string(),
    }
}

fn is_known(name: &str) -> bool {
    rules::ALL.iter().any(|d| d.name == name)
}

/// Whether a `-A/-W/-D` name is for us: our prefix, `all`, or a name in our registry (bare or `vue/`).
/// Anything else is an oxlint rule or category.
fn is_ours(raw: &str, prefixes: &[String]) -> bool {
    match raw.split_once('/') {
        Some((p, _)) if p == config::DEFAULT_PREFIX || prefixes.iter().any(|x| x == p) => true,
        Some(("vue", n)) => is_known(n),
        Some(_) => false,
        None => raw == "all" || is_known(raw),
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
    prefix: &str,
    warn_unknown: &Mutex<Vec<String>>,
) -> Vec<ConfiguredRule> {
    let mut out = Vec::new();
    for (name, s) in rules {
        let Some(sev) = s.severity else { continue };
        match configure(name, sev, &s.options, settings) {
            Ok(Some(r)) => out.push(r),
            Ok(None) => {}
            Err(_) => {
                let e = format!("unknown rule {prefix}/{name}");
                let mut w = warn_unknown.lock().unwrap();
                if !w.contains(&e) {
                    w.push(e);
                }
            }
        }
    }
    out
}

/// A file to lint: path as shown (relative to cwd when under it), absolute path, config index.
struct Target {
    shown: PathBuf,
    abs: PathBuf,
    cfg: usize,
}

/// Configs by file: `-c` (one for all), or the nearest config of each file (oxlint nested configs;
/// a nested config replaces its parents, it is not merged with them).
struct Configs {
    cwd: PathBuf,
    fixed: Option<usize>,
    list: Vec<Config>,
    by_path: HashMap<PathBuf, usize>,
    by_dir: HashMap<PathBuf, usize>,
    /// An `oxlint.config.*` found where no JSON config was (we cannot read it).
    js_config: Option<PathBuf>,
}

impl Configs {
    fn new(args: &Args, cwd: &Path) -> Result<Configs, String> {
        let mut c = Configs {
            cwd: cwd.to_path_buf(),
            fixed: None,
            list: Vec::new(),
            by_path: HashMap::new(),
            by_dir: HashMap::new(),
            js_config: None,
        };
        if let Some(p) = &args.config {
            c.fixed = Some(c.load(Some(config::absolute(p)))?);
        } else if args.disable_nested_config {
            c.fixed = Some(c.for_dir(cwd)?);
        }
        Ok(c)
    }

    fn load(&mut self, path: Option<PathBuf>) -> Result<usize, String> {
        let key = path.clone().unwrap_or_default();
        if let Some(&i) = self.by_path.get(&key) {
            return Ok(i);
        }
        let cfg = match &path {
            Some(p) => Config::load(p)?,
            None => Config::empty(&self.cwd),
        };
        for w in &cfg.warnings {
            eprintln!("warning: {w}");
        }
        self.list.push(cfg);
        self.by_path.insert(key, self.list.len() - 1);
        Ok(self.list.len() - 1)
    }

    /// Config for files in `dir` (absolute).
    fn for_dir(&mut self, dir: &Path) -> Result<usize, String> {
        if let Some(i) = self.fixed {
            return Ok(i);
        }
        if let Some(&i) = self.by_dir.get(dir) {
            return Ok(i);
        }
        let found = config::find_config(dir);
        if found.is_none() && self.js_config.is_none() {
            self.js_config = config::find_js_config(dir);
        }
        let i = self.load(found)?;
        self.by_dir.insert(dir.to_path_buf(), i);
        Ok(i)
    }

    /// The config `--print-config` shows (the one for the current directory).
    fn for_cwd(&mut self) -> Result<usize, String> {
        let cwd = self.cwd.clone();
        self.for_dir(&cwd)
    }
}

/// Path as reported: relative to `cwd` when under it (as oxlint prints it), else absolute.
fn shown(abs: &Path, cwd: &Path) -> PathBuf {
    abs.strip_prefix(cwd)
        .map_or_else(|_| abs.to_path_buf(), Path::to_path_buf)
}

fn collect_files(args: &Args, cfgs: &mut Configs) -> Result<Vec<Target>, String> {
    let cwd = cfgs.cwd.clone();
    let mut found = Vec::new();
    for p in &args.paths {
        let abs = config::absolute(p);
        if abs.is_file() {
            found.push(abs);
            continue;
        }
        if !abs.is_dir() {
            return Err(format!("{}: no such file or directory", p.display()));
        }
        let mut wb = ignore::WalkBuilder::new(&abs);
        wb.hidden(!args.hidden)
            .git_ignore(!args.no_ignore)
            .git_exclude(!args.no_ignore)
            .ignore(!args.no_ignore)
            .require_git(false);
        for e in wb.build().filter_map(Result::ok) {
            if e.file_type().is_some_and(|t| t.is_file())
                && e.path().extension().is_some_and(|x| x == "vue")
            {
                found.push(e.into_path());
            }
        }
    }
    let cli_ignore = config::gitignore(&cwd, &args.ignore_patterns)
        .map_err(|e| format!("--ignore-pattern: {e}"))?;
    // One entry per real file, however it was spelled (`.`, `x.vue`, `$PWD/x.vue`, a symlink).
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for abs in found {
        if !seen.insert(std::fs::canonicalize(&abs).unwrap_or_else(|_| abs.clone())) {
            continue;
        }
        let dir = abs.parent().unwrap_or(&abs).to_path_buf();
        let cfg = cfgs.for_dir(&dir)?;
        if !args.no_ignore && cfgs.list[cfg].is_ignored(&abs) {
            continue;
        }
        if let Ok(rel) = abs.strip_prefix(&cwd)
            && cli_ignore
                .matched_path_or_any_parents(rel, false)
                .is_ignore()
        {
            continue;
        }
        files.push(Target {
            shown: shown(&abs, &cwd),
            abs,
            cfg,
        });
    }
    files.sort_by(|a, b| a.shown.cmp(&b.shown));
    Ok(files)
}

/// Write `text` over `path` atomically (temp file + rename, keeping permissions), unless the file
/// changed since it was read as `read`.
fn write_fixed(path: &Path, read: &str, text: &str) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    if std::fs::read_to_string(path).map_err(err)? != read {
        return Err(format!(
            "{}: changed while linting, fixes not written",
            path.display()
        ));
    }
    // A file we may not write keeps its content (ESLint fails with EACCES; the rename would succeed).
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|e| format!("{}: {e}, fixes not written", path.display()))?;
    let perms = std::fs::metadata(path).map_err(err)?.permissions();
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = path.with_file_name(format!(".{name}.oxlint-vue-rs-{}.tmp", std::process::id()));
    let res = std::fs::write(&tmp, text)
        .and_then(|()| std::fs::set_permissions(&tmp, perms))
        .and_then(|()| std::fs::rename(&tmp, path));
    if res.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    res.map_err(err)
}

const MAX_FIX_PASSES: usize = 50;

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

fn lint_file(
    t: &Target,
    args: &Args,
    cfg: &Config,
    unknown: &Mutex<Vec<String>>,
) -> Result<Vec<Report>, String> {
    let path = t.abs.as_path();
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", t.shown.display()))?;
    // ESLint lints the text without a BOM and puts it back on fix output.
    let bom = if text.starts_with('\u{feff}') {
        "\u{feff}"
    } else {
        ""
    };
    let mut source = text[bom.len()..].to_string();
    let prefix = cfg
        .prefixes
        .first()
        .cloned()
        .unwrap_or_else(|| config::DEFAULT_PREFIX.into());
    let rules = instantiate(
        &effective_rules(cfg, path, &args.cli_rules),
        &cfg.settings,
        &prefix,
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
        // ESLint stops after 10 passes; reorderings (`attributes-order`, `order-in-components`: one
        // move per pass) combined with layout rules can need ~30 on real files. Stop on a cycle.
        let mut seen = vec![hash(&source)];
        for _ in 0..MAX_FIX_PASSES {
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
                    let h = hash(&source);
                    if seen.contains(&h) {
                        break;
                    }
                    seen.push(h);
                }
                _ => break,
            }
        }
        if source != original {
            write_fixed(path, &text, &format!("{bom}{source}"))?;
        }
    }
    let lines = LineIndex::for_source(&source, path.extension().is_some_and(|e| e == "vue"));
    Ok(diags
        .into_iter()
        .map(|d| {
            let [line, column, end_line, end_column] = d.location(&lines, &source);
            Report {
                file: t.shown.clone(),
                line,
                column,
                end_line,
                end_column,
                offset: d.span.start,
                length: d.span.end - d.span.start,
                severity: d.severity,
                code: if d.rule == PARSING_ERROR {
                    String::new()
                } else {
                    format!("{prefix}/{}", d.rule)
                },
                message: d.message,
                fixable: d.fix.is_some(),
                help: None,
                url: None,
            }
        })
        .collect())
}

/// oxlint from the nearest `node_modules/.bin`, else from PATH.
fn find_oxlint(start: &Path) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["oxlint.cmd", "oxlint.exe", "oxlint"]
    } else {
        &["oxlint"]
    };
    let mut d = Some(start);
    while let Some(cur) = d {
        for n in names {
            let p = cur.join("node_modules/.bin").join(n);
            if p.is_file() {
                return Some(p);
            }
        }
        d = cur.parent();
    }
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

/// oxlint's diagnostics, its `number_of_files` and whether it failed (its exit status).
struct OxlintRun {
    reports: Vec<Report>,
    files: usize,
    failed: bool,
}

/// Run oxlint with `--format json` and convert its diagnostics.
fn run_oxlint(
    args: &Args,
    forward: &[(String, Option<Severity>)],
    bin: &Path,
    cwd: &Path,
) -> Result<OxlintRun, String> {
    let mut cmd = Command::new(bin);
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
    if args.no_ignore {
        cmd.arg("--no-ignore");
    }
    for p in &args.ignore_patterns {
        cmd.arg(format!("--ignore-pattern={p}"));
    }
    for (name, sev) in forward {
        let flag = match sev {
            None => "-A",
            Some(Severity::Warn) => "-W",
            Some(Severity::Error) => "-D",
        };
        cmd.arg(flag).arg(name);
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
    // Source + line table per file, for end positions of multi-line spans.
    let mut sources: HashMap<PathBuf, Option<(String, LineIndex)>> = HashMap::new();
    for d in v["diagnostics"].as_array().into_iter().flatten() {
        let label = &d["labels"][0]["span"];
        let line = label["line"].as_u64().unwrap_or(0) as u32;
        let column = label["column"].as_u64().unwrap_or(0) as u32;
        let offset = label["offset"].as_u64().unwrap_or(0) as u32;
        let length = label["length"].as_u64().unwrap_or(0) as u32;
        let abs = config::absolute(&cwd.join(d["filename"].as_str().unwrap_or("")));
        let src = sources.entry(abs.clone()).or_insert_with(|| {
            let s = std::fs::read_to_string(&abs).ok()?;
            let l = LineIndex::for_source(&s, abs.extension().is_some_and(|e| e == "vue"));
            Some((s, l))
        });
        let (end_line, end_column) = match src {
            Some((s, l)) if (offset + length) as usize <= s.len() => l.line_col(s, offset + length),
            _ => (line, column + length),
        };
        let text = |k: &str| d[k].as_str().filter(|s| !s.is_empty()).map(String::from);
        reports.push(Report {
            file: shown(&abs, cwd),
            line,
            column,
            end_line,
            end_column,
            offset,
            length,
            severity: if d["severity"] == "error" {
                Severity::Error
            } else {
                Severity::Warn
            },
            code: d["code"].as_str().unwrap_or("oxlint").to_string(),
            message: d["message"].as_str().unwrap_or("").to_string(),
            fixable: false,
            help: text("help"),
            url: text("url"),
        });
    }
    Ok(OxlintRun {
        reports,
        files: v["number_of_files"].as_u64().unwrap_or(0) as usize,
        failed: !out.status.success(),
    })
}

fn print(reports: &[Report], format: &str, files: usize) {
    // Written in one go at the end; a closed stdout (`| head`) is ignored.
    let mut o = String::new();
    macro_rules! p {
        ($($t:tt)*) => {
            let _ = writeln!($($t)*);
        };
    }
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
                    let mut v = json!({
                        "message": r.message,
                        "code": r.code,
                        "severity": if r.severity == Severity::Error { "error" } else { "warning" },
                        "filename": r.file.to_string_lossy(),
                        "fixable": r.fixable,
                        "labels": [{"span": {"offset": r.offset, "length": r.length, "line": r.line, "column": r.column, "endLine": r.end_line, "endColumn": r.end_column}}],
                    });
                    for (k, x) in [("help", &r.help), ("url", &r.url)] {
                        if let Some(x) = x {
                            v[k] = x.as_str().into();
                        }
                    }
                    v
                })
                .collect();
            p!(
                o,
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
                let code = if r.code.is_empty() {
                    String::new()
                } else {
                    format!("/{}", r.code)
                };
                p!(
                    o,
                    "{}:{}:{}: {} [{sev}{code}]",
                    r.file.display(),
                    r.line,
                    r.column,
                    r.message,
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
                let msg = gh_escape(&r.message, false);
                p!(
                    o,
                    "::{sev} file={},line={},col={},endLine={},endColumn={},title={}::{msg}",
                    gh_escape(&r.file.to_string_lossy(), true),
                    r.line,
                    r.column,
                    r.end_line,
                    r.end_column,
                    gh_escape(&r.code, true)
                );
            }
        }
        _ => {
            let mut cur: Option<&Path> = None;
            for r in reports {
                if cur != Some(r.file.as_path()) {
                    if cur.is_some() {
                        p!(o);
                    }
                    p!(o, "{}", r.file.display());
                    cur = Some(&r.file);
                }
                let sev = if r.severity == Severity::Error {
                    "error  "
                } else {
                    "warning"
                };
                let line = format!(
                    "  {:>4}:{:<4} {sev}  {}  {}",
                    r.line, r.column, r.message, r.code
                );
                p!(o, "{}", line.trim_end());
            }
            if !reports.is_empty() {
                p!(o);
            }
            p!(
                o,
                "Found {warnings} warning{} and {errors} error{} in {files} file{}.",
                s(warnings),
                s(errors),
                s(files)
            );
        }
    }
    let _ = std::io::stdout().lock().write_all(o.as_bytes());
}

/// GitHub workflow command escaping (`property` values also escape `:` and `,`).
fn gh_escape(s: &str, property: bool) -> String {
    let s = s
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    if property {
        s.replace(':', "%3A").replace(',', "%2C")
    } else {
        s
    }
}

fn s(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Worker thread stack (deeply nested templates / expressions).
const WORKER_STACK: usize = 256 << 20;

fn main() -> ExitCode {
    let mut args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            return ExitCode::from(2);
        }
    };
    let cwd = config::absolute(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut cfgs = match Configs::new(&args, &cwd) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let cwd_cfg = match cfgs.for_cwd() {
        Ok(i) => i,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    let cfg = &cfgs.list[cwd_cfg];
    let cfg_path = cfgs
        .by_path
        .iter()
        .find(|&(p, &i)| i == cwd_cfg && !p.as_os_str().is_empty())
        .map(|(p, _)| p.clone());

    if args.list_rules {
        for d in rules::ALL {
            let ok = (d.create)(&oxlint_vue_rs::RuleOptions {
                options: &[],
                settings: &Value::Null,
            })
            .is_some();
            say(&format!(
                "{}/{:<50} {}",
                cfg.prefixes[0],
                d.name,
                if ok { "implemented" } else { "not implemented" }
            ));
        }
        return ExitCode::SUCCESS;
    }
    if args.print_config {
        let rules = effective_rules(cfg, &cwd.join("x.vue"), &args.cli_rules);
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
        say(&serde_json::to_string_pretty(
            &json!({"config": cfg_path, "prefixes": cfg.prefixes, "rules": obj}),
        )
        .unwrap());
        return ExitCode::SUCCESS;
    }

    let prefixes = cfg.prefixes.clone();
    let files = match collect_files(&args, &mut cfgs) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    // oxlint.config.ts & co. need a JS runtime; we only read JSON configs. Without any rule of ours
    // enabled on the command line the run would silently check nothing, so that is a config error (2).
    if let Some(js) = &cfgs.js_config {
        eprintln!(
            "warning: {}: oxlint-vue-rs reads only JSON configs (.oxlintrc.json / .oxlintrc.jsonc / \
             oxlintrc.json), not oxlint.config.*; no JSON config was found, so no vue-rs rules are \
             enabled. Put them in a .oxlintrc.json (oxlint reads it too) or pass -c <config.json>.",
            js.display()
        );
        if !args
            .cli_rules
            .iter()
            .any(|(r, s)| s.is_some() && is_ours(r, &prefixes))
        {
            return ExitCode::from(2);
        }
    }
    let oxlint_bin = match args.with_oxlint {
        Some(false) => None,
        auto_or_required => match find_oxlint(&cwd) {
            Some(b) => Some(b),
            None if auto_or_required == Some(true) => {
                eprintln!(
                    "error: --with-oxlint: oxlint not found in node_modules/.bin or PATH (npm i -D oxlint)"
                );
                return ExitCode::from(2);
            }
            None => None,
        },
    };
    // `-A/-W/-D` names that are not ours go to oxlint when it runs.
    let forward = if oxlint_bin.is_some() {
        let (ours, theirs) = std::mem::take(&mut args.cli_rules)
            .into_iter()
            .partition(|(r, _)| is_ours(r, &prefixes));
        args.cli_rules = ours;
        theirs
    } else {
        Vec::new()
    };
    let args = args;
    if files.is_empty() && oxlint_bin.is_none() {
        say("No files found to lint. Please check your paths and ignore patterns.");
        return ExitCode::from(1);
    }
    let cfgs = &cfgs.list;
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
            // Deeply nested input recurses deeply (parser, oxc, rules); a stack overflow would abort
            // the whole run, which `catch_unwind` cannot catch.
            let worker = std::thread::Builder::new().stack_size(WORKER_STACK);
            let spawned = worker.spawn_scoped(sc, || {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(f) = files.get(i) else { break };
                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        lint_file(f, &args, &cfgs[f.cfg], &unknown)
                    })) {
                        Ok(Ok(r)) => results.lock().unwrap().extend(r),
                        Ok(Err(e)) => failures.lock().unwrap().push(e),
                        Err(_) => failures.lock().unwrap().push(format!(
                            "{}: internal error (panic) — please report",
                            f.shown.display()
                        )),
                    }
                }
            });
            if let Err(e) = spawned {
                failures
                    .lock()
                    .unwrap()
                    .push(format!("cannot start a worker thread: {e}"));
            }
        }
    });
    let mut reports = results.into_inner().unwrap();
    let mut file_count = files.len();
    let mut oxlint_failed = false;
    if let Some(bin) = &oxlint_bin {
        match run_oxlint(&args, &forward, bin, &cwd) {
            Ok(r) => {
                // oxlint also lints the `.vue` files we did.
                file_count = file_count.max(r.files);
                oxlint_failed = r.failed;
                reports.extend(r.reports);
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
    if errors > 0 || too_many || !fails.is_empty() || oxlint_failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
