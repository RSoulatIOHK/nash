use super::*;
use crate::{
    pretty::pretty,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::arena::Arena;

fn binder(unique: u32, text: &str) -> Binder<'_> {
    Binder {
        name: Name { text, unique },
        ty: Ty::Erased,
    }
}

#[test]
fn substitution_preserves_an_explicit_coercion_view() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&crate::ty::ConstTy::Int);
    let bytes = Ty::Const(&crate::ty::ConstTy::Bytes);
    let x = b.fresh("coerce");
    let coerced = b.with_type(b.var(x, int), bytes);
    let result = substitute(&b, coerced, x.unique, b.int(42));
    assert_eq!(result.ty, bytes);
    assert!(matches!(result.kind, CoreKind::Lit(_)));
    assert!(validate(result, &[]).is_ok());
}

#[test]
fn scope_diagnostics() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(10, "x");
    let y = binder(11, "y");
    let tree = b.let_(
        x,
        b.var(x.name, x.ty),
        crate::test_support::constr(
            &b,
            0,
            &[b.lam(&[x], b.var(x.name, x.ty)), b.var(y.name, y.ty)],
        ),
    );
    insta::assert_debug_snapshot!(validate(tree, &[]).expect_err("invalid scope"));
    assert!(validate(b.var(y.name, y.ty), &[y.name]).is_ok());
}

#[test]
fn freshening_preserves_shadowing_and_numeric_identity() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = Binder {
        ty: Ty::Const(&ConstTy::Int),
        ..binder(1, "x")
    };
    let renamed_text = Name {
        text: "display_only",
        unique: 1,
    };
    let tree = b.let_(
        x,
        b.int(3),
        crate::test_support::constr(
            &b,
            0,
            &[
                b.var(renamed_text, x.ty),
                b.lam(&[x], b.var(x.name, x.ty)),
                b.var(x.name, x.ty),
            ],
        ),
    );
    let result = freshen(&b, tree);
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn substitution_avoids_capture_and_freshens_each_copy() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let y = binder(2, "y");
    let z = binder(3, "z");
    let replacement = b.lam(
        &[z],
        crate::test_support::constr(&b, 0, &[b.var(y.name, y.ty), b.var(z.name, z.ty)]),
    );
    let x = Binder {
        ty: replacement.ty,
        ..binder(1, "x")
    };
    let tree = b.lam(
        &[y],
        crate::test_support::constr(&b, 0, &[b.var(x.name, x.ty), b.var(x.name, x.ty)]),
    );
    let result = substitute(&b, tree, x.name.unique, replacement);
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[y.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn substitution_respects_let_rhs_and_shadowed_body() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = Binder {
        ty: Ty::Const(&ConstTy::Int),
        ..binder(1, "x")
    };
    let tree = b.let_(x, b.var(x.name, x.ty), b.var(x.name, x.ty));
    let result = substitute(&b, tree, 1, b.int(42));
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn recursive_group_scope_is_simultaneous_and_parameters_are_local() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let function = Ty::Term(&TermTy::Fun(&[Ty::Erased], Ty::Erased));
    let f = Binder {
        ty: function,
        ..binder(1, "f")
    };
    let g = Binder {
        ty: function,
        ..binder(2, "g")
    };
    let p = binder(3, "p");
    let tree = b.let_rec(
        &[
            RecBinder {
                binder: f,
                params: arena.alloc_slice_copy(&[p]),
                static_params: &[0],
                body: b.app(b.var(g.name, g.ty), &[b.var(p.name, p.ty)], Ty::Erased),
            },
            RecBinder {
                binder: g,
                params: &[],
                static_params: &[],
                body: b.var(f.name, f.ty),
            },
        ],
        b.var(g.name, g.ty),
    );
    assert!(validate(tree, &[]).is_ok());
    let result = substitute(&b, tree, g.name.unique, b.error(g.ty));
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
    let leaking = b.let_rec(
        &[
            RecBinder {
                binder: f,
                params: arena.alloc_slice_copy(&[p]),
                static_params: &[],
                body: b.var(p.name, p.ty),
            },
            RecBinder {
                binder: g,
                params: &[],
                static_params: &[],
                body: b.var(p.name, p.ty),
            },
        ],
        b.var(p.name, p.ty),
    );
    insta::assert_debug_snapshot!(validate(leaking, &[]).expect_err("invalid scope"));
}

#[test]
fn branch_binders_do_not_scope_over_scrutinee_siblings_or_default() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = Binder {
        ty: Ty::Const(&ConstTy::Int),
        ..binder(1, "x")
    };
    let tree = b.case(
        CaseKind::Tag,
        b.var(x.name, x.ty),
        &[
            Branch {
                test: Test::Tag(0),
                binders: arena.alloc_slice_copy(&[x]),
                body: b.var(x.name, x.ty),
            },
            Branch {
                test: Test::Tag(1),
                binders: &[],
                body: b.var(x.name, x.ty),
            },
        ],
        Some(b.var(x.name, x.ty)),
        x.ty,
    );
    insta::assert_debug_snapshot!(validate(tree, &[]).expect_err("invalid scope"));
    let result = substitute(&b, tree, 1, b.int(42));
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn duplicate_ids_include_disjoint_scopes_and_external_names() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let y = binder(1, "y");
    let tree = crate::test_support::constr(
        &b,
        0,
        &[
            b.lam(&[x], b.var(x.name, x.ty)),
            b.lam(&[y], b.var(y.name, y.ty)),
        ],
    );
    insta::assert_debug_snapshot!(validate(tree, &[x.name]).expect_err("invalid scope"));
}

#[test]
fn freshening_reserves_free_ids_and_does_not_fix_unbound_variables() {
    let arena = Arena::new();
    let original = Builder::new(&arena);
    let x = binder(10, "x");
    let free = binder(1, "free");
    let tree = original.lam(
        &[x],
        crate::test_support::constr(
            &original,
            0,
            &[original.var(x.name, x.ty), original.var(free.name, free.ty)],
        ),
    );
    let separate_supply = Builder::new(&arena);
    let result = freshen(&separate_supply, tree);
    assert_eq!(
        validate(result, &[]),
        Err(vec![HygieneError::UnboundVariable(free.name)])
    );
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[free.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn replacement_is_not_substituted_recursively() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let tree = b.var(x.name, x.ty);
    let replacement = b.trace(
        b.lit(nash_plutus::constant::Constant::string(
            &arena,
            "replacement",
        )),
        b.var(x.name, x.ty),
    );
    let result = substitute(&b, tree, 1, replacement);
    assert_eq!(result.ty, tree.ty);
    assert!(validate(result, &[x.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}
