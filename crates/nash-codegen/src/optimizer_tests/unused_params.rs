//! Nonrecursive, exact-call-only unused-parameter trial.
use nash_ir::{
    anf,
    build::Builder,
    core::*,
    hygiene,
    pretty::pretty,
    ty::{ConstTy, TermTy, Ty},
    unused_params,
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn bind<'a>(b: &Builder<'a>, name: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(name),
        ty,
    }
}
fn trace<'a>(b: &Builder<'a>, name: &'a str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, name)), value)
}
fn call<'a>(b: &Builder<'a>, f: Binder<'a>, args: &[&'a Core<'a>]) -> &'a Core<'a> {
    b.app(b.var(f.name, f.ty), args, INT)
}
fn check(name: &str, b: &Builder<'_>, core: &Core<'_>, changed: bool, fails: bool) {
    let before = anf::normalize(b, core);
    let after = unused_params::reduce(b, before);

    for phase in [before, after] {
        anf::validate(phase).unwrap();
        hygiene::validate(phase, &[]).unwrap();
    }

    let baseline =
        crate::harness::eval_core_raw(b.arena, crate::recursion::rewrite(b, before).unwrap());
    let candidate =
        crate::harness::eval_core_raw(b.arena, crate::recursion::rewrite(b, after).unwrap());

    assert_eq!(candidate.result.starts_with("error:"), fails);
    insta::assert_snapshot!(
        name,
        format!(
            "--- core before\n{}\n--- core after\n{}\n--- uplc before\n{}\n--- uplc after\n{}\n--- result\n{}\n--- logs\n{:?}",
            pretty(before),
            pretty(after),
            baseline.uplc,
            candidate.uplc,
            candidate.result,
            candidate.logs
        )
    );
    // Properties independent of the expected snapshot.
    assert_eq!(!std::ptr::eq(before, after), changed);
    assert!(std::ptr::eq(after, unused_params::reduce(b, after)));
    assert_eq!(before.ty, after.ty);
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
}
#[test]
fn removes_first_middle_and_last_parameters() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (index, name) in ["first", "middle", "last"].iter().enumerate() {
        let params: Vec<_> = ["x", "y", "z"].iter().map(|s| bind(&b, s, INT)).collect();
        let used: Vec<_> = params
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != index)
            .map(|(_, p)| b.var(p.name, p.ty))
            .collect();
        let value = b.lam(&params, b.builtin(F::AddInteger, &used, INT));
        let f = bind(&b, "helper", value.ty);
        let invoke = || call(&b, f, &[b.int(10), b.int(20), b.int(30)]);
        check(
            name,
            &b,
            b.let_(
                f,
                value,
                b.builtin(F::AddInteger, &[invoke(), invoke()], INT),
            ),
            true,
            false,
        );
    }
}
#[test]
fn all_unused_runs_body_per_call_and_stays_cold() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(&[x], trace(&b, "body", b.int(21)));
    let f = bind(&b, "helper", value.ty);
    let calls = b.builtin(
        F::AddInteger,
        &[call(&b, f, &[b.int(1)]), call(&b, f, &[b.int(2)])],
        INT,
    );
    check("all_unused_twice", &b, b.let_(f, value, calls), true, false);
    let cold = b.if_(
        b.lit(Constant::bool(&a, true)),
        b.int(42),
        call(&b, f, &[b.int(1)]),
    );
    check("all_unused_cold", &b, b.let_(f, value, cold), true, false);
}
#[test]
fn strict_removed_arguments_keep_effects_and_failure() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "unused", INT);
    let y = bind(&b, "used", INT);
    let value = b.lam(&[x, y], trace(&b, "body", b.var(y.name, INT)));
    let f = bind(&b, "helper", value.ty);
    check(
        "strict_trace",
        &b,
        b.let_(
            f,
            value,
            call(&b, f, &[trace(&b, "argument", b.int(7)), b.int(42)]),
        ),
        true,
        false,
    );
    check(
        "strict_failure",
        &b,
        b.let_(
            f,
            value,
            call(&b, f, &[trace(&b, "argument", b.error(INT)), b.int(42)]),
        ),
        true,
        true,
    );
    let all_unused = b.lam(&[x], trace(&b, "unreachable", b.int(42)));
    let f = bind(&b, "allUnused", all_unused.ty);
    check(
        "all_unused_failure",
        &b,
        b.let_(f, all_unused, call(&b, f, &[b.error(INT)])),
        true,
        true,
    );
}
#[test]
fn partial_staged_escaping_and_oversaturated_uses_block_changes() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "unused", INT);
    let y = bind(&b, "used", INT);
    let value = b.lam(&[x, y], b.var(y.name, INT));
    let f = bind(&b, "helper", value.ty);
    let unary = Ty::Term(a.alloc(TermTy::Fun(&[INT], INT)));
    let partial = b.app(b.var(f.name, f.ty), &[b.int(0)], unary);
    check(
        "partial",
        &b,
        b.let_(f, value, b.app(partial, &[b.int(42)], INT)),
        false,
        false,
    );
    check(
        "staged_trace",
        &b,
        b.let_(
            f,
            value,
            call(&b, f, &[b.int(0), trace(&b, "later argument", b.int(42))]),
        ),
        false,
        false,
    );
    let alias = bind(&b, "alias", f.ty);
    let escaped = b.let_(
        alias,
        b.var(f.name, f.ty),
        b.builtin(
            F::AddInteger,
            &[
                call(&b, f, &[b.int(0), b.int(21)]),
                call(&b, alias, &[b.int(0), b.int(21)]),
            ],
            INT,
        ),
    );
    check(
        "escaping_blocks_all_calls",
        &b,
        b.let_(f, value, escaped),
        false,
        false,
    );
    let z = bind(&b, "z", INT);
    let value = b.lam(&[x], b.lam(&[z], b.var(z.name, INT)));
    let f = bind(&b, "returnsFunction", value.ty);
    check(
        "oversaturated",
        &b,
        b.let_(f, value, call(&b, f, &[b.int(0), b.int(42)])),
        false,
        false,
    );
}
#[test]
fn suspended_captures_and_unique_names_are_respected() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let used = bind(&b, "same", INT);
    let unused = bind(&b, "same", INT);
    let x = bind(&b, "same", INT);
    let value = b.lam(&[used, unused], b.lam(&[x], b.var(used.name, INT)));
    let f = bind(&b, "helper", value.ty);
    let returned = b.app(
        b.var(f.name, f.ty),
        &[b.int(42), b.int(0)],
        Ty::Term(a.alloc(TermTy::Fun(&[INT], INT))),
    );
    check(
        "captured_parameter",
        &b,
        b.let_(f, value, b.app(returned, &[b.int(7)], INT)),
        true,
        false,
    );
}
#[test]
fn explicit_function_views_keep_their_result_annotations() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "unused", INT);
    let y = bind(&b, "used", INT);
    let value = b.lam(&[x, y], b.var(y.name, INT));
    let f = bind(&b, "helper", value.ty);
    let bytes = Ty::Const(&ConstTy::Bytes);
    let view = Ty::Term(a.alloc(TermTy::Fun(&[INT, INT], bytes)));
    let root = b.with_type(
        b.let_(
            f,
            value,
            b.app(b.var(f.name, view), &[b.int(0), b.int(42)], bytes),
        ),
        Ty::Erased,
    );
    let after = unused_params::reduce(&b, root);
    assert_eq!(after.ty, Ty::Erased);
    let CoreKind::Let {
        binder,
        value,
        body,
    } = after.kind
    else {
        panic!("let");
    };
    assert_eq!(binder.ty, value.ty);
    let CoreKind::App { func, args } = body.kind else {
        panic!("call");
    };
    assert_eq!(args.len(), 1);
    assert_eq!(body.ty, bytes);
    assert_eq!(func.ty, Ty::Term(a.alloc(TermTy::Fun(&[INT], bytes))));
    hygiene::validate(after, &[]).unwrap();
    anf::validate(after).unwrap();
    insta::assert_snapshot!(
        "type_views",
        format!(
            "--- core before\n{}\n--- core after\n{}",
            pretty(root),
            pretty(after)
        )
    );
}

#[test]
fn recursive_signatures_and_unsupported_views_are_unchanged() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "unused", INT);
    let y = bind(&b, "used", INT);
    let value = b.lam(&[x, y], b.var(y.name, INT));
    let f = bind(&b, "helper", value.ty);
    let recursive = b.let_rec(
        &[RecBinder {
            binder: f,
            params: a.alloc_slice_copy(&[x, y]),
            static_params: &[],
            body: b.var(y.name, INT),
        }],
        call(&b, f, &[b.int(0), b.int(42)]),
    );
    check("recursive_unchanged", &b, recursive, false, false);
    let opaque = b.let_(
        f,
        value,
        b.app(b.var(f.name, Ty::Erased), &[b.int(0), b.int(42)], INT),
    );
    assert!(std::ptr::eq(opaque, unused_params::reduce(&b, opaque)));
}
#[test]
fn all_unused_preserves_independent_delayed_type_views() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "unused", INT);
    let value = b.lam(&[x], b.int(42));
    let f = bind(&b, "helper", value.ty);
    let bytes = Ty::Const(&ConstTy::Bytes);
    let view = Ty::Term(a.alloc(TermTy::Fun(&[INT], bytes)));
    let root = b.let_(f, value, b.app(b.var(f.name, view), &[b.int(0)], bytes));
    let after = unused_params::reduce(&b, root);
    let CoreKind::Let {
        binder,
        value,
        body,
    } = after.kind
    else {
        panic!("let");
    };
    assert_eq!(binder.ty, b.delay(b.int(42)).ty);
    assert_eq!(value.ty, binder.ty);
    let CoreKind::Force(target) = body.kind else {
        panic!("force");
    };
    assert_eq!(
        target.ty,
        Ty::Runtime(a.alloc(nash_ir::ty::RuntimeTy::Delay(bytes)))
    );
    assert_eq!(body.ty, bytes);
    hygiene::validate(after, &[]).unwrap();
    anf::validate(after).unwrap();
    insta::assert_snapshot!(
        "delayed_type_views",
        format!(
            "--- core before\n{}\n--- core after\n{}",
            pretty(root),
            pretty(after)
        )
    );
}
