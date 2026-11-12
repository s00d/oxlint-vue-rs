//! vue/no-restricted-v-bind

use serde_json::json;

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_restrict::{DirectiveOpt, check_directives, mods_suffix, static_arg_raw};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let default = [json!({
        "argument": "/^v-/",
        "message": "Using `:v-xxx` is not allowed. Instead, remove `:` and use it as directive."
    })];
    let src = if opts.options.is_empty() { &default[..] } else { opts.options };
    let options = src.iter().filter_map(DirectiveOpt::parse).collect();
    Some(Box::new(NoRestrictedVBind { options }))
}

struct NoRestrictedVBind {
    options: Vec<DirectiveOpt>,
}

impl Rule for NoRestrictedVBind {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        check_directives(ctx, "bind", &self.options, "restrictedVBind", |e, k, o| {
            let vbind = if k.name.raw_name == ":" { "" } else { "v-bind" };
            let arg = static_arg_raw(k).map(|a| format!(":{a}")).unwrap_or_default();
            let on = if o.element.is_some() {
                format!(" on `<{}>`", e.raw_name)
            } else {
                String::new()
            };
            format!("Using `{vbind}{arg}{}`{on} is not allowed.", mods_suffix(o))
        });
    }
}
