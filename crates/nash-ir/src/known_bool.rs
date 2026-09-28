//! Trial: select branches only for literal Boolean subjects and valid Boolean tables.
use crate::{
    build::Builder,
    core::{CaseKind, Core, CoreKind, Test},
};
use nash_plutus::constant::Constant;

/// No evaluation or substitution: the literal subject is already a value.
/// Preserve malformed tables and missing matches for the lowerer/runtime.
/// Keep the enclosing result type view when selecting a branch or default.
pub fn reduce<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
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
