//! Explicit-only Chunk 6 recursive reachability trial; not part of cargo test/nextest.
#[path = "../src/source.rs"]
mod source;
use nash_ir::{
    anf,
    build::Builder,
    core::*,
    hygiene, small_inline, static_lift,
    ty::{ConstTy, TermTy, Ty},
};
use nash_plutus::{
    arena::Arena,
    builtin::DefaultFunction as F,
    debruijn, flat,
    machine::{ExBudget, PlutusVersion},
    pretty,
    program::{Program, Version},
};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn bind<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn measure(label: &str, a: &Arena, core: &Core<'_>) {
    let b = Builder::new(a);
    let eval = |discard| {
        let core = if discard {
            nash_ir::dead_recursive::prune(&b, core)
        } else {
            core
        };
        let core = nash_codegen::recursion::rewrite(&b, core).unwrap();
        let term = nash_codegen::lower::lower_with_constant_sharing(a, core).unwrap();
        let program = Program::new(
            a,
            Version::plutus_v3(a),
            debruijn::to_debruijn(a, term).unwrap(),
        );
        let bytes = flat::encode(program).unwrap().len();
        let result = program.eval_version_budget(
            a,
            PlutusVersion::V3,
            ExBudget {
                cpu: 100_000_000,
                mem: 2_000_000,
            },
        );
        (
            result.info.consumed_budget,
            bytes,
            pretty::term(result.term.expect("bounded successful evaluation")),
            result.info.logs,
        )
    };
    let before = eval(false);
    let after = eval(true);
    assert_eq!(before.2, after.2);
    assert_eq!(before.3, after.3);
    println!(
        "{label},{},{},{},{},{},{}",
        before.0.cpu, after.0.cpu, before.0.mem, after.0.mem, before.1, after.1
    );
}
fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(120));
        std::process::exit(2);
    });
    println!("case,cpu before,cpu after,memory before,memory after,bytes before,bytes after");
    for total in [1, 2, 4, 8] {
        let mut counts = vec![0, 1, total];
        counts.sort();
        counts.dedup();
        for live in counts {
            let a = Arena::new();
            let b = Builder::new(&a);
            let fty = Ty::Term(a.alloc(TermTy::Fun(&[INT], INT)));
            let functions: Vec<_> = (0..total).map(|_| bind(&b, "f", fty)).collect();
            let definitions: Vec<_> = functions
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let p = bind(&b, "n", INT);
                    let body = if i + 1 == live {
                        b.var(p.name, INT)
                    } else {
                        let next = if i < live { functions[i + 1] } else { *f };
                        b.app(b.var(next.name, next.ty), &[b.var(p.name, INT)], INT)
                    };
                    RecBinder {
                        binder: *f,
                        params: a.alloc_slice_copy(&[p]),
                        static_params: &[],
                        body,
                    }
                })
                .collect();
            let body = if live == 0 {
                b.int(42)
            } else {
                b.app(b.var(functions[0].name, fty), &[b.int(42)], INT)
            };
            measure(
                &format!("group-{total}-live-{live}"),
                &a,
                b.let_rec(&definitions, body),
            );
        }
    }
    let a = Arena::new();
    let b = Builder::new(&a);
    let value = b.delay(b.int(42));
    let worker = bind(&b, "worker", value.ty);
    let member = RecBinder {
        binder: worker,
        params: &[],
        static_params: &[],
        body: value,
    };
    measure("unused-delayed-worker", &a, b.let_rec(&[member], b.int(42)));
    measure(
        "live-delayed-worker",
        &a,
        b.let_rec(&[member], b.force(b.var(worker.name, worker.ty), INT)),
    );
    for total in [2, 8] {
        for calls in [0, 1, 8, 64] {
            let a = Arena::new();
            let b = Builder::new(&a);
            let fty = Ty::Term(a.alloc(TermTy::Fun(&[INT], INT)));
            let functions: Vec<_> = (0..total).map(|_| bind(&b, "f", fty)).collect();
            let defs: Vec<_> = functions
                .iter()
                .enumerate()
                .map(|(i, f)| {
                    let n = bind(&b, "n", INT);
                    let recur = b.app(
                        b.var(f.name, f.ty),
                        &[b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT)],
                        INT,
                    );
                    let body = if i == 0 {
                        b.if_(
                            b.builtin(
                                F::EqualsInteger,
                                &[b.var(n.name, INT), b.int(0)],
                                Ty::Const(&ConstTy::Bool),
                            ),
                            b.int(42),
                            recur,
                        )
                    } else {
                        recur
                    };
                    RecBinder {
                        binder: *f,
                        params: a.alloc_slice_copy(&[n]),
                        static_params: &[],
                        body,
                    }
                })
                .collect();
            measure(
                &format!("recursive-group-{total}-calls-{calls}"),
                &a,
                b.let_rec(
                    &defs,
                    b.app(b.var(functions[0].name, fty), &[b.int(calls)], INT),
                ),
            );
        }
    }
    let names = [
        "listTraversal",
        "staticRecursion",
        "dataMatch",
        "dataMiss",
        "decoding",
        "validationPass",
        "constantPrefixTwice",
        "constantPrefixCold",
        "constantPrefixLoop",
    ];
    let cores = source::compile(&a, include_str!("../fixtures/Workloads.nash"), &names);
    for (name, core) in names.iter().zip(cores) {
        let core = hygiene::freshen(&b, core);
        let core = small_inline::simplify(&b, anf::normalize(&b, static_lift::lift(&b, core)));
        measure(name, &a, core);
    }
}
