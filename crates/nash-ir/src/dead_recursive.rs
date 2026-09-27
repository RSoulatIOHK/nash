//! Remove recursive members unreachable from their group's continuation.
use crate::{
    build::Builder,
    core::{Core, CoreKind},
};
use std::collections::HashMap;

fn mark(
    core: &Core<'_>,
    indices: &HashMap<u32, usize>,
    live: &mut [bool],
    pending: &mut Vec<usize>,
) {
    core.walk(&mut |node| {
        if let CoreKind::Var(name) = node.kind
            && let Some(&index) = indices.get(&name.unique)
            && !live[index]
        {
            live[index] = true;
            pending.push(index);
        }
    });
}

/// Input must have globally unique, well-scoped binders. Every reference counts,
/// including partial applications, escaping values and suspended captures.
/// Function bodies are deferred; the supported singleton delayed worker is too.
/// No call-shape analysis, static-parameter inference or parameter removal occurs.
pub fn prune<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    core.map(b, &mut |node| {
        let CoreKind::LetRec { binders, body } = node.kind else {
            return None;
        };
        // Other zero-parameter groups are unsupported recursive values, not
        // suspended function definitions. Preserve their lowering error.
        if binders.iter().any(|r| r.params.is_empty())
            && !(binders.len() == 1 && matches!(binders[0].body.kind, CoreKind::Delay(_)))
        {
            return None;
        }
        let indices = binders
            .iter()
            .enumerate()
            .map(|(i, r)| (r.binder.name.unique, i))
            .collect();
        let mut live = vec![false; binders.len()];
        let mut pending = Vec::new();
        mark(body, &indices, &mut live, &mut pending);
        while let Some(index) = pending.pop() {
            mark(binders[index].body, &indices, &mut live, &mut pending);
        }
        let kept: Vec<_> = binders
            .iter()
            .zip(&live)
            .filter_map(|(r, live)| live.then_some(*r))
            .collect();
        if kept.len() == binders.len() && !kept.is_empty() {
            return None;
        }
        Some(b.with_type(
            if kept.is_empty() {
                body
            } else {
                b.let_rec(&kept, body)
            },
            node.ty,
        ))
    })
}
