//! Conservative removal of unused nonrecursive bindings.
use crate::{
    analysis,
    build::Builder,
    core::{Core, CoreKind},
};
use std::{collections::HashSet, ptr};

/// Requires globally unique, well-scoped binders. Iterate so discarding an unused
/// closure or alias can expose dead bindings that supplied its captured values.
/// Calls, recursive groups and parameters are not eliminated by this pass.
pub fn simplify<'a>(b: &Builder<'a>, mut core: &'a Core<'a>) -> &'a Core<'a> {
    loop {
        let mut used = HashSet::new();
        core.walk(&mut |node| {
            if let CoreKind::Var(name) = node.kind {
                used.insert(name.unique);
            }
        });
        let next = core.map(b, &mut |node| {
            if let CoreKind::Let {
                binder,
                value,
                body,
            } = node.kind
                && !used.contains(&binder.name.unique)
                && analysis::safe_to_discard(value)
            {
                return Some(b.with_type(body, node.ty));
            }
            None
        });
        if ptr::eq(core, next) {
            return next;
        }
        core = next;
    }
}
