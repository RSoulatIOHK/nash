//! Trial: shorten nonrecursive helper signatures with exact, direct calls only.
use crate::{
    build::Builder,
    core::{Binder, Core, CoreKind},
    ty::{RuntimeTy, TermTy, Ty},
};
use std::collections::HashSet;

fn signature<'a>(b: &Builder<'a>, ty: Ty<'a>, keep: &[bool]) -> Option<Ty<'a>> {
    let Ty::Term(TermTy::Fun(params, result)) = ty else {
        return None;
    };
    if params.len() != keep.len() {
        return None;
    }
    let params: Vec<_> = params
        .iter()
        .zip(keep)
        .filter_map(|(p, keep)| keep.then_some(*p))
        .collect();
    Some(if params.is_empty() {
        Ty::Runtime(b.arena.alloc(RuntimeTy::Delay(*result)))
    } else {
        Ty::Term(
            b.arena
                .alloc(TermTy::Fun(b.arena.alloc_slice_copy(&params), *result)),
        )
    })
}

/// Requires typed ANF and globally unique, well-scoped binders. Every use must
/// be a direct application with exactly the original arity. Partial, escaping,
/// oversaturated or unsupported signature views leave the helper unchanged.
/// ANF arguments are safe-to-discard atoms; their preceding strict computations
/// remain in place. This pass neither removes those lets nor changes LetRec
/// parameters. All-unused helpers become delays, forced separately at each call.
pub fn reduce<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> &'a Core<'a> {
    core.map(b, &mut |node| {
        let CoreKind::Let {
            binder,
            value,
            body,
        } = node.kind
        else {
            return None;
        };
        let CoreKind::Lam {
            params,
            body: lambda_body,
        } = value.kind
        else {
            return None;
        };
        let mut used = HashSet::new();
        lambda_body.walk(&mut |node| {
            if let CoreKind::Var(name) = node.kind {
                used.insert(name.unique);
            }
        });
        let keep: Vec<_> = params
            .iter()
            .map(|p| used.contains(&p.name.unique))
            .collect();
        if keep.iter().all(|keep| *keep) {
            return None;
        }
        let (mut uses, mut calls) = (0usize, 0usize);
        body.walk(&mut |node| match node.kind {
            CoreKind::Var(name) if name.unique == binder.name.unique => uses += 1,
            CoreKind::App { func, args }
                if matches!(func.kind, CoreKind::Var(name) if name.unique == binder.name.unique)
                    && args.len() == params.len()
                    && matches!(func.ty, Ty::Term(TermTy::Fun(types, _)) if types.len() == keep.len()) =>
            {
                calls += 1
            }
            _ => {}
        });
        if calls == 0 || calls != uses {
            return None;
        }
        let binder_ty = signature(b, binder.ty, &keep)?;
        let value_ty = signature(b, value.ty, &keep)?;
        let params: Vec<_> = params
            .iter()
            .zip(&keep)
            .filter_map(|(p, keep)| keep.then_some(*p))
            .collect();
        let value = b.with_type(
            if params.is_empty() {
                b.delay(lambda_body)
            } else {
                b.lam(&params, lambda_body)
            },
            value_ty,
        );
        let body = body.map(b, &mut |call| {
            let CoreKind::App { func, args } = call.kind else {
                return None;
            };
            let CoreKind::Var(name) = func.kind else {
                return None;
            };
            if name.unique != binder.name.unique {
                return None;
            }
            let func = b.var(
                name,
                signature(b, func.ty, &keep).expect("prechecked signature"),
            );
            let args: Vec<_> = args
                .iter()
                .zip(&keep)
                .filter_map(|(arg, keep)| keep.then_some(*arg))
                .collect();
            Some(if args.is_empty() {
                b.force(func, call.ty)
            } else {
                b.app(func, &args, call.ty)
            })
        });
        Some(b.with_type(
            b.let_(
                Binder {
                    ty: binder_ty,
                    ..binder
                },
                value,
                body,
            ),
            node.ty,
        ))
    })
}
