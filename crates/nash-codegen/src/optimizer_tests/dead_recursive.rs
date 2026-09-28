//! Chunk 6 trial: continuation-rooted recursive group reachability.
use nash_ir::{
    build::Builder,
    core::*,
    dead_recursive, hygiene,
    pretty::pretty,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction as F, constant::Constant};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn bind<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn function<'a>(b: &Builder<'a>, text: &'a str, arity: usize) -> Binder<'a> {
    bind(
        b,
        text,
        Ty::Term(b.arena.alloc(TermTy::Fun(
            b.arena.alloc_slice_copy(&vec![INT; arity]),
            INT,
        ))),
    )
}
fn def<'a>(
    b: &Builder<'a>,
    binder: Binder<'a>,
    params: &[Binder<'a>],
    body: &'a Core<'a>,
) -> RecBinder<'a> {
    RecBinder {
        binder,
        params: b.arena.alloc_slice_copy(params),
        static_params: &[],
        body,
    }
}
fn call<'a>(b: &Builder<'a>, f: Binder<'a>, args: &[&'a Core<'a>]) -> &'a Core<'a> {
    b.app(b.var(f.name, f.ty), args, INT)
}
fn trace<'a>(b: &Builder<'a>, message: &'a str, body: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(b.lit(Constant::string(b.arena, message)), body)
}
fn count(core: &Core<'_>) -> usize {
    let mut n = 0;
    core.walk(&mut |node| {
        if let CoreKind::LetRec { binders, .. } = node.kind {
            n += binders.len();
        }
    });
    n
}
fn check(name: &str, b: &Builder<'_>, before: &Core<'_>, members: usize, fails: bool) {
    let after = dead_recursive::prune(b, before);
    hygiene::validate(before, &[]).unwrap();
    hygiene::validate(after, &[]).unwrap();

    let anf = nash_ir::anf::normalize(b, before);
    nash_ir::anf::validate(dead_recursive::prune(b, anf)).unwrap();
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
    assert_eq!(before.ty, after.ty);
    assert_eq!(count(after), members);
    assert!(std::ptr::eq(after, dead_recursive::prune(b, after)));
    assert_eq!(baseline.observable, candidate.observable);
    assert_eq!(baseline.logs, candidate.logs);
}
#[test]
fn unused_self_and_mutual_cycles() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 1);
    let g = function(&b, "g", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let self_def = def(
        &b,
        f,
        &[x],
        trace(&b, "unreachable", call(&b, f, &[b.var(x.name, INT)])),
    );
    check(
        "unused_self",
        &b,
        b.let_rec(&[self_def], b.int(42)),
        0,
        false,
    );
    let fs = def(&b, f, &[x], call(&b, g, &[b.var(x.name, INT)]));
    let gs = def(&b, g, &[y], call(&b, f, &[b.var(y.name, INT)]));
    check(
        "unused_cycle",
        &b,
        b.let_rec(&[fs, gs], trace(&b, "continuation", b.int(42))),
        0,
        false,
    );
    check(
        "unused_failure",
        &b,
        b.let_rec(&[def(&b, f, &[x], b.error(INT))], b.int(42)),
        0,
        false,
    );
}
#[test]
fn transitive_chain_retains_order_and_drops_cycle() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 1);
    let g = function(&b, "g", 1);
    let dead = function(&b, "dead", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let z = bind(&b, "z", INT);
    let root = b.let_rec(
        &[
            def(&b, g, &[y], trace(&b, "g", b.var(y.name, INT))),
            def(&b, dead, &[z], call(&b, dead, &[b.var(z.name, INT)])),
            def(
                &b,
                f,
                &[x],
                trace(&b, "f", call(&b, g, &[b.var(x.name, INT)])),
            ),
        ],
        call(&b, f, &[b.int(42)]),
    );
    let after = dead_recursive::prune(&b, root);
    let CoreKind::LetRec { binders, .. } = after.kind else {
        panic!("retained group");
    };
    assert_eq!(
        binders
            .iter()
            .map(|r| r.binder.name.text)
            .collect::<Vec<_>>(),
        ["g", "f"]
    );
    check("transitive", &b, root, 2, false);
}
#[test]
fn returned_partial_and_suspended_references_are_roots() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 2);
    let dead = function(&b, "dead", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let z = bind(&b, "z", INT);
    let defs = [
        def(
            &b,
            f,
            &[x, y],
            b.builtin(
                F::AddInteger,
                &[b.var(x.name, INT), b.var(y.name, INT)],
                INT,
            ),
        ),
        def(&b, dead, &[z], b.error(INT)),
    ];
    let returned = b.app(
        b.let_rec(&defs, b.var(f.name, f.ty)),
        &[b.int(40), b.int(2)],
        INT,
    );
    check("returned", &b, returned, 1, false);
    let partial = b.app(
        b.var(f.name, f.ty),
        &[b.int(40)],
        function(&b, "typeOnly", 1).ty,
    );
    check(
        "partial",
        &b,
        b.app(b.let_rec(&defs, partial), &[b.int(2)], INT),
        1,
        false,
    );
    let delayed = b.let_rec(&defs, b.delay(call(&b, f, &[b.int(40), b.int(2)])));
    check("delayed_capture", &b, b.force(delayed, INT), 1, false);
    let p = bind(&b, "p", INT);
    let closure = b.let_rec(
        &defs,
        b.lam(&[p], call(&b, f, &[b.int(40), b.var(p.name, INT)])),
    );
    check(
        "lambda_capture",
        &b,
        b.app(closure, &[b.int(2)], INT),
        1,
        false,
    );
}
#[test]
fn singleton_preserves_static_metadata_and_captures() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 2);
    let dead = function(&b, "dead", 1);
    let x = bind(&b, "static", INT);
    let n = bind(&b, "n", INT);
    let z = bind(&b, "z", INT);
    let capture = bind(&b, "capture", INT);
    let body = b.if_(
        b.builtin(
            F::EqualsInteger,
            &[b.var(n.name, INT), b.int(0)],
            Ty::Const(&ConstTy::Bool),
        ),
        b.builtin(
            F::AddInteger,
            &[b.var(x.name, INT), b.var(capture.name, INT)],
            INT,
        ),
        call(
            &b,
            f,
            &[
                b.var(x.name, INT),
                b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT),
            ],
        ),
    );
    let member = RecBinder {
        static_params: &[0],
        ..def(&b, f, &[x, n], body)
    };
    let group = b.let_rec(
        &[member, def(&b, dead, &[z], b.error(INT))],
        call(&b, f, &[b.int(40), b.int(3)]),
    );
    let after = dead_recursive::prune(&b, group);
    let CoreKind::LetRec {
        binders: [kept], ..
    } = after.kind
    else {
        panic!("singleton");
    };
    assert!(std::ptr::eq(kept.params, member.params));
    assert_eq!(kept.static_params, &[0]);
    assert_eq!(kept.binder.ty, member.binder.ty);
    check(
        "singleton_metadata",
        &b,
        b.let_(capture, b.int(2), group),
        1,
        false,
    );
}
#[test]
fn reachable_failure_stays() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 1);
    let x = bind(&b, "x", INT);
    let root = b.let_rec(
        &[def(&b, f, &[x], trace(&b, "failure", b.error(INT)))],
        call(&b, f, &[b.int(42)]),
    );
    assert!(std::ptr::eq(root, dead_recursive::prune(&b, root)));
    check("reachable_failure", &b, root, 1, true);
}
#[test]
fn delayed_workers_and_nested_groups() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let value = b.delay(b.int(42));
    let worker = bind(&b, "worker", value.ty);
    let member = def(
        &b,
        worker,
        &[],
        b.delay(b.if_(
            b.lit(Constant::bool(&a, true)),
            b.int(42),
            b.force(b.var(worker.name, worker.ty), INT),
        )),
    );
    check(
        "unused_delayed_worker",
        &b,
        b.let_rec(&[member], b.int(7)),
        0,
        false,
    );
    check(
        "live_delayed_worker",
        &b,
        b.let_rec(&[member], b.force(b.var(worker.name, worker.ty), INT)),
        1,
        false,
    );
    let f = function(&b, "outer", 1);
    let g = function(&b, "inner", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let inner = b.let_rec(
        &[def(&b, g, &[y], call(&b, f, &[b.var(y.name, INT)]))],
        b.int(42),
    );
    let root = b.let_rec(&[def(&b, f, &[x], b.var(x.name, INT))], inner);
    check("nested_dead_capture", &b, root, 0, false);
}
#[test]
fn unsupported_recursive_values_stay_rejected() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let value = bind(&b, "value", INT);
    let root = b.let_rec(&[def(&b, value, &[], b.error(INT))], b.int(42));
    let after = dead_recursive::prune(&b, root);
    assert!(std::ptr::eq(root, after));
    assert_eq!(
        crate::recursion::rewrite(&b, after).unwrap_err(),
        crate::recursion::Error::RecursiveValue
    );
}
#[test]
fn reachable_mutual_cycle_survives() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 1);
    let g = function(&b, "g", 1);
    let dead = function(&b, "dead", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let z = bind(&b, "z", INT);
    let step = |next, name| {
        b.if_(
            b.builtin(
                F::EqualsInteger,
                &[b.var(name, INT), b.int(0)],
                Ty::Const(&ConstTy::Bool),
            ),
            b.int(42),
            call(
                &b,
                next,
                &[b.builtin(F::SubtractInteger, &[b.var(name, INT), b.int(1)], INT)],
            ),
        )
    };
    let root = b.let_rec(
        &[
            def(&b, f, &[x], step(g, x.name)),
            def(&b, dead, &[z], b.error(INT)),
            def(&b, g, &[y], step(f, y.name)),
        ],
        call(&b, f, &[b.int(4)]),
    );
    check("live_mutual_cycle", &b, root, 2, false);
}
#[test]
fn cold_and_nested_references_remain_live() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "outer", 1);
    let g = function(&b, "inner", 1);
    let x = bind(&b, "x", INT);
    let y = bind(&b, "y", INT);
    let outer = def(&b, f, &[x], b.var(x.name, INT));
    let inner = b.let_rec(
        &[def(&b, g, &[y], call(&b, f, &[b.var(y.name, INT)]))],
        call(&b, g, &[b.int(42)]),
    );
    check(
        "nested_live_capture",
        &b,
        b.let_rec(&[outer], inner),
        2,
        false,
    );
    let cold = b.let_rec(
        &[def(&b, f, &[x], b.error(INT))],
        b.if_(
            b.lit(Constant::bool(&a, true)),
            b.int(42),
            call(&b, f, &[b.int(0)]),
        ),
    );
    assert!(std::ptr::eq(cold, dead_recursive::prune(&b, cold)));
    check("cold_reference", &b, cold, 1, false);
}

#[test]
fn accepted_cleanup_releases_dead_captures_but_preserves_effects() {
    let a = Arena::new();
    let b = Builder::new(&a);
    for (name, captured, suspended) in [
        ("cleanup_dead_delay_capture", b.delay(b.error(INT)), true),
        ("cleanup_strict_capture", trace(&b, "kept", b.int(7)), false),
    ] {
        let capture = bind(&b, "capture", captured.ty);
        let f = function(&b, "dead", 1);
        let x = bind(&b, "x", INT);
        let value = b.var(capture.name, capture.ty);
        let read = if suspended {
            b.force(value, INT)
        } else {
            value
        };
        let body = b.builtin(F::AddInteger, &[read, read], INT);
        let root = b.let_(
            capture,
            captured,
            b.let_rec(&[def(&b, f, &[x], body)], b.int(42)),
        );
        let before = nash_ir::anf::normalize(&b, root);
        let after = nash_ir::small_inline::simplify(&b, before);
        assert_eq!(count(after), 0);
        if suspended {
            assert!(matches!(after.kind, CoreKind::Lit(_)));
        }
        assert_eq!(before.ty, after.ty);
        nash_ir::anf::validate(after).unwrap();
        hygiene::validate(after, &[]).unwrap();
        assert!(std::ptr::eq(
            after,
            nash_ir::small_inline::simplify(&b, after)
        ));
        let baseline =
            crate::harness::eval_core_raw(&a, crate::recursion::rewrite(&b, before).unwrap());
        let candidate =
            crate::harness::eval_core_raw(&a, crate::recursion::rewrite(&b, after).unwrap());
        assert_eq!(baseline.observable, candidate.observable);
        assert_eq!(baseline.logs, candidate.logs);

        insta::assert_snapshot!(
            name,
            format!(
                "--- core before\n{}\n--- core after\n{}\n--- result\n{}\n--- logs\n{:?}",
                pretty(before),
                pretty(after),
                candidate.result,
                candidate.logs
            )
        );
    }
}
