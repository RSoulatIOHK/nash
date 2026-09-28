//! Rule 4 semantic snapshots, without performance assertions.
use nash_ir::{
    anf,
    build::Builder,
    core::*,
    hygiene,
    pretty::pretty,
    small_inline,
    ty::{ConstTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn bind<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn call<'a>(b: &Builder<'a>, f: Binder<'a>, args: &[&'a Core<'a>]) -> &'a Core<'a> {
    b.app(b.var(f.name, f.ty), args, INT)
}
fn trace<'a>(b: &Builder<'a>, text: &'a str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, text)), value)
}
fn check<'a>(name: &str, b: &Builder<'a>, core: &'a Core<'a>, fails: bool) -> &'a Core<'a> {
    let before = nash_ir::single_use::simplify(b, anf::normalize(b, core));
    // A fresh name supply must also be safe when the input already owns IDs.
    let optimizer = Builder::new(b.arena);
    let after = small_inline::simplify(&optimizer, before);

    let baseline =
        crate::harness::eval_core_raw(b.arena, crate::recursion::rewrite(b, before).unwrap());
    let candidate =
        crate::harness::eval_core_raw(b.arena, crate::recursion::rewrite(b, after).unwrap());

    assert_eq!(candidate.result.starts_with("error:"), fails);
    insta::assert_snapshot!(
        name,
        crate::harness::pass_snapshot(
            b.arena,
            core,
            format!(
                "--- core before\n{}\n--- baseline uplc\n{}\n--- core after\n{}\n--- optimized uplc\n{}\n--- result\n{}\n--- logs\n{:?}",
                pretty(before),
                baseline.uplc,
                pretty(after),
                candidate.uplc,
                candidate.result,
                candidate.logs
            )
        )
    );
    for term in [before, after] {
        anf::validate(term).unwrap();
        hygiene::validate(term, &[]).unwrap();
        assert_eq!(term.ty, core.ty);
    }
    // Properties independent of the expected snapshot.
    assert!(std::ptr::eq(
        after,
        small_inline::simplify(&optimizer, after)
    ));
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
    after
}
#[test]
fn repeated_identity() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(&[x], b.var(x.name, INT));
    let f = bind(&b, "identity", value.ty);
    let root = b.let_(
        f,
        value,
        b.builtin(
            F::AddInteger,
            &[call(&b, f, &[b.int(20)]), call(&b, f, &[b.int(22)])],
            INT,
        ),
    );
    check("identity", &b, root, false);
}
#[test]
fn captured_computation_runs_once() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let offset = bind(&b, "offset", INT);
    let x = bind(&b, "x", INT);
    let value = b.lam(
        &[x],
        b.builtin(
            F::AddInteger,
            &[b.var(offset.name, INT), b.var(x.name, INT)],
            INT,
        ),
    );
    let f = bind(&b, "adjust", value.ty);
    check(
        "captured_once",
        &b,
        b.let_(
            offset,
            trace(&b, "offset", b.int(10)),
            b.let_(
                f,
                value,
                b.builtin(
                    F::AddInteger,
                    &[
                        call(&b, f, &[trace(&b, "first", b.int(1))]),
                        call(&b, f, &[trace(&b, "second", b.int(2))]),
                    ],
                    INT,
                ),
            ),
        ),
        false,
    );
}
#[test]
fn unused_argument_remains_strict() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "unused", INT);
    let value = b.lam(
        &[x, y],
        b.builtin(F::AddInteger, &[b.var(x.name, INT), b.int(1)], INT),
    );
    let f = bind(&b, "increment", value.ty);
    check(
        "unused_failure",
        &b,
        b.let_(
            f,
            value,
            b.builtin(
                F::AddInteger,
                &[
                    call(&b, f, &[b.int(1), trace(&b, "failure", b.error(INT))]),
                    call(&b, f, &[trace(&b, "unreached", b.int(1)), b.int(2)]),
                ],
                INT,
            ),
        ),
        true,
    );
}
#[test]
fn unselected_call_stays_unselected() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(
        &[x],
        b.builtin(F::AddInteger, &[b.var(x.name, INT), b.int(1)], INT),
    );
    let f = bind(&b, "increment", value.ty);
    check(
        "unselected",
        &b,
        b.let_(
            f,
            value,
            b.if_(
                b.lit(Constant::bool(&a, true)),
                call(&b, f, &[trace(&b, "chosen", b.int(41))]),
                call(&b, f, &[trace(&b, "cold", b.error(INT))]),
            ),
        ),
        false,
    );
}
#[test]
fn conditional_body_is_excluded() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(
        &[x],
        b.if_(
            b.lit(Constant::bool(&a, true)),
            b.var(x.name, INT),
            b.int(0),
        ),
    );
    let f = bind(&b, "conditional", value.ty);
    check(
        "conditional_excluded",
        &b,
        b.let_(
            f,
            value,
            b.builtin(
                F::AddInteger,
                &[call(&b, f, &[b.int(20)]), call(&b, f, &[b.int(22)])],
                INT,
            ),
        ),
        false,
    );
}
#[test]
fn partial_calls_stay_shared_while_full_call_inlines() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let value = b.lam(
        &[x, y],
        b.builtin(
            F::AddInteger,
            &[b.var(x.name, INT), b.var(y.name, INT)],
            INT,
        ),
    );
    let f = bind(&b, "add", value.ty);
    let partial_ty = b.lam(&[y], b.var(y.name, INT)).ty;
    let partial = |v| {
        b.app(
            b.app(b.var(f.name, f.ty), &[b.int(v)], partial_ty),
            &[b.int(1)],
            INT,
        )
    };
    check(
        "partial_calls",
        &b,
        b.let_(
            f,
            value,
            b.builtin(
                F::AddInteger,
                &[
                    partial(10),
                    b.builtin(
                        F::AddInteger,
                        &[partial(20), call(&b, f, &[b.int(3), b.int(4)])],
                        INT,
                    ),
                ],
                INT,
            ),
        ),
        false,
    );
}
#[test]
fn repeated_and_reordered_parameters_preserve_argument_order() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let value = b.lam(
        &[x, y],
        b.builtin(
            F::SubtractInteger,
            &[b.var(y.name, INT), b.var(x.name, INT)],
            INT,
        ),
    );
    let f = bind(&b, "subtract", value.ty);
    let duplicate = b.lam(
        &[x],
        b.builtin(
            F::AddInteger,
            &[b.var(x.name, INT), b.var(x.name, INT)],
            INT,
        ),
    );
    let g = bind(&b, "twice", duplicate.ty);
    // Use independent parameter IDs for independently bound lambdas.
    let duplicate = hygiene::freshen(&b, duplicate);
    check(
        "argument_order",
        &b,
        b.let_(
            f,
            value,
            b.let_(
                g,
                duplicate,
                b.builtin(
                    F::AddInteger,
                    &[
                        call(
                            &b,
                            f,
                            &[trace(&b, "first", b.int(2)), trace(&b, "second", b.int(5))],
                        ),
                        b.builtin(
                            F::AddInteger,
                            &[
                                call(&b, f, &[b.int(1), b.int(4)]),
                                b.builtin(
                                    F::AddInteger,
                                    &[
                                        call(&b, g, &[trace(&b, "once", b.int(10))]),
                                        call(&b, g, &[b.int(11)]),
                                    ],
                                    INT,
                                ),
                            ],
                            INT,
                        ),
                    ],
                    INT,
                ),
            ),
        ),
        false,
    );
}
#[test]
fn builtin_failure_stays_at_call() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(
        &[x],
        b.builtin(F::DivideInteger, &[b.int(10), b.var(x.name, INT)], INT),
    );
    let f = bind(&b, "divide", value.ty);
    check(
        "builtin_failure",
        &b,
        b.let_(
            f,
            value,
            b.builtin(
                F::AddInteger,
                &[
                    call(&b, f, &[trace(&b, "zero", b.int(0))]),
                    call(&b, f, &[trace(&b, "cold", b.int(1))]),
                ],
                INT,
            ),
        ),
        true,
    );
}
#[test]
fn literal_payload_boundary() {
    for length in [64, 65] {
        let a = Arena::new();
        let b = Builder::new(&a);
        let payload = b.lit(Constant::byte_string(
            &a,
            a.alloc_slice_copy(&vec![7u8; length]),
        ));
        let x = bind(&b, "x", payload.ty);
        let value = b.lam(&[x], b.builtin(F::LengthOfByteString, &[payload], INT));
        let f = bind(&b, "length", value.ty);
        check(
            if length == 64 {
                "literal_64"
            } else {
                "literal_65"
            },
            &b,
            b.let_(
                f,
                value,
                b.builtin(
                    F::AddInteger,
                    &[call(&b, f, &[payload]), call(&b, f, &[payload])],
                    INT,
                ),
            ),
            false,
        );
    }
}
#[test]
fn escaping_uses_keep_shared_definition() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(&[x], b.var(x.name, INT));
    let f = bind(&b, "identity", value.ty);
    let indirect = || {
        b.app(
            b.if_(
                b.lit(Constant::bool(&a, true)),
                b.var(f.name, f.ty),
                b.var(f.name, f.ty),
            ),
            &[b.int(20)],
            INT,
        )
    };
    check(
        "escaping",
        &b,
        b.let_(
            f,
            value,
            b.builtin(F::AddInteger, &[indirect(), call(&b, f, &[b.int(22)])], INT),
        ),
        false,
    );
}
#[test]
fn wrapper_in_recursive_body() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(
        &[x],
        b.builtin(F::AddInteger, &[b.var(x.name, INT), b.int(1)], INT),
    );
    let f = bind(&b, "increment", value.ty);
    let n = bind(&b, "n", INT);
    let unary = b.lam(&[n], b.var(n.name, INT)).ty;
    let worker = bind(&b, "worker", unary);
    let body = b.if_(
        b.builtin(
            F::EqualsInteger,
            &[b.var(n.name, INT), b.int(0)],
            Ty::Const(&ConstTy::Bool),
        ),
        b.int(0),
        call(
            &b,
            f,
            &[call(
                &b,
                worker,
                &[b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT)],
            )],
        ),
    );
    check(
        "recursive_wrapper",
        &b,
        b.let_(
            f,
            value,
            b.let_rec(
                &[RecBinder {
                    binder: worker,
                    params: a.alloc_slice_copy(&[n]),
                    static_params: &[],
                    body,
                }],
                call(&b, f, &[call(&b, worker, &[b.int(3)])]),
            ),
        ),
        false,
    );
}
