//! Chunk 6 first trial: discard only safe unused nonrecursive bindings.
use nash_ir::{
    analysis,
    build::Builder,
    core::*,
    dead_bindings, hygiene,
    pretty::pretty,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn unary<'a>(b: &Builder<'a>) -> Ty<'a> {
    Ty::Term(
        b.arena
            .alloc(TermTy::Fun(b.arena.alloc_slice_copy(&[INT]), INT)),
    )
}
fn bind<'a>(b: &Builder<'a>, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh("unused"),
        ty,
    }
}
fn check(
    name: &str,
    b: &Builder<'_>,
    before: &Core<'_>,
    removes: bool,
    fails: bool,
    logs: &[&str],
) {
    let after = dead_bindings::simplify(b, before);
    let normalized = nash_ir::anf::normalize(b, before);
    nash_ir::anf::validate(dead_bindings::simplify(b, normalized)).unwrap();
    hygiene::validate(after, &[]).unwrap();
    assert_eq!(before.ty, after.ty);
    assert_eq!(pretty(after), pretty(dead_bindings::simplify(b, after)));
    assert_eq!(
        analysis::size_estimate(after).nodes < analysis::size_estimate(before).nodes,
        removes
    );
    let baseline = crate::harness::eval_core_raw(b.arena, before);
    let candidate = crate::harness::eval_core_raw(b.arena, after);
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
    assert_eq!(candidate.logs, logs);
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
}
#[test]
fn unused_safe_values() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let p = bind(&b, INT);
    let values = [
        ("literal", b.int(9)),
        ("closure_with_failure", b.lam(&[p], b.error(INT))),
        ("delayed_failure", b.delay(b.error(INT))),
        (
            "partial_builtin",
            b.builtin(F::AddInteger, &[b.int(9)], unary(&b)),
        ),
        (
            "forced_builtin",
            b.builtin(
                F::HeadList,
                &[],
                Ty::Term(a.alloc(TermTy::Fun(
                    a.alloc_slice_copy(&[Ty::Const(a.alloc(ConstTy::List(INT)))]),
                    INT,
                ))),
            ),
        ),
    ];
    for (name, value) in values {
        check(
            name,
            &b,
            b.let_(bind(&b, value.ty), value, b.int(42)),
            true,
            false,
            &[],
        );
    }
}
#[test]
fn unused_strict_work_stays() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let trace = b.trace(b.lit(Constant::string(&a, "kept")), b.int(7));
    for (name, value, fails, logs) in [
        ("trace", trace, false, &["kept"][..]),
        ("failure", b.error(INT), true, &[][..]),
        (
            "forced_failure",
            b.force(b.delay(b.error(INT)), INT),
            true,
            &[][..],
        ),
        (
            "saturated_builtin",
            b.builtin(F::DivideInteger, &[b.int(1), b.int(0)], INT),
            true,
            &[][..],
        ),
        (
            "partial_strict_argument",
            b.builtin(F::AddInteger, &[trace], unary(&b)),
            false,
            &["kept"][..],
        ),
    ] {
        check(
            name,
            &b,
            b.let_(bind(&b, value.ty), value, b.int(42)),
            false,
            fails,
            logs,
        );
    }
}
#[test]
fn cascading_unused_bindings() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, INT);
    let y = bind(&b, INT);
    check(
        "cascade",
        &b,
        b.let_(x, b.int(7), b.let_(y, b.var(x.name, INT), b.int(42))),
        true,
        false,
        &[],
    );
}
#[test]
fn used_capture_stays() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, INT);
    let p = bind(&b, INT);
    check(
        "used_capture",
        &b,
        b.let_(x, b.int(7), b.lam(&[p], b.var(x.name, INT))),
        false,
        false,
        &[],
    );
}
#[test]
fn constructor_fields_are_strict() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (name, field, removes, fails) in [
        ("safe_constructor", b.int(7), true, false),
        ("strict_constructor", b.error(INT), false, true),
    ] {
        let ty = Ty::Runtime(a.alloc(nash_ir::ty::RuntimeTy::Constr {
            tag: 0,
            fields: a.alloc_slice_copy(&[INT]),
        }));
        let value = b.constr(0, &[field], ty);
        check(
            name,
            &b,
            b.let_(bind(&b, ty), value, b.int(42)),
            removes,
            fails,
            &[],
        );
    }
}
#[test]
fn diverging_call_is_retained() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let p = bind(&b, INT);
    let f = bind(&b, unary(&b));
    let recur = b.app(b.var(f.name, f.ty), &[b.var(p.name, INT)], INT);
    let root = b.let_rec(
        &[RecBinder {
            binder: f,
            params: a.alloc_slice_copy(&[p]),
            static_params: &[],
            body: recur,
        }],
        b.let_(
            bind(&b, INT),
            b.app(b.var(f.name, f.ty), &[b.int(0)], INT),
            b.int(42),
        ),
    );
    let after = dead_bindings::simplify(&b, root);
    // Deliberately do not run an infinite program: exact identity proves that
    // the unused recursive call and its strict evaluation remain intact.
    assert!(std::ptr::eq(root, after));
    hygiene::validate(after, &[]).unwrap();
    insta::assert_snapshot!(
        "diverging_call",
        format!(
            "--- core before\n{}\n--- core after\n{}",
            pretty(root),
            pretty(after)
        )
    );
}
#[test]
fn unused_closure_releases_capture() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, INT);
    let p = bind(&b, INT);
    let lam = b.lam(&[p], b.var(x.name, INT));
    check(
        "dead_capture",
        &b,
        b.let_(x, b.int(7), b.let_(bind(&b, lam.ty), lam, b.int(42))),
        true,
        false,
        &[],
    );
}
