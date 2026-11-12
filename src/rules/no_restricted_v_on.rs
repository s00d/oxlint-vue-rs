//! vue/no-restricted-v-on

use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::tpl_restrict::{DirectiveOpt, check_directives, mods_suffix, static_arg_raw};

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    let options = opts.options.iter().filter_map(DirectiveOpt::parse).collect();
    Some(Box::new(NoRestrictedVOn { options }))
}

struct NoRestrictedVOn {
    options: Vec<DirectiveOpt>,
}

impl Rule for NoRestrictedVOn {
    fn run(&self, ctx: &mut Ctx<'_, '_>) {
        if self.options.is_empty() {
            return;
        }
        check_directives(ctx, "on", &self.options, "restrictedVOn", |e, k, o| {
            let at = k.name.raw_name == "@";
            let von = if at { "" } else { "v-on" };
            let arg = static_arg_raw(k)
                .map(|a| format!("{}{a}", if at { "@" } else { ":" }))
                .unwrap_or_default();
            let element = if o.element.is_some() {
                format!("<{}>", e.raw_name)
            } else {
                "element".to_string()
            };
            format!(
                "Using `{von}{arg}{}` is not allowed on this {element}.",
                mods_suffix(o)
            )
        });
    }
}
