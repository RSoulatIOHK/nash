//! Trial: cancel only a syntactically adjacent force/delay pair.
use crate::{
    build::Builder,
    core::{Core, CoreKind},
};

/// Replace `force (delay body)` at the force's original evaluation point.
/// No substitution, duplication, hoisting, or inverse `delay (force x)` rule.
/// Bottom-up traversal also removes nested adjacent pairs. Preserve the outer
/// type view. This accepts nested Core; subsequent cleanup may flatten lets
/// exposed in computation positions, without another ANF normalization.
pub fn reduce<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    core.map(b, &mut |node| match node.kind {
        CoreKind::Force(value) => match value.kind {
            CoreKind::Delay(body) => Some(b.with_type(body, node.ty)),
            _ => None,
        },
        _ => None,
    })
}
