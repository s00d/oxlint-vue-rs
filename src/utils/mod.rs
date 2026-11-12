//! Shared helpers for rules (ports of eslint-plugin-vue `lib/utils`).
//!
//! - [`casing`]: case checks/converters (`casing.ts`).
//! - [`regexp`]: `/pattern/flags` option strings (`regexp.ts`).
//! - [`tokens`]: ESLint `SourceCode` token/comment navigation over script tokens.
//! - [`js`]: generic oxc AST helpers (static names, literal values, parents, symbols).
//! - [`template`]: template element/attribute helpers + shared name lists (`data/*.json`).
//! - [`vue_component`]: Vue component detection, `<script setup>` macros, props/emits.

pub mod casing;
pub mod esquery;
pub mod indent;
pub mod js;
pub mod property_references;
pub mod ref_object_references;
pub mod reference_tracker;
pub mod regexp;
pub mod template;
pub mod tokens;
pub mod tpl_analysis;
pub mod tpl_core;
pub mod tpl_ext;
pub mod tpl_misc;
pub mod tpl_restrict;
pub mod tpl_style;
pub mod tpl_valid;
pub mod vue_component;
