//! Known-subject case folding. Constructor folding remains a standalone trial.
use crate::{
    build::Builder,
    core::{CaseKind, Core, CoreKind, Test},
};
use nash_plutus::constant::Constant;

/// No evaluation or substitution: the literal subject is already a value.
/// Preserve malformed tables and missing matches for the lowerer/runtime.
/// Keep the enclosing result type view when selecting a branch or default.
pub fn reduce_bool<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    core.map(b, &mut |node| {
        let CoreKind::Case {
            kind: CaseKind::Bool,
            scrutinee,
            branches,
            default,
        } = node.kind
        else {
            return None;
        };
        let CoreKind::Lit(Constant::Boolean(value)) = scrutinee.kind else {
            return None;
        };
        let (mut yes, mut no) = (None, None);
        for branch in branches {
            if !branch.binders.is_empty() {
                return None;
            }
            let slot = match branch.test {
                Test::True => &mut yes,
                Test::False => &mut no,
                _ => return None,
            };
            if slot.is_some() {
                return None;
            }
            *slot = Some(branch.body);
        }
        let selected = (if *value { yes } else { no }).or(default)?;
        Some(b.with_type(selected, node.ty))
    })
}

/// Input binders must be globally unique and well scoped, as for beta reduction.
/// Keep every field strict, in source order, even when its binder is unused.
/// Leave invalid tables, absent tags and application arity mismatches untouched.
pub fn reduce_constr<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    core.map(b, &mut |node| {
        let CoreKind::Case {
            kind: CaseKind::Tag,
            scrutinee,
            branches,
            default: None,
        } = node.kind
        else {
            return None;
        };
        let CoreKind::Constr { tag, fields } = scrutinee.kind else {
            return None;
        };
        let mut seen = vec![false; branches.len()];
        for branch in branches {
            let Test::Tag(tag) = branch.test else {
                return None;
            };
            let slot = seen.get_mut(usize::from(tag))?;
            if *slot {
                return None;
            }
            *slot = true;
        }
        let selected = branches
            .iter()
            .find(|branch| branch.test == Test::Tag(tag))?;
        if selected.binders.len() != fields.len() {
            return None;
        }
        let mut body = selected.body;
        for (&binder, &field) in selected.binders.iter().zip(fields).rev() {
            body = b.let_(binder, field, body);
        }
        Some(b.with_type(body, node.ty))
    })
}
