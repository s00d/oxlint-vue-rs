#!/usr/bin/env node
// Copy vue-eslint-parser AST fixtures into tests/parser-fixtures/ and extract a compact
// expected.json (template tokens/comments/errors/references/variables, UTF-8 byte offsets)
// from the large ast.json files (which are not copied).
//
// Usage: node scripts/extract-parser-fixtures.js <vue-eslint-parser checkout>
const fs = require("fs");
const path = require("path");

const repo = process.argv[2];
const SRC = path.join(repo, "test/fixtures/ast");
const OUT = path.join(__dirname, "../tests/parser-fixtures");
fs.mkdirSync(OUT, { recursive: true });
fs.copyFileSync(path.join(repo, "LICENSE"), path.join(OUT, "LICENSE"));

function skipReason(name, opts, source) {
    if (opts.templateTokenizer || name === "pug" || name === "custom-template-tokenizer") return "custom template tokenizer (pug)";
    if (opts.ecmaVersion === 5) return "ecmaVersion 5 (espree-only v-for path)";
    if (opts.parser === "@babel/eslint-parser") return "babel parser";
    const vf = opts.vueFeatures || {};
    if (vf.filter === false) return "vueFeatures.filter=false";
    if (vf.interpolationAsNonHTML === false) return "vueFeatures.interpolationAsNonHTML=false";
    return null;
}

function isTs(opts, source) {
    const p = opts.parser;
    if (typeof p === "string") return /typescript/.test(p);
    if (p && typeof p === "object") {
        const m = /<script[^>]*\blang=["']?(\w+)/.exec(source);
        const lang = m ? m[1] : "js";
        const v = p["<template>"] || p[lang] || (lang === "tsx" ? p.ts : undefined);
        return typeof v === "string" && /typescript/.test(v);
    }
    return false;
}

for (const name of fs.readdirSync(SRC).sort()) {
    const dir = path.join(SRC, name);
    if (!fs.statSync(dir).isDirectory()) continue;
    const source = fs.readFileSync(path.join(dir, "source.vue"), "utf8");
    const optsPath = path.join(dir, "parser-options.json");
    const opts = fs.existsSync(optsPath) ? JSON.parse(fs.readFileSync(optsPath, "utf8")) : {};
    const ast = JSON.parse(fs.readFileSync(path.join(dir, "ast.json"), "utf8"));
    // UTF-16 index -> UTF-8 byte offset.
    const b = (i) => Buffer.byteLength(source.slice(0, i));
    const tb = ast.templateBody;
    const out = { ts: isTs(opts, source), skip: skipReason(name, opts, source), hasTemplateBody: tb != null };
    if (tb) {
        const tok = (t) => [t.type, b(t.range[0]), b(t.range[1]), t.value];
        out.tokens = tb.tokens.map(tok);
        out.comments = tb.comments.map(tok);
        out.errors = tb.errors.map((e) => [e.message, b(e.index)]);
        out.elements = [];
        out.references = [];
        out.variables = [];
        const walk = (n) => {
            if (!n || typeof n !== "object") return;
            if (Array.isArray(n)) return n.forEach(walk);
            if (n.type === "VElement") {
                out.elements.push([n.name, n.rawName, n.namespace.replace(/^.*\//, ""), b(n.range[0]), b(n.range[1])]);
                if (n.variables.length) out.variables.push([b(n.range[0]), n.variables.map((v) => [v.id.name, v.kind, b(v.id.range[0])])]);
            }
            if (n.type === "VExpressionContainer" && n.references.length) {
                out.references.push([b(n.range[0]), n.references.map((r) => [r.id.name, r.mode, b(r.id.range[0])])]);
            }
            for (const k of Object.keys(n)) {
                if (k !== "tokens" && k !== "comments" && k !== "errors" && k !== "parent" && k !== "loc" && k !== "range") walk(n[k]);
            }
        };
        walk(tb);
    }
    const dst = path.join(OUT, name);
    fs.mkdirSync(dst, { recursive: true });
    for (const f of ["source.vue", "tree.json", "token-ranges.json"]) fs.copyFileSync(path.join(dir, f), path.join(dst, f));
    fs.writeFileSync(path.join(dst, "expected.json"), JSON.stringify(out) + "\n");
}
