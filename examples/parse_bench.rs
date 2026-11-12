//! Parse every `.vue` file under the given directories (release: `cargo run --release --example
//! parse_bench -- <dir>...`) and report throughput, parse errors and panics.

use std::path::{Path, PathBuf};
use std::time::Instant;

use oxc::allocator::Allocator;
use oxlint_vue_rs::parser::{ParserOptions, parse_document};

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "node_modules") {
                collect(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "vue") {
            out.push(p);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    for d in std::env::args().skip(1) {
        collect(Path::new(&d), &mut files);
    }
    let sources: Vec<(PathBuf, String)> = files
        .into_iter()
        .filter_map(|p| std::fs::read_to_string(&p).ok().map(|s| (p, s)))
        .collect();
    let bytes: usize = sources.iter().map(|s| s.1.len()).sum();
    let (mut panics, mut with_errors, mut tokens) = (0, 0, 0usize);
    let rounds = 5;
    let start = Instant::now();
    for round in 0..rounds {
        for (path, src) in &sources {
            let ts = src.contains("lang=\"ts\"");
            let r = std::panic::catch_unwind(|| {
                let alloc = Allocator::default();
                let doc = parse_document(&alloc, src, &ParserOptions { ts, jsx: false });
                (doc.errors.len(), doc.tokens.len())
            });
            match r {
                Ok((e, t)) if round == 0 => {
                    if e > 0 {
                        with_errors += 1;
                        if std::env::var("VERBOSE").is_ok() {
                            let alloc = Allocator::default();
                            let doc =
                                parse_document(&alloc, src, &ParserOptions { ts, jsx: false });
                            for err in &doc.errors {
                                let at = err.offset as usize;
                                let end = (at + 30).min(src.len());
                                let end = (end..=src.len())
                                    .find(|&i| src.is_char_boundary(i))
                                    .unwrap_or(src.len());
                                eprintln!(
                                    "{}: {} {:?}",
                                    path.display(),
                                    err.message,
                                    &src[at..end]
                                );
                            }
                        }
                    }
                    tokens += t;
                }
                Ok(_) => {}
                Err(_) if round == 0 => {
                    panics += 1;
                    eprintln!("PANIC: {}", path.display());
                }
                Err(_) => {}
            }
        }
    }
    let secs = start.elapsed().as_secs_f64() / rounds as f64;
    println!(
        "{} files, {:.1} KiB, {} tokens: {:.2} ms/pass, {:.0} files/s, {:.1} MiB/s; {} with parse errors, {} panics",
        sources.len(),
        bytes as f64 / 1024.0,
        tokens,
        secs * 1000.0,
        sources.len() as f64 / secs,
        bytes as f64 / secs / 1024.0 / 1024.0,
        with_errors,
        panics
    );
}
