//! vue/no-deprecated-v-on-number-modifiers

use crate::rule::{Ctx, Fix, Rule, RuleOptions};
use crate::utils::tpl_misc::{is_safe_integer, js_to_number};
use crate::utils::tpl_valid::body_directives;

struct R;

/// `utils/keycode-to-key.ts`.
fn key_of(code: f64) -> Option<String> {
    let c = code as u32;
    Some(match c {
        8 => "backspace".into(),
        9 => "tab".into(),
        13 => "enter".into(),
        16 => "shift".into(),
        17 => "ctrl".into(),
        18 => "alt".into(),
        19 => "pause".into(),
        20 => "caps-lock".into(),
        27 => "escape".into(),
        32 => "space".into(),
        33 => "page-up".into(),
        34 => "page-down".into(),
        35 => "end".into(),
        36 => "home".into(),
        37 => "arrow-left".into(),
        38 => "arrow-up".into(),
        39 => "arrow-right".into(),
        40 => "arrow-down".into(),
        45 => "insert".into(),
        46 => "delete".into(),
        65..=90 => ((b'a' + (c - 65) as u8) as char).to_string(),
        112..=123 => format!("f{}", c - 111),
        144 => "num-lock".into(),
        145 => "scroll-lock".into(),
        _ => return None,
    })
}

impl Rule for R {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (_, _, _, k) in body_directives(doc, "on") {
            let Some((m, n)) = k
                .modifiers
                .iter()
                .find_map(|m| js_to_number(&m.name).filter(|n| is_safe_integer(*n)).map(|n| (m, n)))
            else {
                continue;
            };
            if !(0.0..=9.0).contains(&n) {
                let fix = key_of(n).map(|key| Fix::replace(m.span, key));
                ctx.report_fix(
                    m.span,
                    None,
                    "'KeyboardEvent.keyCode' modifier on 'v-on' directive is deprecated. Using 'KeyboardEvent.key' instead.",
                    fix,
                );
            }
        }
    }
}

pub fn create(_opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(R))
}
