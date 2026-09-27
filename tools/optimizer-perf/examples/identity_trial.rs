//! Temporary Rule 4 experiment: inline a selected helper at saturated direct calls.
//! No size policy, identity recognition, or compiler-pipeline wiring.
#[path = "../src/source.rs"]
mod source;

use nash_ir::{
    analysis, anf,
    build::Builder,
    core::*,
    hygiene, pretty, single_use, static_lift,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{
    arena::Arena,
    builtin::DefaultFunction as F,
    constant::Constant,
    debruijn, flat,
    machine::{ExBudget, MachineError, PlutusVersion},
    program::{Program, Version},
};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn binder<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn fun<'a>(b: &Builder<'a>, params: &[Ty<'a>], result: Ty<'a>) -> Ty<'a> {
    Ty::Term(
        b.arena
            .alloc(TermTy::Fun(b.arena.alloc_slice_copy(params), result)),
    )
}
fn trace<'a>(b: &Builder<'a>, s: &str, value: &'a Core<'a>) -> &'a Core<'a> {
    b.trace(
        b.lit(Constant::string(b.arena, b.arena.as_bump().alloc_str(s))),
        value,
    )
}

// Select by binding ID from the fixture, not by name or lambda-body shape.
fn inline_selected<'a>(b: &Builder<'a>, core: &'a Core<'a>, selected: u32) -> &'a Core<'a> {
    core.map(b, &mut |node| {
        let CoreKind::Let {
            binder,
            value,
            body,
        } = node.kind
        else {
            return None;
        };
        if binder.name.unique != selected {
            return None;
        }
        let CoreKind::Lam { params, .. } = value.kind else {
            panic!("selected helper must be a lambda")
        };
        let mut replace_call = |call: &'a Core<'a>| {
            let CoreKind::App { func, args } = call.kind else {
                return None;
            };
            if !matches!(func.kind,CoreKind::Var(n) if n.unique==selected)
                || args.len() != params.len()
            {
                return None;
            }
            Some(b.app(
                b.with_type(hygiene::freshen(b, value), func.ty),
                args,
                call.ty,
            ))
        };
        let body = if std::env::args().any(|arg| arg == "--recursive-only") {
            body.map(b, &mut |node| {
                let CoreKind::LetRec { binders, body } = node.kind else {
                    return None;
                };
                let binders: Vec<_> = binders
                    .iter()
                    .map(|rb| RecBinder {
                        body: rb.body.map(b, &mut replace_call),
                        ..*rb
                    })
                    .collect();
                Some(b.with_type(b.let_rec(&binders, body), node.ty))
            })
        } else {
            body.map(b, &mut replace_call)
        };
        // The shared lambda remains if there are any non-call uses.
        let body = if analysis::free_variables(body)
            .iter()
            .any(|n| n.unique == selected)
        {
            b.let_(binder, value, body)
        } else {
            body
        };
        Some(b.with_type(body, node.ty))
    })
}

#[derive(Debug, PartialEq)]
struct Observation {
    result: String,
    logs: Vec<String>,
}
fn measure<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> (Observation, i64, i64, usize, String) {
    let core = nash_codegen::recursion::rewrite(b, core).unwrap();
    let named = nash_codegen::lower::lower(b.arena, core).unwrap();
    let term = debruijn::to_debruijn(b.arena, named).unwrap();
    let p = Program::new(b.arena, Version::plutus_v3(b.arena), term);
    let ev = p.eval_version_budget(
        b.arena,
        PlutusVersion::V3,
        ExBudget {
            cpu: 100_000_000,
            mem: 2_000_000,
        },
    );
    assert!(!matches!(&ev.term, Err(MachineError::OutOfExError(_))));
    let result = match ev.term {
        Ok(t) => nash_plutus::pretty::term(t),
        Err(e) => format!("error: {e:?}"),
    };
    (
        Observation {
            result,
            logs: ev.info.logs,
        },
        ev.info.consumed_budget.cpu,
        ev.info.consumed_budget.mem,
        flat::encode(p).unwrap().len(),
        nash_plutus::pretty::term(named),
    )
}
fn run(
    label: &str,
    make: impl for<'a> FnOnce(&Builder<'a>, Binder<'a>) -> &'a Core<'a>,
    expected: &str,
    logs: &[&str],
) {
    let a = Arena::new();
    let b = Builder::new(&a);
    let param = binder(&b, "x", INT);
    let wrapper = std::env::args().any(|arg| arg == "builtin-wrapper");
    let conditional = std::env::args().any(|arg| arg == "conditional");
    let helper_body = if conditional {
        b.if_(
            b.builtin(
                F::LessThanInteger,
                &[b.var(param.name, INT), b.int(0)],
                Ty::Const(&ConstTy::Bool),
            ),
            b.int(0),
            b.var(param.name, INT),
        )
    } else if wrapper {
        b.builtin(F::AddInteger, &[b.var(param.name, INT), b.int(1)], INT)
    } else {
        b.var(param.name, INT)
    };
    let lambda = b.lam(&[param], helper_body);
    let id = binder(
        &b,
        if conditional {
            "nonNegative"
        } else if wrapper {
            "increment"
        } else {
            "identity"
        },
        lambda.ty,
    );
    let body = make(&b, id);
    let core = b.let_(id, lambda, body);
    let before = single_use::simplify(&b, anf::normalize(&b, static_lift::lift(&b, core)));
    let calls = analysis::occurrences(before)
        .uses
        .iter()
        .filter(|u| u.name.unique == id.name.unique)
        .count();
    assert!(calls >= 2, "fixture must retain a multiple-use helper");
    let after = single_use::simplify(&b, inline_selected(&b, before, id.name.unique));
    for c in [before, after] {
        anf::validate(c).unwrap();
        hygiene::validate(c, &[]).unwrap();
        assert_eq!(c.ty, core.ty);
    }
    let (old, oc, om, os, ou) = measure(&b, before);
    let (new, nc, nm, ns, nu) = measure(&b, after);
    assert_eq!(old, new);
    let expected = if wrapper {
        match label {
            "two calls" | "traced arguments" => "(con integer 4)",
            "eight calls" => "(con integer 16)",
            "recursive caller: setup once + eight iterations" => "(con integer 17)",
            "selected branch only" => "(con integer 43)",
            _ => expected,
        }
    } else {
        expected
    };
    assert_eq!(new.result, expected);
    assert_eq!(new.logs, logs);
    println!(
        "CASE {label}: {calls} static uses; CPU {oc} -> {nc}; MEMORY {om} -> {nm}; BYTES {os} -> {ns}"
    );
    println!(
        "--- before Core\n{}\n--- after Core\n{}\n--- before UPLC\n{ou}\n--- after UPLC\n{nu}\n--- outcome\n{new:?}",
        pretty::pretty(before),
        pretty::pretty(after)
    );
}
fn call<'a>(b: &Builder<'a>, id: Binder<'a>, x: &'a Core<'a>) -> &'a Core<'a> {
    b.app(b.var(id.name, id.ty), &[x], INT)
}
fn repeated<'a>(
    b: &Builder<'a>,
    id: Binder<'a>,
    count: usize,
    traced: bool,
    fail: bool,
) -> &'a Core<'a> {
    let mut total = b.int(0);
    for i in 0..count {
        let mut value = if fail && i == 0 {
            b.error(INT)
        } else {
            b.int(1)
        };
        if traced {
            value = trace(b, &format!("arg {i}"), value)
        }
        total = b.builtin(F::AddInteger, &[total, call(b, id, value)], INT);
    }
    total
}
fn recursive<'a>(b: &Builder<'a>, id: Binder<'a>) -> &'a Core<'a> {
    recursive_value(b, id, 1)
}
fn recursive_value<'a>(b: &Builder<'a>, id: Binder<'a>, value: i128) -> &'a Core<'a> {
    let n = binder(b, "n", INT);
    let acc = binder(b, "acc", INT);
    let f = binder(b, "loop", fun(b, &[INT, INT], INT));
    let next = b.app(
        b.var(f.name, f.ty),
        &[
            b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT),
            b.builtin(
                F::AddInteger,
                &[b.var(acc.name, INT), call(b, id, b.int(value))],
                INT,
            ),
        ],
        INT,
    );
    let body = b.if_(
        b.builtin(
            F::EqualsInteger,
            &[b.var(n.name, INT), b.int(0)],
            Ty::Const(&ConstTy::Bool),
        ),
        b.var(acc.name, INT),
        next,
    );
    b.let_rec(
        &[RecBinder {
            binder: f,
            params: b.arena.alloc_slice_copy(&[n, acc]),
            static_params: &[],
            body,
        }],
        b.app(b.var(f.name, f.ty), &[b.int(8), call(b, id, b.int(0))], INT),
    )
}
fn source_cases() {
    for (name, expected) in [
        ("booleanHelpers", "(con bool True)"),
        ("staticRecursion", "(con integer 42)"),
    ] {
        let arena = Arena::new();
        let b = Builder::new(&arena);
        let core = source::compile(&arena, include_str!("../fixtures/Workloads.nash"), &[name])[0];
        let fresh = hygiene::freshen(&b, core);
        let before = single_use::simplify(&b, anf::normalize(&b, static_lift::lift(&b, fresh)));
        // These fixtures each bind their shared literal-conversion helper first.
        // Select that binding explicitly, as in the earlier constructed trials.
        let CoreKind::Let { binder, value, .. } = before.kind else {
            panic!("expected outer helper binding")
        };
        assert!(matches!(value.kind, CoreKind::Lam { .. }));
        let after = single_use::simplify(&b, inline_selected(&b, before, binder.name.unique));
        for term in [before, after] {
            anf::validate(term).unwrap();
            hygiene::validate(term, &[]).unwrap();
            assert_eq!(term.ty, core.ty);
        }
        let (o0, c0, m0, s0, _) = measure(&b, core);
        let (old, oc, om, os, ou) = measure(&b, before);
        let (new, nc, nm, ns, nu) = measure(&b, after);
        assert_eq!(o0, old);
        assert_eq!(old, new);
        assert_eq!(new.result, expected);
        assert!(new.logs.is_empty());
        println!(
            "CASE {name}: O0 / rules1-3 / rules1-4; CPU {c0} / {oc} / {nc}; MEMORY {m0} / {om} / {nm}; BYTES {s0} / {os} / {ns}"
        );
        println!(
            "--- before Core\n{}\n--- after Core\n{}\n--- before UPLC\n{ou}\n--- after UPLC\n{nu}\n--- outcome\n{new:?}",
            pretty::pretty(before),
            pretty::pretty(after)
        );
    }
}

fn conditional_cases() {
    for count in [2, 4, 8, 16] {
        for (pattern, values) in [
            ("positive", vec![1; count]),
            ("negative", vec![-1; count]),
            (
                "mixed",
                (0..count)
                    .map(|i| if i % 2 == 0 { -1 } else { 1 })
                    .collect(),
            ),
        ] {
            let expected = format!(
                "(con integer {})",
                values.iter().copied().map(|x| x.max(0)).sum::<i128>()
            );
            run(
                &format!("{count} calls {pattern}"),
                |b, id| {
                    values.iter().fold(b.int(0), |sum, &value| {
                        b.builtin(F::AddInteger, &[sum, call(b, id, b.int(value))], INT)
                    })
                },
                &expected,
                &[],
            );
        }
        for selected in [0, count - 1] {
            run(
                &format!("{count} sites only branch {selected} executes"),
                |b, id| {
                    let mut body = call(
                        b,
                        id,
                        if selected == count - 1 {
                            b.int(-1)
                        } else {
                            trace(b, "cold", b.error(INT))
                        },
                    );
                    for i in (0..count - 1).rev() {
                        let arg = if i == selected {
                            b.int(-1)
                        } else {
                            trace(b, "cold", b.error(INT))
                        };
                        body = b.if_(
                            b.lit(Constant::bool(b.arena, i == selected)),
                            call(b, id, arg),
                            body,
                        );
                    }
                    body
                },
                "(con integer 0)",
                &[],
            );
        }
    }
    for (value, expected) in [(1, "(con integer 8)"), (-1, "(con integer 0)")] {
        run(
            &format!("recursive caller value {value}"),
            |b, id| recursive_value(b, id, value),
            expected,
            &[],
        );
    }
    run(
        "traced computed arguments",
        |b, id| {
            let negative = trace(
                b,
                "negative",
                b.builtin(F::SubtractInteger, &[b.int(0), b.int(1)], INT),
            );
            let positive = trace(
                b,
                "positive",
                b.builtin(F::AddInteger, &[b.int(1), b.int(1)], INT),
            );
            b.builtin(
                F::AddInteger,
                &[call(b, id, negative), call(b, id, positive)],
                INT,
            )
        },
        "(con integer 2)",
        &["negative", "positive"],
    );
    run(
        "first argument fails",
        |b, id| repeated(b, id, 2, true, true),
        "error: ExplicitErrorTerm",
        &["arg 0"],
    );
    run(
        "zero boundary",
        |b, id| {
            b.builtin(
                F::AddInteger,
                &[call(b, id, b.int(0)), call(b, id, b.int(-1))],
                INT,
            )
        },
        "(con integer 0)",
        &[],
    );
}

fn recursive_matrix() {
    for sites in [1, 2, 4, 8, 16] {
        for iterations in [0, 1, 4, 8] {
            let expected = format!("(con integer {})", iterations * (sites / 2));
            run(
                &format!(
                    "loop {sites} sites x {iterations} iterations; two outside calls retained"
                ),
                |b, id| loop_sites(b, id, sites, iterations, None, true),
                &expected,
                &[],
            );
        }
    }
    for sites in [2, 4, 8, 16] {
        for selected in [0, sites - 1] {
            run(
                &format!("loop {sites} sites x 8 iterations; only site {selected} selected"),
                |b, id| loop_sites(b, id, sites, 8, Some(selected), true),
                "(con integer 8)",
                &[],
            );
        }
    }
    for sites in [2, 8] {
        let expected = format!("(con integer {})", 8 * (sites / 2));
        run(
            &format!("loop {sites} sites x 8 iterations; no outside call"),
            |b, id| loop_sites(b, id, sites, 8, None, false),
            &expected,
            &[],
        );
    }
}
fn loop_sites<'a>(
    b: &Builder<'a>,
    id: Binder<'a>,
    sites: usize,
    iterations: usize,
    selected: Option<usize>,
    outside: bool,
) -> &'a Core<'a> {
    let n = binder(b, "n", INT);
    let acc = binder(b, "acc", INT);
    let f = binder(b, "loop", fun(b, &[INT, INT], INT));
    let work = if let Some(selected) = selected {
        let arg = |i| {
            if i == selected {
                b.int(1)
            } else {
                trace(b, "cold", b.error(INT))
            }
        };
        let mut work = call(b, id, arg(sites - 1));
        for i in (0..sites - 1).rev() {
            work = b.if_(
                b.lit(Constant::bool(b.arena, i == selected)),
                call(b, id, arg(i)),
                work,
            );
        }
        work
    } else {
        (0..sites).fold(b.int(0), |sum, i| {
            b.builtin(
                F::AddInteger,
                &[sum, call(b, id, b.int(if i % 2 == 0 { -1 } else { 1 }))],
                INT,
            )
        })
    };
    let next = b.app(
        b.var(f.name, f.ty),
        &[
            b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT),
            b.builtin(F::AddInteger, &[b.var(acc.name, INT), work], INT),
        ],
        INT,
    );
    let body = b.if_(
        b.builtin(
            F::EqualsInteger,
            &[b.var(n.name, INT), b.int(0)],
            Ty::Const(&ConstTy::Bool),
        ),
        b.var(acc.name, INT),
        next,
    );
    b.let_rec(
        &[RecBinder {
            binder: f,
            params: b.arena.alloc_slice_copy(&[n, acc]),
            static_params: &[],
            body,
        }],
        b.app(
            b.var(f.name, f.ty),
            &[
                b.int(iterations as i128),
                if outside {
                    b.builtin(
                        F::AddInteger,
                        &[call(b, id, b.int(0)), call(b, id, b.int(0))],
                        INT,
                    )
                } else {
                    b.int(0)
                },
            ],
            INT,
        ),
    )
}

fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(120));
        eprintln!("trial timeout");
        std::process::exit(2)
    });
    if std::env::args().any(|arg| arg == "conditional") {
        if std::env::args().any(|arg| arg == "--recursive-matrix") {
            recursive_matrix();
        } else {
            conditional_cases();
        }
        return;
    }
    if std::env::args().any(|arg| arg == "source-cases") {
        source_cases();
        return;
    }
    println!(
        "Plutus V3; UPLC 1.1.0; bundled default V3 model; raw Flat bytes; accepted normalize-once/rules1-3 versus selected-helper inlining plus rules1-3"
    );
    run(
        "two calls",
        |b, id| repeated(b, id, 2, false, false),
        "(con integer 2)",
        &[],
    );
    run(
        "eight calls",
        |b, id| repeated(b, id, 8, false, false),
        "(con integer 8)",
        &[],
    );
    run(
        "recursive caller: setup once + eight iterations",
        recursive,
        "(con integer 8)",
        &[],
    );
    run(
        "traced arguments",
        |b, id| repeated(b, id, 2, true, false),
        "(con integer 2)",
        &["arg 0", "arg 1"],
    );
    run(
        "first argument fails",
        |b, id| repeated(b, id, 2, true, true),
        "error: ExplicitErrorTerm",
        &["arg 0"],
    );
    run(
        "selected branch only",
        |b, id| {
            b.if_(
                b.lit(Constant::bool(b.arena, true)),
                call(b, id, trace(b, "chosen", b.int(42))),
                call(b, id, trace(b, "unselected", b.error(INT))),
            )
        },
        "(con integer 42)",
        &["chosen"],
    );
}
