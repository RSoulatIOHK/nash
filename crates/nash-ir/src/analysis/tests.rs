use super::*;
use crate::{build::Builder, core::*, ty::Ty};
use nash_plutus::{arena::Arena, builtin::DefaultFunction};

fn binder(name: Name<'_>) -> Binder<'_> {
    Binder {
        name,
        ty: Ty::Erased,
    }
}

#[test]
fn shadowing_and_rhs_scope() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = b.fresh("x");
    let term = b.let_(binder(x), b.var(x), b.let_(binder(x), b.var(x), b.var(x)));
    let uses = occurrences(term);
    assert_eq!(
        uses.uses.iter().map(|u| u.binding).collect::<Vec<_>>(),
        vec![None, Some(0), Some(1)]
    );
    let renamed = Name {
        text: "different spelling",
        unique: x.unique,
    };
    assert_eq!(
        free_variables(b.constr(0, &[b.var(x), b.var(renamed)])),
        vec![x]
    );
    insta::assert_debug_snapshot!(uses);
}

#[test]
fn branch_and_suspension_scopes() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(b.fresh("x"));
    let p = binder(b.fresh("p"));
    let term = b.lam(
        &[x],
        b.case(
            CaseKind::Tag,
            b.var(x.name),
            &[
                Branch {
                    test: Test::Tag(0),
                    binders: arena.alloc_slice_copy(&[p]),
                    body: b.delay(b.var(x.name)),
                },
                Branch {
                    test: Test::Tag(1),
                    binders: &[],
                    body: b.lam(&[], b.var(p.name)),
                },
            ],
            Some(b.var(x.name)),
        ),
    );
    let found = occurrences(term);
    assert_eq!(found.bindings[0].execution_scope, vec![Boundary::Lambda(0)]);
    assert_eq!(
        found.bindings[1].execution_scope,
        vec![
            Boundary::Lambda(0),
            Boundary::Branch {
                node: 1,
                index: Some(0)
            }
        ]
    );
    assert_eq!(found.uses[2].binding, None);
    assert_eq!(found.uses[2].execution_scope.len(), 2); // empty lambda adds no boundary
    assert_ne!(found.uses[2].execution_scope, found.uses[3].execution_scope);
    insta::assert_debug_snapshot!(found);
}

#[test]
fn recursive_group_scope_and_size() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let f = binder(b.fresh("f"));
    let g = binder(b.fresh("g"));
    let x = binder(b.fresh("x"));
    let term = b.let_rec(
        &[
            RecBinder {
                binder: f,
                params: arena.alloc_slice_copy(&[x]),
                static_params: &[],
                body: b.app(b.var(g.name), &[b.var(x.name)]),
            },
            RecBinder {
                binder: g,
                params: &[],
                static_params: &[],
                body: b.var(f.name),
            },
        ],
        b.var(x.name),
    );
    assert_eq!(free_variables(term), vec![x.name]);
    assert!(!safe_to_discard(term));
    assert_eq!(
        size_estimate(term),
        CoreSize {
            nodes: 6,
            binders: 3
        }
    );
    let found = occurrences(term);
    assert_eq!(
        found.bindings[2].execution_scope,
        vec![Boundary::RecursiveBody { node: 0, index: 0 }]
    );
    assert_eq!(found.uses[0].scope, vec![0, 1, 2]);
    insta::assert_debug_snapshot!(found);
}

#[test]
fn discard_respects_strict_evaluation() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = binder(b.fresh("x"));
    let cases = [
        ("literal", b.int(1), true),
        ("bound variable", b.var(x.name), true),
        ("error", b.error(), false),
        ("trace", b.trace(b.int(1), b.int(2)), false),
        ("lambda", b.lam(&[x], b.error()), true),
        ("empty lambda", b.lam(&[], b.error()), false),
        ("delay", b.delay(b.error()), true),
        ("force", b.force(b.delay(b.error())), false),
        (
            "partial builtin",
            b.builtin(DefaultFunction::AddInteger, &[b.int(1)]),
            true,
        ),
        (
            "strict partial builtin",
            b.builtin(DefaultFunction::AddInteger, &[b.error()]),
            false,
        ),
        (
            "saturated builtin",
            b.builtin(DefaultFunction::AddInteger, &[b.int(1), b.int(2)]),
            false,
        ),
        ("strict constructor", b.constr(0, &[b.error()]), false),
        ("value constructor", b.constr(0, &[b.int(1)]), true),
        (
            "possibly divergent call",
            b.app(b.var(x.name), &[b.var(x.name)]),
            false,
        ),
        ("strict let", b.let_(x, b.error(), b.int(0)), false),
    ];
    for (label, term, expected) in cases {
        assert_eq!(safe_to_discard(term), expected, "{label}");
    }
}

#[test]
fn shared_subtrees_count_per_occurrence() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let x = b.var(b.fresh("x"));
    let term = b.constr(0, &[x, x]);
    assert_eq!(
        size_estimate(term),
        CoreSize {
            nodes: 3,
            binders: 0
        }
    );
    let found = occurrences(term);
    assert_ne!(found.uses[0].node, found.uses[1].node);
}
