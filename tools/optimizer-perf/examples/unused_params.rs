//! Explicit-only Chunk 6 nonrecursive unused-parameter trial; not part of cargo test/nextest.
#[path = "../src/source.rs"]
mod source;
use nash_ir::{
    anf,
    build::Builder,
    core::*,
    hygiene, small_inline, static_lift,
    ty::{ConstTy, Ty},
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
            nash_ir::unused_params::reduce(&b, core)
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
    for omitted in [0, 1, 2, 3] {
        for count in [1, 2, 8] {
            for cold in [false, true] {
                let a = Arena::new();
                let b = Builder::new(&a);
                let params: Vec<_> = (0..3).map(|_| bind(&b, "p", INT)).collect();
                let live: Vec<_> = params
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| omitted != 3 && *i != omitted)
                    .map(|(_, p)| b.var(p.name, p.ty))
                    .collect();
                let body = if live.is_empty() {
                    b.trace(b.lit(Constant::string(&a, "body")), b.int(21))
                } else {
                    b.if_(
                        b.builtin(
                            F::EqualsInteger,
                            &[live[0], b.int(0)],
                            Ty::Const(&ConstTy::Bool),
                        ),
                        live[1],
                        b.builtin(F::AddInteger, &live, INT),
                    )
                };
                let value = b.lam(&params, body);
                let f = bind(&b, "helper", value.ty);
                let call = || b.app(b.var(f.name, f.ty), &[b.int(10), b.int(20), b.int(30)], INT);
                let calls = (1..count).fold(call(), |sum, _| {
                    b.builtin(F::AddInteger, &[sum, call()], INT)
                });
                let calls = if cold {
                    b.if_(b.lit(Constant::bool(&a, true)), b.int(42), calls)
                } else {
                    calls
                };
                let root = anf::normalize(&b, b.let_(f, value, calls));
                measure(
                    &format!("drop-{omitted}-sites-{count}-cold-{cold}"),
                    &a,
                    root,
                );
            }
        }
    }
    let a = Arena::new();
    let b = Builder::new(&a);
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

    let trial = r#"module Trial exposing (..)
import Primitive exposing (..)
import Builtin exposing (..)
import Literal exposing (..)

helper : int -> int -> int
helper ignored n =
    if Builtin.equalsInteger n 0 then
        40
    else
        Builtin.addInteger n 40

repeated : int
repeated = Builtin.addInteger (helper 100 1) (helper 200 2)

strict : int
strict = Builtin.addInteger (helper (Builtin.trace "argument" 100) 1) (helper 200 2)

namedRepeated : int
namedRepeated =
    let
        a = 100
        b = 1
        c = 200
        d = 2
    in
    Builtin.addInteger (helper a b) (helper c d)

noParams : int -> int
noParams ignored = Builtin.trace "body" 21

allUnused : int
allUnused = Builtin.addInteger (noParams 100) (noParams 200)
"#;
    let names = ["repeated", "strict", "namedRepeated", "allUnused"];
    let cores = source::compile(&a, trial, &names);
    for (name, core) in names.iter().zip(cores) {
        let core = hygiene::freshen(&b, core);
        let core = small_inline::simplify(&b, anf::normalize(&b, static_lift::lift(&b, core)));
        measure(name, &a, core);
    }
}
