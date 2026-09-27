//! Explicit-only Chunk 5 step 2 trial. Compare against force sharing alone.
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
    constant::Constant,
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
fn op<'a>(b: &Builder<'a>, kind: &str, x: &'a Core<'a>) -> &'a Core<'a> {
    match kind {
        "integer" => b.builtin(F::SubtractInteger, &[b.int(100), x], INT),
        "trace" => b.trace(b.lit(Constant::string(b.arena, "tick")), x),
        "bytes1" | "bytes64" | "bytes1024" => {
            let len: usize = kind[5..].parse().unwrap();
            let bytes = b.lit(Constant::byte_string(
                b.arena,
                b.arena.alloc_slice_copy(&vec![42; len]),
            ));
            let tail = b.lit(Constant::byte_string(b.arena, &[0]));
            let joined = b.builtin(F::AppendByteString, &[bytes, tail], bytes.ty);
            b.builtin(F::LengthOfByteString, &[joined], INT)
        }
        _ => unreachable!(),
    }
}
fn sites<'a>(b: &Builder<'a>, kind: &str, count: usize, x: &'a Core<'a>) -> &'a Core<'a> {
    (1..count).fold(op(b, kind, x), |sum, _| {
        b.builtin(F::AddInteger, &[sum, op(b, kind, x)], INT)
    })
}
fn measure(label: &str, a: &Arena, core: &Core<'_>) {
    let core = nash_codegen::recursion::rewrite(&Builder::new(a), core).unwrap();
    let eval = |sharing| {
        let term = if sharing {
            nash_codegen::lower::lower_with_constant_sharing(a, core)
        } else {
            nash_codegen::lower::lower_with_builtin_sharing(a, core)
        }
        .unwrap();
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
        eprintln!("experiment timeout");
        std::process::exit(2);
    });
    println!("case,cpu before,cpu after,memory before,memory after,bytes before,bytes after");
    for kind in ["integer", "trace", "bytes1", "bytes64", "bytes1024"] {
        for count in [1, 2, 3, 8] {
            let a = Arena::new();
            let b = Builder::new(&a);
            measure(
                &format!("{kind}-sites-{count}"),
                &a,
                sites(&b, kind, count, b.int(1)),
            );
            if count > 1 {
                let cold = b.if_(
                    b.lit(Constant::bool(&a, true)),
                    b.int(42),
                    sites(&b, kind, count, b.int(1)),
                );
                measure(&format!("{kind}-cold-{count}"), &a, cold);
            }
        }
    }
    for calls in [0, 1, 8, 64] {
        for body_sites in [1, 2] {
            let a = Arena::new();
            let b = Builder::new(&a);
            let n = bind(&b, "n", INT);
            let f = bind(&b, "loop", Ty::Term(a.alloc(TermTy::Fun(&[INT], INT))));
            let recur = b.app(
                b.var(f.name, f.ty),
                &[b.builtin(F::SubtractInteger, &[b.var(n.name, INT), b.int(1)], INT)],
                INT,
            );
            let next = b.builtin(
                F::AddInteger,
                &[sites(&b, "integer", body_sites, b.var(n.name, INT)), recur],
                INT,
            );
            let body = b.if_(
                b.builtin(
                    F::EqualsInteger,
                    &[b.var(n.name, INT), b.int(0)],
                    Ty::Const(&ConstTy::Bool),
                ),
                b.int(0),
                next,
            );
            let root = b.let_rec(
                &[RecBinder {
                    binder: f,
                    params: a.alloc_slice_copy(&[n]),
                    static_params: &[],
                    body,
                }],
                b.app(b.var(f.name, f.ty), &[b.int(calls)], INT),
            );
            measure(&format!("loop-{body_sites}-sites-{calls}-calls"), &a, root);
        }
    }
    for count in [2, 8] {
        let a = Arena::new();
        let b = Builder::new(&a);
        let x = bind(&b, "validatorArg", INT);
        let branches: Vec<_> = (0..count)
            .map(|i| Branch {
                test: Test::Int(nash_plutus::constant::integer_from(&a, i)),
                binders: &[],
                body: op(&b, "integer", b.var(x.name, INT)),
            })
            .collect();
        let root = b.app(
            b.lam(
                &[x],
                b.case(
                    CaseKind::Int,
                    b.var(x.name, INT),
                    &branches,
                    Some(b.int(0)),
                    INT,
                ),
            ),
            &[b.int(0)],
            INT,
        );
        measure(&format!("exclusive-{count}"), &a, root);
    }
    let a = Arena::new();
    let b = Builder::new(&a);
    let x = bind(&b, "x", INT);
    let value = b.lam(&[x], sites(&b, "integer", 8, b.var(x.name, INT)));
    let unused = bind(&b, "unused", value.ty);
    measure("unused-lambda", &a, b.let_(unused, value, b.int(42)));
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
