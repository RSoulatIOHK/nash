//! Explicit-only Chunk 6 unused-binding trial; not part of cargo test/nextest.
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
fn measure(label: &str, a: &Arena, core: &Core<'_>) {
    let b = Builder::new(a);
    let eval = |discard| {
        let core = if discard {
            nash_ir::dead_bindings::simplify(&b, core)
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
    let a = Arena::new();
    let b = Builder::new(&a);
    let p = bind(&b, "p", INT);
    for (name, value) in [
        ("literal", b.int(7)),
        ("closure", b.lam(&[p], b.error(INT))),
        ("delay", b.delay(b.error(INT))),
        (
            "partial",
            b.builtin(
                F::AddInteger,
                &[b.int(9)],
                Ty::Term(a.alloc(TermTy::Fun(&[INT], INT))),
            ),
        ),
        (
            "strict_trace",
            b.trace(b.lit(Constant::string(&a, "kept")), b.int(7)),
        ),
        (
            "saturated",
            b.builtin(F::AddInteger, &[b.int(1), b.int(2)], INT),
        ),
    ] {
        measure(
            name,
            &a,
            b.let_(bind(&b, "unused", value.ty), value, b.int(42)),
        );
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
