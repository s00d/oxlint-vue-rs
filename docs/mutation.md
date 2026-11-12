# Mutation testing

## Method

[cargo-mutants](https://mutants.rs) changes the code one small edit at a time (replace a function body with a
default value, flip a comparison, delete a `!`, swap `&&`/`||`, …) and runs the tests against each mutant. A
mutant the tests still pass on ("missed") marks logic no test pins down.

```bash
cargo install cargo-mutants --locked
cargo mutants --file src/config.rs --file src/directives.rs --file src/main.rs -- --release
cargo mutants --file 'src/rules/<rule>.rs' -- --release --test upstream   # one rule against its upstream cases
```

Scope: the shared infrastructure (config loading, disable directives, CLI) and the rules, file by file. Results
are in `mutants.out/` (`missed.txt`, `caught.txt`, `timeout.txt`, `unviable.txt`). Each missed mutant is either
killed by a new test or noted below as equivalent (the change does not alter behaviour).

## Results

Shared infrastructure, cargo-mutants 27.1, tests `--lib --test cli --test directives --test upstream`
(timeouts count as caught — the mutant hung):

| run | scope | mutants | caught | timeout | missed | unviable | score |
|---|---|---:|---:|---:|---:|---:|---:|
| 1 | `lib.rs`, `config.rs`, `directives.rs` | 314 | 192 | 20 | 84 | 18 | 72% |
| 2 | `config.rs`, `directives.rs` after new tests | 239 | 168 | 20 | 39 | 12 | 83% |

Run 1's survivors drove new tests: severity spellings and rule-setting arrays, `jsPlugins` alias detection,
`extends` chains / settings merge / recursion limit, JSONC comments, disable/enable/`-line`/`-next-line` ranges and
template directives (`src/config.rs` tests, `tests/directives.rs`); after run 2, exact JSONC end-of-input edges.

Remaining survivors, reviewed:

- `lib.rs` `guess_script_lang` and the `ts`/`tsx`/`jsx` arms after it: equivalent — the guess only avoids a second
  parse of the template; a wrong guess re-parses with the right language, so results do not change.
- `config.rs` `Config::empty` fields: equivalent for every observable behaviour (the defaults are filled again where used).
- `directives.rs` comparisons on sweep restarts (`filter`): the restart path only runs for out-of-order diagnostics,
  which `lint_source` never produces (it sorts them).
- a few `strip_jsonc` index bounds at the very end of input that only change behaviour on invalid JSON.

Rules (`src/rules/*`, ~5000 mutants, roughly 40 machine-hours at this test cost) were not mutation-tested. Their
behaviour is pinned instead by the 6839 replayed eslint-plugin-vue cases, ~101 000 differential comparisons against
ESLint over option combinations and source mutations (final audit), and exact output on 151 548 problems in real
projects (see [compatibility.md](compatibility.md)).
