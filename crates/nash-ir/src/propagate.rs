//! Atom propagation over typed ANF with globally unique, well-scoped binders.
use crate::{
    build::Builder,
    core::{Core, CoreKind},
};
use nash_plutus::constant::Constant;
use std::collections::HashMap;

/// Remove aliases, then inline single-use literals and approved repeated literals.
/// Literal counts are taken AFTER removing aliases to avoid duplicating a shared
/// payload through an alias chain. Integers, bytes up to 64 bytes and BLS
/// constants may be duplicated. Other ANF atoms (lambdas, delays and builtin
/// references) remain bound: their inlining/sharing belongs to separate rules.
/// Input must have globally unique IDs; substitutions introduce no binders and
/// preserve each occurrence's explicit type view. No computation is moved.
pub fn propagate<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    let mut aliases = HashMap::new();
    core.walk(&mut |node| {
        if let CoreKind::Let { binder, value, .. } = node.kind
            && let CoreKind::Var(target) = value.kind
        {
            // Preorder visits an alias target's binding before this binding.
            // Resolve once here rather than walking the chain at every use.
            let target = aliases.get(&target.unique).copied().unwrap_or(value);
            aliases.insert(binder.name.unique, target);
        }
    });
    let core = replace(b, core, &aliases);
    let mut uses: HashMap<u32, usize> = HashMap::new();
    core.walk(&mut |node| {
        if let CoreKind::Var(name) = node.kind {
            *uses.entry(name.unique).or_default() += 1;
        }
    });
    let mut literals = HashMap::new();
    core.walk(&mut |node| {
        if let CoreKind::Let { binder, value, .. } = node.kind
            && let CoreKind::Lit(literal) = value.kind
            && (uses.get(&binder.name.unique).copied().unwrap_or(0) <= 1 || can_duplicate(literal))
        {
            literals.insert(binder.name.unique, value);
        }
    });
    replace(b, core, &literals)
}

pub(crate) fn can_duplicate(literal: &Constant<'_>) -> bool {
    match literal {
        Constant::Integer(_)
        | Constant::Bls12_381G1Element(_)
        | Constant::Bls12_381G2Element(_)
        | Constant::Bls12_381MlResult(_) => true,
        Constant::ByteString(bytes) => bytes.len() <= 64,
        _ => false,
    }
}

fn replace<'a>(
    b: &Builder<'a>,
    core: &'a Core<'a>,
    values: &HashMap<u32, &'a Core<'a>>,
) -> &'a Core<'a> {
    if values.is_empty() {
        return core;
    }
    core.map(b, &mut |node| match node.kind {
        CoreKind::Var(name) => {
            let value = *values.get(&name.unique)?;
            Some(b.with_type(value, node.ty))
        }
        CoreKind::Let { binder, body, .. } if values.contains_key(&binder.name.unique) => {
            Some(b.with_type(body, node.ty))
        }
        _ => None,
    })
}

#[cfg(test)]
mod tests;
