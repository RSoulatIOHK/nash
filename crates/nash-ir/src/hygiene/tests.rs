use super::*;
use crate::{pretty::pretty, ty::Ty};
use nash_plutus::arena::Arena;

fn binder(unique: u32, text: &str) -> Binder<'_> {
    Binder {
        name: Name { text, unique },
        ty: Ty::Erased,
    }
}

#[test]
fn scope_diagnostics() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(10, "x");
    let y = binder(11, "y");
    let tree = b.let_(
        x,
        b.var(x.name),
        b.constr(0, &[b.lam(&[x], b.var(x.name)), b.var(y.name)]),
    );
    insta::assert_debug_snapshot!(validate(tree, &[]));
    assert!(validate(b.var(y.name), &[y.name]).is_ok());
}

#[test]
fn freshening_preserves_shadowing_and_numeric_identity() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let renamed_text = Name {
        text: "display_only",
        unique: 1,
    };
    let tree = b.let_(
        x,
        b.int(3),
        b.constr(
            0,
            &[
                b.var(renamed_text),
                b.lam(&[x], b.var(x.name)),
                b.var(x.name),
            ],
        ),
    );
    let result = freshen(&b, tree);
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn substitution_avoids_capture_and_freshens_each_copy() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let y = binder(2, "y");
    let z = binder(3, "z");
    let replacement = b.lam(&[z], b.constr(0, &[b.var(y.name), b.var(z.name)]));
    let tree = b.lam(&[y], b.constr(0, &[b.var(x.name), b.var(x.name)]));
    let result = substitute(&b, tree, x.name.unique, replacement);
    assert!(validate(result, &[y.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn substitution_respects_let_rhs_and_shadowed_body() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let tree = b.let_(x, b.var(x.name), b.var(x.name));
    let result = substitute(&b, tree, 1, b.int(42));
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn recursive_group_scope_is_simultaneous_and_parameters_are_local() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let f = binder(1, "f");
    let g = binder(2, "g");
    let p = binder(3, "p");
    let tree = b.let_rec(
        &[
            RecBinder {
                binder: f,
                params: arena.alloc_slice_copy(&[p]),
                static_params: &[0],
                body: b.app(b.var(g.name), &[b.var(p.name)]),
            },
            RecBinder {
                binder: g,
                params: &[],
                static_params: &[],
                body: b.var(f.name),
            },
        ],
        b.var(g.name),
    );
    assert!(validate(tree, &[]).is_ok());
    let result = substitute(&b, tree, g.name.unique, b.error());
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
    let leaking = b.let_rec(
        &[
            RecBinder {
                binder: f,
                params: arena.alloc_slice_copy(&[p]),
                static_params: &[],
                body: b.var(p.name),
            },
            RecBinder {
                binder: g,
                params: &[],
                static_params: &[],
                body: b.var(p.name),
            },
        ],
        b.var(p.name),
    );
    insta::assert_debug_snapshot!(validate(leaking, &[]));
}

#[test]
fn branch_binders_do_not_scope_over_scrutinee_siblings_or_default() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let tree = b.case(
        CaseKind::Tag,
        b.var(x.name),
        &[
            Branch {
                test: Test::Tag(0),
                binders: arena.alloc_slice_copy(&[x]),
                body: b.var(x.name),
            },
            Branch {
                test: Test::Tag(1),
                binders: &[],
                body: b.var(x.name),
            },
        ],
        Some(b.var(x.name)),
    );
    insta::assert_debug_snapshot!(validate(tree, &[]));
    let result = substitute(&b, tree, 1, b.int(42));
    assert!(validate(result, &[]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn duplicate_ids_include_disjoint_scopes_and_external_names() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let y = binder(1, "y");
    let tree = b.constr(0, &[b.lam(&[x], b.var(x.name)), b.lam(&[y], b.var(y.name))]);
    insta::assert_debug_snapshot!(validate(tree, &[x.name]));
}

#[test]
fn freshening_reserves_free_ids_and_does_not_fix_unbound_variables() {
    let arena = Arena::new();
    let original = Builder::new(&arena);
    let x = binder(10, "x");
    let free = binder(1, "free");
    let tree = original.lam(
        &[x],
        original.constr(0, &[original.var(x.name), original.var(free.name)]),
    );
    let separate_supply = Builder::new(&arena);
    let result = freshen(&separate_supply, tree);
    assert_eq!(
        validate(result, &[]),
        Err(vec![HygieneError::UnboundVariable(free.name)])
    );
    assert!(validate(result, &[free.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}

#[test]
fn replacement_is_not_substituted_recursively() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(1, "x");
    let result = substitute(&b, b.var(x.name), 1, b.delay(b.var(x.name)));
    assert!(validate(result, &[x.name]).is_ok());
    insta::assert_snapshot!(pretty(result));
}
