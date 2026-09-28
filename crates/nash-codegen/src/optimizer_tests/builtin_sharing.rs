//! Chunk 5 force sharing: UPLC pairs, semantics and outermost placement.
use nash_ir::{
    build::Builder,
    core::*,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant, term::Term};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn binder<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn trace<'a>(b: &Builder<'a>, text: &'a str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, text)), value)
}
fn check<'a>(
    name: &str,
    b: &Builder<'a>,
    core: &'a Core<'a>,
    fails: bool,
    bindings: usize,
    apply: bool,
) {
    let core = crate::recursion::rewrite(b, core).unwrap();
    let before = crate::lower::lower(b.arena, core).unwrap();
    let after = crate::lower::lower_with_builtin_sharing(b.arena, core).unwrap();
    let evaluate = |term: &'a Term<'a, _>| {
        crate::harness::eval_named(
            b.arena,
            if apply {
                term.apply(b.arena, Term::integer_from(b.arena, 3))
            } else {
                term
            },
        )
    };
    let baseline = evaluate(before);
    let candidate = evaluate(after);

    assert_eq!(candidate.result.starts_with("error:"), fails);

    insta::assert_snapshot!(
        name,
        format!(
            "--- uplc before\n{}\n--- uplc after\n{}\n--- result\n{}\n--- logs\n{:?}",
            nash_plutus::pretty::term(before),
            nash_plutus::pretty::term(after),
            candidate.result,
            candidate.logs
        )
    );
    let mut rest = after;
    for _ in 0..bindings {
        let Term::Apply { function, argument } = rest else {
            panic!("outer binding")
        };
        let Term::Lambda { body, .. } = function else {
            panic!("binding lambda")
        };
        let mut value = *argument;
        let mut forces = 0;
        while let Term::Force(inner) = value {
            forces += 1;
            value = inner;
        }
        let Term::Builtin(func) = value else {
            panic!("only builtin values hoist")
        };
        assert_eq!(forces, func.force_count());
        assert!(forces > 0);
        rest = body;
    }
    if apply {
        assert!(matches!(rest, Term::Lambda { .. }));
    }
    // Properties independent of the expected snapshot.
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
    assert_eq!(
        nash_plutus::pretty::term(after),
        nash_plutus::pretty::term(crate::lower::lower_with_builtin_sharing(b.arena, core).unwrap())
    );
}
#[test]
fn one_use_outside_validator_arguments() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = binder(&b, "x", INT);
    check(
        "single",
        &b,
        b.lam(&[x], trace(&b, "called", b.var(x.name, INT))),
        false,
        1,
        true,
    );
}
#[test]
fn repeated_trace_preserves_argument_order() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "repeated",
        &b,
        b.builtin(
            F::AddInteger,
            &[trace(&b, "left", b.int(20)), trace(&b, "right", b.int(22))],
            INT,
        ),
        false,
        1,
        false,
    );
}
#[test]
fn unselected_failure_and_trace_stay_lazy() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = binder(&b, "x", INT);
    let body = b.case(
        CaseKind::Bool,
        b.lit(Constant::bool(&a, true)),
        &[
            Branch {
                test: Test::True,
                binders: &[],
                body: b.int(42),
            },
            Branch {
                test: Test::False,
                binders: &[],
                body: trace(&b, "never", b.error(INT)),
            },
        ],
        None,
        INT,
    );
    check("unselected", &b, b.lam(&[x], body), false, 1, true);
}
#[test]
fn argument_failure_is_not_hoisted() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "failure",
        &b,
        b.builtin(
            F::Trace,
            &[
                b.lit(Constant::string(&a, "never")),
                trace(&b, "argument", b.error(INT)),
            ],
            INT,
        ),
        true,
        1,
        false,
    );
}
#[test]
fn two_forces_and_multiple_type_instantiations() {
    let a = Arena::new();
    let b = Builder::new(&a);
    // mkPairData itself has no forces, while fstPair requires two.
    let data = b.builtin(F::IData, &[b.int(42)], Ty::Big(&nash_ir::ty::BigTy::Data));
    let pair = b.builtin(
        F::MkPairData,
        &[data, data],
        Ty::Const(a.alloc(ConstTy::Pair(data.ty, data.ty))),
    );
    let first = b.builtin(F::FstPair, &[pair], data.ty);
    check(
        "two_forces",
        &b,
        b.builtin(F::UnIData, &[first], INT),
        false,
        1,
        false,
    );
    let bool_ty = Ty::Const(&ConstTy::Bool);
    let left = b.builtin(
        F::IfThenElse,
        &[b.lit(Constant::bool(&a, true)), b.int(42), b.int(0)],
        INT,
    );
    let right = b.builtin(
        F::IfThenElse,
        &[
            b.lit(Constant::bool(&a, true)),
            b.lit(Constant::bool(&a, true)),
            b.lit(Constant::bool(&a, false)),
        ],
        bool_ty,
    );
    check(
        "polymorphic",
        &b,
        b.case(
            CaseKind::Bool,
            right,
            &[
                Branch {
                    test: Test::True,
                    binders: &[],
                    body: left,
                },
                Branch {
                    test: Test::False,
                    binders: &[],
                    body: b.int(0),
                },
            ],
            None,
            INT,
        ),
        false,
        1,
        false,
    );
}
#[test]
fn unforced_builtins_remain_unchanged() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "unforced",
        &b,
        b.builtin(F::AddInteger, &[b.int(20), b.int(22)], INT),
        false,
        0,
        false,
    );
}
#[test]
fn delayed_body_stays_delayed() {
    let a = Arena::new();
    let b = Builder::new(&a);
    check(
        "delay",
        &b,
        b.force(b.delay(trace(&b, "forced", b.int(42))), INT),
        false,
        1,
        false,
    );
}
#[test]
fn recursive_calls_share_one_reference() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let n = binder(&b, "n", INT);
    let f = binder(&b, "loop", Ty::Term(a.alloc(TermTy::Fun(&[INT], INT))));
    let recur = b.app(
        b.var(f.name, f.ty),
        &[b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT)],
        INT,
    );
    let body = b.case(
        CaseKind::Bool,
        b.builtin(
            F::EqualsInteger,
            &[b.var(n.name, INT), b.int(0)],
            Ty::Const(&ConstTy::Bool),
        ),
        &[
            Branch {
                test: Test::True,
                binders: &[],
                body: b.int(42),
            },
            Branch {
                test: Test::False,
                binders: &[],
                body: trace(&b, "tick", recur),
            },
        ],
        None,
        INT,
    );
    let root = b.let_rec(
        &[RecBinder {
            binder: f,
            params: a.alloc_slice_copy(&[n]),
            static_params: &[],
            body,
        }],
        b.app(b.var(f.name, f.ty), &[b.int(3)], INT),
    );
    check("recursion", &b, root, false, 1, false);
}

#[test]
fn data_case_and_trace_introduced_by_lowering() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let data_ty = Ty::Big(&nash_ir::ty::BigTy::Data);
    let x = binder(&b, "decoded", INT);
    let core = b.case(
        CaseKind::Data,
        b.builtin(F::IData, &[b.int(42)], data_ty),
        &[Branch {
            test: Test::DataI,
            binders: a.alloc_slice_copy(&[x]),
            body: trace(&b, "integer", b.var(x.name, INT)),
        }],
        Some(b.error(INT)),
        INT,
    );
    check("lowered_data", &b, core, false, 2, false);
}

#[test]
fn bare_and_partial_references_share_with_direct_calls() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let string = b.lit(Constant::string(&a, "partial"));
    let full_ty = Ty::Term(a.alloc(TermTy::Fun(a.alloc_slice_copy(&[string.ty, INT]), INT)));
    let partial_ty = Ty::Term(a.alloc(TermTy::Fun(&[INT], INT)));
    let bare = binder(&b, "bare", full_ty);
    let partial = binder(&b, "partial", partial_ty);
    let root = b.let_(
        bare,
        b.builtin(F::Trace, &[], full_ty),
        b.let_(
            partial,
            b.builtin(F::Trace, &[string], partial_ty),
            b.builtin(
                F::AddInteger,
                &[
                    b.app(
                        b.var(bare.name, full_ty),
                        &[b.lit(Constant::string(&a, "bare")), b.int(20)],
                        INT,
                    ),
                    b.app(b.var(partial.name, partial_ty), &[b.int(22)], INT),
                ],
                INT,
            ),
        ),
    );
    check("partial", &b, root, false, 1, false);
}

#[test]
fn discarded_defaults_do_not_create_unused_bindings() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let fallback = b.case(
        CaseKind::Bool,
        b.lit(Constant::bool(&a, true)),
        &[Branch {
            test: Test::True,
            binders: &[],
            body: trace(&b, "never", b.error(INT)),
        }],
        Some(b.error(INT)),
        INT,
    );
    let boolean = b.case(
        CaseKind::Bool,
        b.lit(Constant::bool(&a, true)),
        &[
            Branch {
                test: Test::True,
                binders: &[],
                body: b.int(42),
            },
            Branch {
                test: Test::False,
                binders: &[],
                body: b.int(0),
            },
        ],
        Some(fallback),
        INT,
    );
    check("discarded_bool_default", &b, boolean, false, 0, false);
    let data_ty = Ty::Big(&nash_ir::ty::BigTy::Data);
    let list_ty = Ty::Const(a.alloc(ConstTy::List(data_ty)));
    let head = binder(&b, "head", data_ty);
    let tail = binder(&b, "tail", list_ty);
    let nil = b.builtin(F::MkNilData, &[b.lit(Constant::unit(&a))], list_ty);
    let list = b.case(
        CaseKind::List,
        nil,
        &[
            Branch {
                test: Test::Nil,
                binders: &[],
                body: b.int(42),
            },
            Branch {
                test: Test::Cons,
                binders: a.alloc_slice_copy(&[head, tail]),
                body: b.int(0),
            },
        ],
        Some(fallback),
        INT,
    );
    check("discarded_list_default", &b, list, false, 0, false);
    let branches: Vec<_> = [
        Test::DataConstr,
        Test::DataMap,
        Test::DataList,
        Test::DataI,
        Test::DataB,
    ]
    .into_iter()
    .map(|test| Branch {
        test,
        binders: a.alloc_slice_copy(&[binder(&b, "unused", Ty::Erased)]),
        body: b.int(42),
    })
    .collect();
    let data = b.case(
        CaseKind::Data,
        b.builtin(F::IData, &[b.int(42)], data_ty),
        &branches,
        Some(fallback),
        INT,
    );
    check("discarded_data_default", &b, data, false, 1, false);
}
