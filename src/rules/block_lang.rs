//! vue/block-lang

use oxc::span::Span;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::literal_value;

struct Block {
    tag: String,
    lang: Vec<String>,
    allow_no_lang: bool,
    required: bool,
}

struct R {
    blocks: Vec<Block>,
}

fn defaults(tag: &str) -> &'static [&'static str] {
    match tag {
        "template" => &["html"],
        "style" => &["css"],
        "script" => &["js", "javascript"],
        _ => &[],
    }
}

fn allows(lang: &[String]) -> String {
    let l: Vec<String> = lang.iter().map(|s| format!("'{s}'")).collect();
    match l.len() {
        1 => l[0].clone(),
        n => format!(
            "{}, and {}",
            l[..n.saturating_sub(1)].join(", "),
            l.last().cloned().unwrap_or_default()
        ),
    }
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for e in doc.top_level() {
            let tag = e.name.as_str();
            let Some(o) = self.blocks.iter().find(|b| b.tag == tag) else {
                continue;
            };
            let lang = e.attr("lang");
            let Some(value) = lang.and_then(literal_value) else {
                if !o.allow_no_lang {
                    ctx.report_id(
                        e.start_tag.span,
                        "missing",
                        format!("The 'lang' attribute of '<{tag}>' is missing."),
                    );
                }
                continue;
            };
            if o.lang.iter().any(|l| l == value) {
                continue;
            }
            let a = allows(&o.lang);
            let (id, msg) = if !o.allow_no_lang {
                (
                    "expected",
                    format!("Only {a} can be used for the 'lang' attribute of '<{tag}>'."),
                )
            } else if o.lang.is_empty() {
                if defaults(tag).contains(&value) {
                    (
                        "unexpectedDefault",
                        format!(
                            "Do not explicitly specify the default language for the 'lang' attribute of '<{tag}>'."
                        ),
                    )
                } else {
                    (
                        "unexpected",
                        format!("Do not specify the 'lang' attribute of '<{tag}>'."),
                    )
                }
            } else {
                (
                    "useOrNot",
                    format!(
                        "Only {a} can be used for the 'lang' attribute of '<{tag}>'. Or, not specifying the 'lang' attribute is allowed."
                    ),
                )
            };
            ctx.report_id(lang.map_or(e.start_tag.span, |l| l.span), id, msg);
        }
        for b in &self.blocks {
            if b.required && !doc.top_level().any(|e| e.name == b.tag) {
                ctx.report_id(
                    Span::empty(0),
                    "missingBlock",
                    format!("The '<{}>' block is missing.", b.tag),
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let default = serde_json::json!({
        "script": { "allowNoLang": true },
        "template": { "allowNoLang": true },
        "style": { "allowNoLang": true }
    });
    let o = opts.get(0);
    let o = if o.is_object() { o } else { &default };
    let mut blocks = Vec::new();
    for (tag, v) in o.as_object().into_iter().flatten() {
        if !v.is_object() {
            continue;
        }
        let mut lang: Vec<String> = Vec::new();
        let raw: Vec<&str> = match &v["lang"] {
            serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect(),
            serde_json::Value::String(s) => vec![s.as_str()],
            _ => Vec::new(),
        };
        for l in raw {
            if !lang.iter().any(|x| x == l) {
                lang.push(l.to_string());
            }
        }
        let mut has_default = false;
        for d in defaults(tag) {
            if let Some(i) = lang.iter().position(|x| x == d) {
                lang.remove(i);
                has_default = true;
            }
        }
        let required = v["required"].as_bool() == Some(true);
        let allow_no_lang =
            lang.is_empty() || has_default || v["allowNoLang"].as_bool() == Some(true);
        blocks.push(Block {
            tag: tag.clone(),
            lang,
            allow_no_lang,
            required,
        });
    }
    Some(Box::new(R { blocks }))
}
