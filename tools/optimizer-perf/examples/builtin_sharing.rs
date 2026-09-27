//! Explicit-only Chunk 5 experiment: isolate force sharing from Core passes.
use nash_ir::{
    build::Builder,
    core::*,
    ty::{BigTy, ConstTy, TermTy, Ty},
};
use nash_plutus::{
    arena::Arena,
    builtin::DefaultFunction as F,
    constant::Constant,
    debruijn, flat,
    machine::{ExBudget, PlutusVersion},
    pretty,
    program::{Program, Version},
    term::Term,
};
const INT: Ty<'static> = Ty::Const(&ConstTy::Int);
fn binder<'a>(b: &Builder<'a>, text: &'a str, ty: Ty<'a>) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty,
    }
}
fn operation<'a>(b: &Builder<'a>, two: bool) -> &'a Core<'a> {
    if two {
        let data = b.builtin(F::IData, &[b.int(42)], Ty::Big(&BigTy::Data));
        let pair = b.builtin(
            F::MkPairData,
            &[data, data],
            Ty::Const(b.arena.alloc(ConstTy::Pair(data.ty, data.ty))),
        );
        b.builtin(F::UnIData, &[b.builtin(F::FstPair, &[pair], data.ty)], INT)
    } else {
        b.trace(b.lit(Constant::string(b.arena, "tick")), b.int(42))
    }
}
fn main() {
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_secs(120));
        eprintln!("experiment timeout");
        std::process::exit(2);
    });
    println!("case,cpu before,cpu after,memory before,memory after,bytes before,bytes after");
    for two in [false, true] {
        for (shape, count) in [
            ("sites", 1),
            ("sites", 2),
            ("sites", 8),
            ("loop", 0),
            ("loop", 1),
            ("loop", 8),
            ("loop", 64),
            ("unselected", 1),
        ] {
            let a = Arena::new();
            let b = Builder::new(&a);
            let x = binder(&b, "validatorArg", INT);
            let body = match shape {
                "sites" => (1..count).fold(operation(&b, two), |sum, _| {
                    b.builtin(F::AddInteger, &[sum, operation(&b, two)], INT)
                }),
                "unselected" => b.case(
                    CaseKind::Bool,
                    b.lit(Constant::bool(&a, true)),
                    &[
                        Branch {
                            test: Test::True,
                            binders: &[],
                            body: b.int(0),
                        },
                        Branch {
                            test: Test::False,
                            binders: &[],
                            body: operation(&b, two),
                        },
                    ],
                    None,
                    INT,
                ),
                "loop" => {
                    let n = binder(&b, "n", INT);
                    let f = binder(&b, "loop", Ty::Term(a.alloc(TermTy::Fun(&[INT], INT))));
                    let recur = b.app(
                        b.var(f.name, f.ty),
                        &[b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT)],
                        INT,
                    );
                    let next = b.builtin(F::AddInteger, &[operation(&b, two), recur], INT);
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
                                body: b.int(0),
                            },
                            Branch {
                                test: Test::False,
                                binders: &[],
                                body: next,
                            },
                        ],
                        None,
                        INT,
                    );
                    b.let_rec(
                        &[RecBinder {
                            binder: f,
                            params: a.alloc_slice_copy(&[n]),
                            static_params: &[],
                            body,
                        }],
                        b.app(b.var(f.name, f.ty), &[b.int(count)], INT),
                    )
                }
                _ => unreachable!(),
            };
            let core = nash_codegen::recursion::rewrite(&b, b.lam(&[x], body)).unwrap();
            let measure = |share| {
                let named = if share {
                    nash_codegen::lower::lower_with_builtin_sharing(&a, core)
                } else {
                    nash_codegen::lower::lower(&a, core)
                }
                .unwrap();
                let term = debruijn::to_debruijn(&a, named).unwrap();
                let program = Program::new(&a, Version::plutus_v3(&a), term);
                let bytes = flat::encode(program).unwrap().len();
                let eval = program
                    .apply(&a, Term::integer_from(&a, 0))
                    .eval_version_budget(
                        &a,
                        PlutusVersion::V3,
                        ExBudget {
                            cpu: 100_000_000,
                            mem: 2_000_000,
                        },
                    );
                let result = pretty::term(eval.term.expect("successful bounded evaluation"));
                (eval.info.consumed_budget, bytes, result, eval.info.logs)
            };
            let before = measure(false);
            let after = measure(true);
            assert_eq!(before.2, after.2);
            assert_eq!(before.3, after.3);
            println!(
                "{}-{}-{}, {}, {}, {}, {}, {}, {}",
                if two { "two-force" } else { "one-force" },
                shape,
                count,
                before.0.cpu,
                after.0.cpu,
                before.0.mem,
                after.0.mem,
                before.1,
                after.1
            );
        }
    }
}
