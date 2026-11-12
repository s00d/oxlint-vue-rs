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
        let out = Command::new(BIN)
            .args(args)
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
            format!("{loc} {}", rule.split_once('/').map_or(rule, |x| x.1))
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
