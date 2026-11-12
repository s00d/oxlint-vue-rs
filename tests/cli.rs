//! CLI regressions: runs the built binary on temp projects.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_oxlint-vue-rs");

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("ovr-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    fn file(&self, rel: &str, text: &str) -> &Dir {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
        self
    }
    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.0.join(rel)).unwrap()
    }
    /// (exit code, stdout, stderr), run in `self/<sub>`.
    fn run(&self, sub: &str, args: &[&str]) -> (i32, String, String) {
        // Keep these runs independent of an oxlint on the machine unless a test asks for it.
        let mut all: Vec<&str> = args.to_vec();
        if !args
            .iter()
            .any(|a| *a == "--with-oxlint" || *a == "--auto-oxlint")
        {
            all.insert(0, "--no-oxlint");
        }
        all.retain(|a| *a != "--auto-oxlint");
        let out = Command::new(BIN)
            .args(&all)
            .current_dir(self.0.join(sub))
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into(),
            String::from_utf8_lossy(&out.stderr).into(),
        )
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const VFOR: &str = "<template>\n  <div v-for=\"x in y\"></div>\n</template>\n";
const BARE: &str = "<template><p>hi</p></template>\n";

/// Sorted `file:line:col rule` lines of `-f unix` output.
fn unix(out: &str) -> Vec<String> {
    let mut v: Vec<String> = out
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (loc, rest) = l.split_once(": ").unwrap();
            let rule = rest.rsplit_once('[').unwrap().1.trim_end_matches(']');
            // Paths print natively (`\` on Windows), like oxlint; compare with `/`.
            format!(
                "{} {}",
                loc.replace('\\', "/"),
                rule.split_once('/').map_or(rule, |x| x.1)
            )
        })
        .collect();
    v.sort();
    v
}

// H2: overrides / ignorePatterns apply for `.`, no args and subdirectories (paths relative to the config).
#[test]
fn overrides_and_ignores_relative_to_config() {
    let d = Dir::new("h2");
    d.file(
        ".oxlintrc.json",
        r#"{ "overrides": [{ "files": ["src/components/**"], "rules": { "vue-rs/require-v-for-key": "error" } }],
             "ignorePatterns": ["other/**"] }"#,
    )
    .file("src/components/A.vue", VFOR)
    .file("other/B.vue", VFOR);
    let want = vec!["src/components/A.vue:2:3 vue-rs/require-v-for-key".to_string()];
    for args in [&[][..], &["."], &["-c", ".oxlintrc.json"]] {
        let mut a = vec!["-f", "unix"];
        a.extend_from_slice(args);
        let (code, out, _) = d.run("", &a);
        assert_eq!(code, 1, "{args:?}");
        assert_eq!(unix(&out), want, "{args:?}");
    }
    let (_, out, _) = d.run("src", &["-f", "unix"]);
    assert_eq!(
        unix(&out),
        vec!["components/A.vue:2:3 vue-rs/require-v-for-key"]
    );
}

// M5: gitignore-style ignorePatterns and nested configs (nearest config wins, not merged).
#[test]
fn nested_configs_and_gitignore_patterns() {
    let d = Dir::new("m5");
    d.file(
        ".oxlintrc.json",
        r#"{ "rules": { "vue-rs/no-bare-strings-in-template": "error" },
             "ignorePatterns": ["/build", "gen/**", "!gen/keep/**"] }"#,
    )
    .file("build/a.vue", BARE)
    .file("x/build/a.vue", BARE)
    .file("gen/b.vue", BARE)
    .file("gen/keep/c.vue", BARE)
    .file(
        "sub/.oxlintrc.json",
        r#"{ "rules": { "vue-rs/require-v-for-key": "warn" } }"#,
    )
    .file("sub/a.vue", &format!("{VFOR}<!-- -->"))
    .file("sub/b.vue", BARE);
    let (_, out, _) = d.run("", &["-f", "unix"]);
    assert_eq!(
        unix(&out),
        vec![
            "gen/keep/c.vue:1:14 vue-rs/no-bare-strings-in-template",
            "sub/a.vue:2:3 vue-rs/require-v-for-key",
            "x/build/a.vue:1:14 vue-rs/no-bare-strings-in-template",
        ]
    );
    // `--disable-nested-config`: the root config for everything.
    let (_, out, _) = d.run("", &["-f", "unix", "--disable-nested-config", "sub"]);
    assert_eq!(
        unix(&out),
        vec!["sub/b.vue:1:14 vue-rs/no-bare-strings-in-template"]
    );
}

// M6: a missing path is an error; no files is exit 1 like oxlint.
#[test]
fn missing_path_and_no_files() {
    let d = Dir::new("m6");
    d.file("a.vue", BARE);
    let (code, _, err) = d.run("", &["nope/"]);
    assert_eq!(code, 2);
    assert!(err.contains("nope/"), "{err}");
    std::fs::create_dir_all(d.0.join("empty")).unwrap();
    let (code, out, _) = d.run("", &["empty"]);
    assert_eq!(code, 1);
    assert!(out.contains("No files found"), "{out}");
}

// M8: one file under several spellings is linted once; --fix writes atomically, keeps permissions.
#[cfg(unix)]
#[test]
fn aliases_dedup_and_fix_keeps_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("m8");
    d.file("a.vue", "<template><div></div></template>\n");
    let abs = d.0.join("a.vue");
    let abs_s = abs.to_str().unwrap();
    let (_, out, _) = d.run(
        "",
        &[
            "-f",
            "unix",
            "-D",
            "html-self-closing",
            ".",
            "a.vue",
            "./a.vue",
            abs_s,
        ],
    );
    assert_eq!(unix(&out), vec!["a.vue:1:16 vue-rs/html-self-closing"]);
    std::fs::set_permissions(&abs, std::fs::Permissions::from_mode(0o640)).unwrap();
    let (code, _, err) = d.run(
        "",
        &["--fix", "-D", "html-self-closing", ".", "a.vue", abs_s],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(d.read("a.vue"), "<template><div /></template>\n");
    let mode = std::fs::metadata(&abs).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    let leftovers: Vec<_> = std::fs::read_dir(&d.0)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

// M4: a script syntax error is one fatal ruleId-less "Parsing error" (no rules run), exit 1.
#[test]
fn script_parse_error_is_fatal() {
    let d = Dir::new("m4");
    d.file(
        "e.vue",
        "<template><p>hi</p></template>\n<script>\nconst = ;\n</script>\n",
    );
    let (code, out, _) = d.run(
        "",
        &["-f", "unix", "-D", "no-bare-strings-in-template", "e.vue"],
    );
    assert_eq!(code, 1);
    assert_eq!(
        out.trim(),
        "e.vue:3:7: Parsing error: Unexpected token = [Error]"
    );
    let (_, out, _) = d.run("", &["-f", "json", "-D", "html-indent", "e.vue"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(v["diagnostics"][0]["code"], "");
}

// H1: deep nesting must not abort the process (stack overflow on the default 2 MiB stack).
#[test]
fn deep_nesting_does_not_abort() {
    let d = Dir::new("h1");
    let n = 5000;
    d.file(
        "e.vue",
        &format!(
            "<template><div>{{{{ {}a{} }}}}</div></template>\n",
            "(".repeat(n),
            ")".repeat(n)
        ),
    )
    .file(
        "s.vue",
        &format!(
            "<script>\nx = {}{}\n</script>\n",
            "[".repeat(n),
            "]".repeat(n)
        ),
    )
    .file("ok.vue", BARE);
    let (code, out, err) = d.run(
        "",
        &[
            "-f",
            "unix",
            "-D",
            "no-v-html",
            "-D",
            "one-component-per-file",
            "-D",
            "no-bare-strings-in-template",
        ],
    );
    assert_eq!(code, 1, "{err}");
    assert!(out.contains("ok.vue:1:14"), "{out}");
}

// L1: a BOM is not a column (ESLint strips it) and --fix keeps it.
#[test]
fn bom_is_stripped_and_restored() {
    let d = Dir::new("l1");
    d.file("b.vue", "\u{feff}<template><div></div></template>\n");
    let (_, out, _) = d.run("", &["-f", "unix", "-D", "html-self-closing", "b.vue"]);
    assert_eq!(unix(&out), vec!["b.vue:1:16 vue-rs/html-self-closing"]);
    d.run(
        "",
        &[
            "--fix",
            "-D",
            "html-self-closing",
            "-D",
            "html-indent",
            "b.vue",
        ],
    );
    assert_eq!(d.read("b.vue"), "\u{feff}<template><div /></template>\n");
}

// `--fix` runs until stable: `order-in-components` moves one property per pass, so this list needs
// more than ESLint's 10 passes. A BOM + CRLF file keeps both.
#[test]
fn fix_runs_until_stable() {
    let d = Dir::new("passes");
    let keys = [
        "watch",
        "methods",
        "computed",
        "setup",
        "data",
        "props",
        "model",
        "inject",
        "provide",
        "emits",
        "mixins",
        "filters",
        "directives",
        "components",
        "name",
    ];
    let body: String = keys.iter().map(|k| format!("  {k}: {{}},\r\n")).collect();
    d.file(
        "a.vue",
        &format!("\u{feff}<script>\r\nexport default {{\r\n{body}}}\r\n</script>\r\n"),
    );
    let (code, _, err) = d.run("", &["--fix", "-D", "order-in-components", "a.vue"]);
    assert_eq!(code, 0, "{err}");
    let out = d.read("a.vue");
    assert!(
        out.starts_with("\u{feff}<script>\r\nexport default {\r\n  name: {},\r\n"),
        "{out:?}"
    );
    assert!(!out.replace("\r\n", "").contains('\n'), "{out:?}");
}

// Expected positions below are ESLint + eslint-plugin-vue output for the same inputs.

// L2: in an SFC only `\n` breaks lines (vue-eslint-parser); lone CR and U+2028 do not.
#[test]
fn sfc_lines_break_on_lf_only() {
    let d = Dir::new("l2");
    d.file(
        "c.vue",
        "<template>\r  <div title=\"x\"><div></div></div>\r</template>\n",
    )
    .file(
        "u.vue",
        "<template>\n  <p title=\"\u{2028}\"><div></div></p>\n</template>\n",
    );
    let (_, out, _) = d.run("", &["-f", "unix", "-D", "html-self-closing"]);
    assert_eq!(
        unix(&out),
        vec![
            "c.vue:1:34 vue-rs/html-self-closing",
            "u.vue:2:21 vue-rs/html-self-closing",
            "u.vue:2:3 vue-rs/html-self-closing",
        ]
    );
}

// M2 / M3: directive descriptions (`\s-{2,}\s`), template rule lists split on whitespace,
// multi-line `-line` ignored, JS comments inside template expressions honored.
#[test]
fn directive_comments() {
    let d = Dir::new("m2");
    d.file(
        "t.vue",
        "<template>
  <div>
    <!-- eslint-disable-next-line vue/no-bare-strings-in-template vue/foo -->
    <p>a</p>
    <!-- eslint-disable-next-line vue/no-bare-strings-in-template
         -- multi-line -->
    <p>b</p>
    <p>{{ a == b /* eslint-disable-line vue/eqeqeq */ }}</p>
    <p :x=\"c == d // eslint-disable-line vue/eqeqeq\n\"></p>
    <p>{{ e == f }}</p>
  </div>
</template>
<script>
export default {
  props: {
    /* eslint-disable-line vue/no-boolean-default
    */ a: { type: Boolean, default: true },
    b: { type: Boolean, default: true }, // eslint-disable-line vue/no-boolean-default\t-- tab
    c: { type: Boolean, default: true }, // eslint-disable-line vue/no-boolean-default --x
  }
}
</script>
",
    );
    let (_, out, _) = d.run(
        "",
        &[
            "-f",
            "unix",
            "-D",
            "no-bare-strings-in-template",
            "-D",
            "eqeqeq",
            "-D",
            "no-boolean-default",
        ],
    );
    assert_eq!(
        unix(&out),
        vec![
            "t.vue:11:13 vue-rs/eqeqeq",
            "t.vue:18:37 vue-rs/no-boolean-default",
            "t.vue:20:34 vue-rs/no-boolean-default",
            "t.vue:7:8 vue-rs/no-bare-strings-in-template",
        ]
    );
}

// L5: GitHub annotation properties are escaped; an unknown format is an error.
// (`:` is not allowed in Windows file names.)
#[cfg(unix)]
#[test]
fn github_format_escapes_and_unknown_format() {
    let d = Dir::new("l5");
    d.file("a,b:c.vue", BARE);
    let (_, out, _) = d.run("", &["-f", "github", "-D", "no-bare-strings-in-template"]);
    assert!(
        out.starts_with("::error file=a%2Cb%3Ac.vue,line=1,"),
        "{out}"
    );
    assert!(
        out.contains("title=vue-rs/no-bare-strings-in-template::"),
        "{out}"
    );
    let (code, _, _) = d.run("", &["-f", "xml"]);
    assert_eq!(code, 2);
}

// M7: oxlint's paths are merged with ours, its file count and exit status are used.
#[cfg(unix)]
#[test]
fn with_oxlint_merges_paths_count_and_status() {
    let d = Dir::new("m7");
    d.file("src/A.vue", "<template><p>hi</p></template>\n<script>\nif (\n a\n ==\n b) {}\n</script>\n")
        .file(
            "node_modules/.bin/oxlint",
            "#!/bin/sh\ncat <<'E'\n{\"diagnostics\": [{\"message\": \"eq\", \"code\": \"eslint(eqeqeq)\", \"severity\": \"warning\", \"help\": \"Prefer ===\", \"filename\": \"src/A.vue\", \"labels\": [{\"span\": {\"offset\": 46, \"length\": 8, \"line\": 4, \"column\": 2}}]}], \"number_of_files\": 2}\nE\nexit 1\n",
        );
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        d.0.join("node_modules/.bin/oxlint"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (code, out, err) = d.run("", &["--with-oxlint", "-f", "json"]);
    assert_eq!(code, 1, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["number_of_files"], 2);
    let d0 = &v["diagnostics"][0];
    assert_eq!(d0["filename"], "src/A.vue");
    assert_eq!(d0["help"], "Prefer ===");
    let span = &d0["labels"][0]["span"];
    assert_eq!(
        (span["endLine"].clone(), span["endColumn"].clone()),
        (6.into(), 3.into())
    );
}

// L6: vue2 presets keep the deprecated rules upstream still enables; v-on-event-hyphenation autofix.
#[test]
fn presets_match_upstream_gaps() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("npm/configs");
    let read = |n: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(root.join(n)).unwrap()).unwrap()
    };
    for p in [
        "vue2-essential",
        "vue2-strongly-recommended",
        "vue2-recommended",
    ] {
        let v = read(&format!("{p}.json"));
        for r in [
            "no-v-for-template-key",
            "no-v-model-argument",
            "valid-model-definition",
            "valid-v-bind-sync",
        ] {
            assert_eq!(v["rules"][format!("vue-rs/{r}")], "error", "{p} {r}");
        }
    }
    for p in ["strongly-recommended", "recommended"] {
        let v = read(&format!("{p}.json"));
        assert_eq!(
            v["rules"]["vue-rs/v-on-event-hyphenation"],
            serde_json::json!(["warn", "always", {"autofix": true}])
        );
    }
}

/// oxlint runs by default when it is in node_modules/.bin; `--no-oxlint` skips it.
#[cfg(unix)]
#[test]
fn oxlint_runs_by_default_when_installed() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("auto-oxlint");
    d.file("src/A.vue", "<template><p>hi</p></template>\n")
        .file(
            "node_modules/.bin/oxlint",
            "#!/bin/sh\ncat <<'E'\n{\"diagnostics\": [{\"message\": \"from oxlint\", \"code\": \"eslint(x)\", \"severity\": \"warning\", \"filename\": \"src/A.vue\", \"labels\": [{\"span\": {\"offset\": 0, \"length\": 1, \"line\": 1, \"column\": 1}}]}], \"number_of_files\": 1}\nE\n",
        );
    std::fs::set_permissions(
        d.0.join("node_modules/.bin/oxlint"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (_, out, err) = d.run("", &["--auto-oxlint", "-f", "json"]);
    assert!(
        out.contains("from oxlint"),
        "auto mode must run oxlint: {out} {err}"
    );
    let (_, out, _) = d.run("", &["-f", "json"]); // helper adds --no-oxlint
    assert!(!out.contains("from oxlint"), "{out}");
}

#[test]
fn with_oxlint_required_errors_when_missing() {
    let d = Dir::new("need-oxlint");
    d.file("src/A.vue", "<template><p>hi</p></template>\n");
    let out = Command::new(BIN)
        .args(["--with-oxlint", "src"])
        .current_dir(&d.0)
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

// M2: an oxlint.config.ts we cannot read, with no JSON config: warning; exit 2 unless the command line
// enables a rule of ours.
#[test]
fn js_config_without_json_config() {
    let d = Dir::new("jscfg");
    d.file("oxlint.config.ts", "export default {}\n")
        .file("src/a.vue", VFOR);
    let (code, _, err) = d.run("", &["src"]);
    assert_eq!(code, 2, "{err}");
    assert!(
        err.contains("oxlint.config.ts") && err.contains(".oxlintrc.json"),
        "{err}"
    );
    let (code, _, err) = d.run("", &["-D", "require-v-for-key", "src"]);
    assert_eq!(code, 1, "{err}");
    d.file(".oxlintrc.json", "{}");
    let (code, _, err) = d.run("", &["src"]);
    assert_eq!((code, err.as_str()), (0, ""));
}

// M3: `vue/<our rule>` keys are reported with the alias to use; unknown names keep the user's prefix.
#[test]
fn misnamed_and_unknown_rule_warnings() {
    let d = Dir::new("misnamed");
    d.file(
        ".oxlintrc.json",
        r#"{ "jsPlugins": [{ "name": "vx", "specifier": "oxlint-vue-rs" }],
            "rules": { "vue/html-indent": "warn", "vue/no-dupe-keys": "error", "vx/not-a-rule": "error" } }"#,
    )
    .file("a.vue", BARE);
    let (code, _, err) = d.run("", &[]);
    assert_eq!(code, 0, "{err}");
    assert!(
        err.contains("\"vue/html-indent\" is an oxlint-vue-rs rule, use \"vx/html-indent\""),
        "{err}"
    );
    assert!(!err.contains("no-dupe-keys"), "{err}");
    assert!(err.contains("unknown rule vx/not-a-rule"), "{err}");
}

// Rules in settings["vue-rs"].rules (setup without the JS plugin) run like `rules`.
#[test]
fn rules_from_settings() {
    let d = Dir::new("settings-rules");
    d.file(
        ".oxlintrc.json",
        r#"{ "settings": { "vue-rs": { "rules": { "vue-rs/require-v-for-key": "error" } } } }"#,
    )
    .file("a.vue", VFOR);
    let (code, out, err) = d.run("", &["-f", "unix"]);
    assert_eq!(code, 1, "{err}");
    assert_eq!(unix(&out), vec!["a.vue:2:3 vue-rs/require-v-for-key"]);
}

// M4: presets are the whole upstream config (native vue/* rules too, the rest of them off).
#[test]
fn presets_include_native_rules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let inv: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("tests/rules-inventory.json")).unwrap(),
    )
    .unwrap();
    for p in [
        "essential",
        "recommended",
        "vue2-strongly-recommended",
        "settings/recommended",
    ] {
        let v: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(root.join(format!("npm/configs/{p}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(v["plugins"], serde_json::json!(["vue"]), "{p}");
        for n in inv["native"].as_array().unwrap() {
            assert!(
                v["rules"]
                    .get(format!("vue/{}", n.as_str().unwrap()))
                    .is_some(),
                "{p} {n}"
            );
        }
    }
}

// L11: --fix leaves a read-only file alone and reports it.
#[cfg(unix)]
#[test]
fn fix_skips_read_only_files() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("readonly");
    d.file("a.vue", "<template><div></div></template>\n");
    let p = d.0.join("a.vue");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o444)).unwrap();
    if std::fs::OpenOptions::new().write(true).open(&p).is_ok() {
        return; // root ignores permissions
    }
    let (code, _, err) = d.run("", &["--fix", "-D", "html-self-closing", "a.vue"]);
    assert_eq!(code, 1, "{err}");
    assert!(err.contains("fixes not written"), "{err}");
    assert_eq!(d.read("a.vue"), "<template><div></div></template>\n");
}

// L4: a closed stdout (`| head`) is not a panic.
#[cfg(unix)]
#[test]
fn closed_stdout_exits_quietly() {
    let d = Dir::new("epipe");
    for i in 0..50 {
        d.file(&format!("a{i}.vue"), VFOR);
    }
    for args in [&["--no-oxlint", "-D", "all", "."][..], &["--rules"][..]] {
        let mut child = Command::new(BIN)
            .args(args)
            .current_dir(&d.0)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert!(!err.contains("panicked"), "{err}");
        assert!(
            matches!(out.status.code(), Some(0 | 1)),
            "{:?} {err}",
            out.status
        );
    }
}

// L10: ignore options and -A/-W/-D names that are not ours go to oxlint; ours stay here.
#[cfg(unix)]
#[test]
fn oxlint_gets_ignore_and_foreign_rule_flags() {
    use std::os::unix::fs::PermissionsExt;
    let d = Dir::new("fwd");
    d.file("a.vue", BARE).file(
        "node_modules/.bin/oxlint",
        "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\"; done > args.txt\necho '{\"diagnostics\": [], \"number_of_files\": 1}'\n",
    );
    std::fs::set_permissions(
        d.0.join("node_modules/.bin/oxlint"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let (code, _, err) = d.run(
        "",
        &[
            "--with-oxlint",
            "--no-ignore",
            "--ignore-pattern",
            "gen/**",
            "-D",
            "no-debugger",
            "-W",
            "vue/no-dupe-keys",
            "-D",
            "require-v-for-key",
            "-A",
            "all",
            "-D",
            "vue/html-indent",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let args: Vec<String> = d.read("args.txt").lines().map(String::from).collect();
    let has = |a: &[&str]| args.windows(a.len()).any(|w| w == a);
    assert!(
        has(&["--no-ignore"]) && has(&["--ignore-pattern=gen/**"]),
        "{args:?}"
    );
    assert!(
        has(&["-D", "no-debugger"]) && has(&["-W", "vue/no-dupe-keys"]),
        "{args:?}"
    );
    for ours in ["require-v-for-key", "all", "vue/html-indent"] {
        assert!(!args.iter().any(|a| a == ours), "{ours} {args:?}");
    }
}
