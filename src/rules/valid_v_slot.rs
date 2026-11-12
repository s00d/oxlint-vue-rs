//! vue/valid-v-slot

use oxc::ast::ast::Expression;
use oxc::span::{GetSpan, Span};

use crate::ast::*;
use crate::rule::{Ctx, Rule, RuleOptions};
use crate::utils::template::{
    child_element_chains, directive_key, directive_value, is_custom_component,
    is_empty_expression_value_directive,
};
use crate::utils::tpl_valid::{
    arg_span, body_directives, equal_tokens, expr_span, missing_value, modifiers_span, same_type,
};

/// A directive identified by (element, attribute index).
type DirId = (ElementId, usize);

/// The first `v-slot` directive of an element.
fn first_slot(e: &Element) -> Option<DirId> {
    e.attributes()
        .iter()
        .position(|a| directive_key(a).is_some_and(|k| k.name.name == "slot"))
        .map(|i| (e.id, i))
}

fn attr<'d>(doc: &'d Document, d: DirId) -> (&'d Element, &'d Attribute, &'d DirectiveKey) {
    let e = doc.element(d.0);
    let a = &e.attributes()[d.1];
    (e, a, directive_key(a).expect("slot directive"))
}

fn variable<'d>(doc: &'d Document, r: &Reference) -> Option<&'d Variable> {
    let (e, i) = r.variable?;
    doc.element(e).variables.get(i as usize)
}

fn normalized_name<'a>(doc: &Document<'a>, k: &DirectiveKey) -> &'a str {
    match arg_span(doc, k) {
        None => "default",
        Some(s) if k.modifiers.is_empty() => doc.slice(s),
        Some(s) => doc.slice(Span::new(s.start, k.span.end)),
    }
}

/// `VSlotVForVariables`: the element's `v-for` expression plus the iteration variables used by the slot argument.
struct VForVars<'d, 'a> {
    left: Vec<
        Option<(
            Span,
            std::mem::Discriminant<oxc::ast::ast::BindingPattern<'a>>,
        )>,
    >,
    right: &'d Expression<'a>,
    variables: Vec<&'d Variable>,
}

fn vfor_vars<'d, 'a>(doc: &'d Document<'a>, slot: DirId) -> Option<VForVars<'d, 'a>> {
    let (e, _, k) = attr(doc, slot);
    let vfor = e.directive("for", None)?;
    let Some(VExpression::For {
        left, rest, right, ..
    }) = &directive_value(doc, vfor)?.expression
    else {
        return None;
    };
    let Some(DirectiveArgument::Dynamic(arg)) = &k.argument else {
        return None;
    };
    let tag = e.start_tag.span;
    let variables: Vec<&Variable> = doc
        .expr(*arg)
        .references
        .iter()
        .filter_map(|r| variable(doc, r))
        .filter(|v| {
            v.kind == VariableKind::VFor && v.span.start > tag.start && v.span.end < tag.end
        })
        .collect();
    if variables.is_empty() {
        return None;
    }
    let mut l: Vec<_> = left
        .iter()
        .map(|p| p.as_ref().map(|p| (p.span(), std::mem::discriminant(p))))
        .collect();
    if let Some(r) = rest {
        // ESTree `RestElement`: a type of its own.
        l.push(Some((r.span, std::mem::discriminant(&r.argument))));
    }
    Some(VForVars {
        left: l,
        right,
        variables,
    })
}

fn vars_equal<'d, 'a>(doc: &Document, a: &VForVars<'d, 'a>, b: &VForVars<'d, 'a>) -> bool {
    if a.variables.len() != b.variables.len()
        || !(same_type(a.right, b.right)
            && equal_tokens(doc, expr_span(a.right), expr_span(b.right)))
    {
        return false;
    }
    let mut checked: Vec<String> = Vec::new();
    for (ap, bp) in a.left.iter().zip(&b.left) {
        let find = |p: &Option<(Span, _)>, vars: &[&Variable]| -> Option<String> {
            let (s, _) = (*p)?;
            vars.iter()
                .find(|v| s.start <= v.span.start && v.span.end <= s.end)
                .map(|v| v.name.clone())
        };
        match (find(ap, &a.variables), find(bp, &b.variables)) {
            (Some(an), Some(bn)) => {
                let (Some((sa, ta)), Some((sb, tb))) = (ap, bp) else {
                    return false;
                };
                if an != bn || ta != tb || !equal_tokens(doc, *sa, *sb) {
                    return false;
                }
                checked.push(an);
            }
            (None, None) => {}
            _ => return false,
        }
    }
    a.variables
        .iter()
        .all(|v| checked.contains(&v.name) || b.variables.iter().any(|bv| bv.name == v.name))
}

struct ValidVSlot {
    allow_modifiers: bool,
}

impl Rule for ValidVSlot {
    fn run(&self, ctx: &mut Ctx) {
        let Some(doc) = ctx.doc() else { return };
        for (e, i, a, k) in body_directives(doc, "slot") {
            let this: DirId = (e.id, i);
            let is_default = match &k.argument {
                None => true,
                Some(DirectiveArgument::Static(id)) => id.name == "default",
                _ => false,
            };
            let owner = if e.name == "template" {
                match e.parent {
                    Some(p) => doc.element(p),
                    None => continue,
                }
            } else {
                e
            };
            let owner_is_parent = owner.id != e.id;
            let groups: Vec<Vec<DirId>> = child_element_chains(doc, &owner.children)
                .into_iter()
                .map(|chain| {
                    chain
                        .into_iter()
                        .map(|id| doc.element(id))
                        .filter(|c| c.name == "template")
                        .filter_map(first_slot)
                        .collect()
                })
                .filter(|g: &Vec<DirId>| !g.is_empty())
                .collect();

            if !is_custom_component(owner, false) {
                ctx.report(
                    a.span,
                    format!(
                        "'v-slot' directive must be owned by a custom element, but '{}' is not.",
                        owner.raw_name
                    ),
                );
            }
            if !is_default && e.name != "template" {
                ctx.report(
                    a.span,
                    "Named slots must use '<template>' on a custom element.",
                );
            }
            if !owner_is_parent && !groups.is_empty() {
                ctx.report(a.span, "Default slot must use '<template>' on a custom element when there are other named slots.");
            }
            if first_slot(e) != Some(this) {
                ctx.report(
                    a.span,
                    "An element cannot have multiple 'v-slot' directives.",
                );
            }
            if owner_is_parent {
                let has_vfor = e.has_directive("for");
                let cur = vfor_vars(doc, this);
                let name = normalized_name(doc, k);
                let same: Vec<Vec<DirId>> = groups
                    .iter()
                    .map(|g| {
                        g.iter()
                            .copied()
                            .filter(|d| {
                                if normalized_name(doc, attr(doc, *d).2) != name {
                                    return false;
                                }
                                match (&cur, vfor_vars(doc, *d)) {
                                    (Some(c), Some(o)) => vars_equal(doc, c, &o),
                                    (None, None) => true,
                                    _ => false,
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|g| !g.is_empty())
                    .collect();
                let dup = same.len() >= 2 && !same[0].contains(&this);
                if dup {
                    ctx.report(a.span, "An element cannot have multiple '<template>' elements which are distributed to the same slot.");
                }
                if has_vfor && cur.is_none() {
                    ctx.report(a.span, "An element cannot have multiple '<template>' elements which are distributed to the same slot.");
                }
            }
            if let (Some(DirectiveArgument::Dynamic(arg)), Some(v)) =
                (&k.argument, crate::utils::template::value_span(doc, a))
                && doc
                    .expr(*arg)
                    .references
                    .iter()
                    .filter_map(|r| variable(doc, r))
                    .any(|var| {
                        var.kind == VariableKind::Scope
                            && var.span.start > v.start
                            && var.span.end < v.end
                    })
            {
                ctx.report(
                    a.span,
                    "Dynamic argument of 'v-slot' directive cannot use that slot parameter.",
                );
            }
            let invalid_mods = if self.allow_modifiers {
                k.argument.is_none() && !k.modifiers.is_empty()
            } else {
                !k.modifiers.is_empty()
            };
            if invalid_mods && let Some(s) = modifiers_span(k) {
                ctx.report(s, "'v-slot' directive doesn't support any modifier.");
            }
            if !owner_is_parent
                && is_default
                && (missing_value(doc, a) || is_empty_expression_value_directive(doc, a))
            {
                ctx.report(
                    a.span,
                    "'v-slot' directive on a custom element requires that attribute value.",
                );
            }
        }
    }
}

pub fn create(opts: &RuleOptions) -> Option<Box<dyn Rule>> {
    Some(Box::new(ValidVSlot {
        allow_modifiers: opts.get(0)["allowModifiers"].as_bool() == Some(true),
    }))
}
