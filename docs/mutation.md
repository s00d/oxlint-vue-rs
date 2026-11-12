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

Results: see below.
