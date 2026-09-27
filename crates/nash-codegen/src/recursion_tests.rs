use super::*;
use nash_ir::ty::{ConstTy, TermTy};
use nash_plutus::{
    arena::Arena,
    builtin::DefaultFunction as F,
    program::{Program, Version},
};

const INT: Ty<'static> = Ty::Const(&ConstTy::Int);

fn binding<'a>(b: &Builder<'a>, text: &'a str) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty: Ty::Const(&ConstTy::Int),
    }
}
fn function<'a>(b: &Builder<'a>, text: &'a str, arity: usize) -> Binder<'a> {
    Binder {
        name: b.fresh(text),
        ty: Ty::Term(b.arena.alloc(TermTy::Fun(
            b.arena.alloc_slice_copy(&vec![INT; arity]),
            INT,
        ))),
    }
}
fn op<'a>(b: &Builder<'a>, f: F, x: &'a Core<'a>, y: &'a Core<'a>) -> &'a Core<'a> {
    b.builtin(
        f,
        &[x, y],
        if f == F::EqualsInteger {
            Ty::Const(&ConstTy::Bool)
        } else {
            INT
        },
    )
}
fn decrement<'a>(b: &Builder<'a>, n: Binder<'a>) -> &'a Core<'a> {
    op(b, F::SubtractInteger, b.var(n.name, n.ty), b.int(1))
}
fn eval<'a>(b: &Builder<'a>, core: &'a Core<'a>) -> String {
    let rewritten = rewrite(b, core).unwrap();
    assert!(!nash_ir::pretty::pretty(rewritten).contains("letrec"));
    let term = crate::lower::lower(b.arena, rewritten).unwrap();
    let db = nash_plutus::debruijn::to_debruijn(b.arena, term).unwrap();
    let result = Program::new(b.arena, Version::plutus_v3(b.arena), db).eval(b.arena);
    nash_plutus::pretty::term(result.term.unwrap())
}
fn rec<'a>(
    b: &Builder<'a>,
    f: Binder<'a>,
    params: &[Binder<'a>],
    body: &'a Core<'a>,
) -> RecBinder<'a> {
    RecBinder {
        binder: f,
        params: b.arena.alloc_slice_copy(params),
        static_params: b
            .arena
            .alloc_slice_copy(&static_params(f.name, params, body)),
        body,
    }
}
#[test]
fn tail_recursion() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "sum", 2);
    let acc = binding(&b, "acc");
    let n = binding(&b, "n");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.var(acc.name, acc.ty),
        b.app(
            b.var(f.name, f.ty),
            &[
                op(
                    &b,
                    F::AddInteger,
                    b.var(acc.name, acc.ty),
                    b.var(n.name, n.ty),
                ),
                decrement(&b, n),
            ],
            INT,
        ),
    );
    let program = b.let_rec(
        &[rec(&b, f, &[acc, n], body)],
        b.app(b.var(f.name, f.ty), &[b.int(0), b.int(10)], INT),
    );
    assert_eq!(eval(&b, program), "(con integer 55)");
}
#[test]
fn non_tail_factorial() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "factorial", 1);
    let n = binding(&b, "n");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.int(1),
        op(
            &b,
            F::MultiplyInteger,
            b.var(n.name, n.ty),
            b.app(b.var(f.name, f.ty), &[decrement(&b, n)], INT),
        ),
    );
    assert_eq!(
        eval(
            &b,
            b.let_rec(
                &[rec(&b, f, &[n], body)],
                b.app(b.var(f.name, f.ty), &[b.int(6)], INT)
            )
        ),
        "(con integer 720)"
    );
}
#[test]
fn static_parameter_keeps_original_position() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "count", 2);
    let n = binding(&b, "n");
    let step = binding(&b, "step");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.int(0),
        op(
            &b,
            F::AddInteger,
            b.var(step.name, step.ty),
            b.app(
                b.var(f.name, f.ty),
                &[decrement(&b, n), b.var(step.name, step.ty)],
                INT,
            ),
        ),
    );
    assert_eq!(static_params(f.name, &[n, step], body), vec![1]);
    let program = b.let_rec(
        &[rec(&b, f, &[n, step], body)],
        b.app(b.var(f.name, f.ty), &[b.int(3), b.int(7)], INT),
    );
    assert_eq!(eval(&b, program), "(con integer 21)");
}
#[test]
fn mutual_even_odd_and_three_cycle() {
    for size in [2, 3] {
        let a = Arena::new();
        let b = Builder::new(&a);
        let functions = (0..size)
            .map(|_| function(&b, "cycle", 1))
            .collect::<Vec<_>>();
        let binders = functions
            .iter()
            .enumerate()
            .map(|(i, f)| {
                let n = binding(&b, "n");
                let body = b.if_(
                    op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
                    b.int(i as i128),
                    b.app(
                        b.var(functions[(i + 1) % size].name, functions[(i + 1) % size].ty),
                        &[decrement(&b, n)],
                        INT,
                    ),
                );
                rec(&b, *f, &[n], body)
            })
            .collect::<Vec<_>>();
        let program = b.let_rec(
            &binders,
            b.app(b.var(functions[0].name, functions[0].ty), &[b.int(7)], INT),
        );
        let measured = crate::harness::eval_core(&a, rewrite(&b, program).unwrap());
        assert!(measured.uplc.contains("(case"));
        assert!(measured.uplc.contains("(constr"));
        assert!(!measured.uplc.contains("select"));
        assert_eq!(eval(&b, program), format!("(con integer {})", 7 % size));
    }
}
#[test]
fn captures_lexical_binding() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "go", 1);
    let n = binding(&b, "n");
    let captured = binding(&b, "captured");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.var(captured.name, captured.ty),
        b.app(b.var(f.name, f.ty), &[decrement(&b, n)], INT),
    );
    assert_eq!(
        eval(
            &b,
            b.let_(
                captured,
                b.int(42),
                b.let_rec(
                    &[rec(&b, f, &[n], body)],
                    b.app(b.var(f.name, f.ty), &[b.int(3)], INT)
                )
            )
        ),
        "(con integer 42)"
    );
}
#[test]
fn first_class_recursive_function() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "go", 1);
    let n = binding(&b, "n");
    let alias = function(&b, "alias", 1);
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.int(12),
        b.let_(
            alias,
            b.var(f.name, f.ty),
            b.app(b.var(alias.name, alias.ty), &[decrement(&b, n)], INT),
        ),
    );
    assert!(static_params(f.name, &[n], body).is_empty());
    assert_eq!(
        eval(
            &b,
            b.let_rec(
                &[rec(&b, f, &[n], body)],
                b.app(b.var(f.name, f.ty), &[b.int(3)], INT)
            )
        ),
        "(con integer 12)"
    );
}
#[test]
fn rejects_recursive_value() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = binding(&b, "value");
    let program = b.let_rec(&[rec(&b, f, &[], b.var(f.name, f.ty))], b.int(0));
    assert!(matches!(rewrite(&b, program), Err(Error::RecursiveValue)));
}
#[test]
fn static_analysis_distinguishes_shadowed_names() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "f", 1);
    let x = binding(&b, "x");
    let shadow = binding(&b, "x");
    assert_eq!(
        static_params(
            f.name,
            &[x],
            b.app(b.var(f.name, f.ty), &[b.var(x.name, x.ty)], INT)
        ),
        vec![0]
    );
    assert!(
        static_params(
            f.name,
            &[x],
            b.lam(
                &[shadow],
                b.app(b.var(f.name, f.ty), &[b.var(shadow.name, shadow.ty)], INT)
            )
        )
        .is_empty()
    );
    assert!(static_params(f.name, &[x], b.app(b.var(f.name, f.ty), &[b.int(1)], INT)).is_empty());
}

#[test]
fn fully_static_dead_call_stays_lazy() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "loop", 1);
    let x = binding(&b, "x");
    let body = b.if_(
        b.lit(nash_plutus::constant::Constant::bool(&a, true)),
        b.var(x.name, x.ty),
        b.app(b.var(f.name, f.ty), &[b.var(x.name, x.ty)], INT),
    );
    assert_eq!(static_params(f.name, &[x], body), vec![0]);
    assert_eq!(
        eval(
            &b,
            b.let_rec(
                &[rec(&b, f, &[x], body)],
                b.app(b.var(f.name, f.ty), &[b.int(8)], INT)
            )
        ),
        "(con integer 8)"
    );
}

#[test]
fn stale_static_metadata_does_not_drop_changing_arguments() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "loop", 1);
    let n = binding(&b, "n");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.int(9),
        b.app(b.var(f.name, f.ty), &[decrement(&b, n)], INT),
    );
    let mut rb = rec(&b, f, &[n], body);
    rb.static_params = &[0];
    assert_eq!(
        eval(
            &b,
            b.let_rec(&[rb], b.app(b.var(f.name, f.ty), &[b.int(3)], INT))
        ),
        "(con integer 9)"
    );
}

#[test]
fn separate_builder_name_supply_does_not_capture_inputs() {
    let a = Arena::new();
    let b = Builder::new(&a);
    let f = function(&b, "go", 1);
    let n = binding(&b, "n");
    let captured = binding(&b, "self");
    let body = b.if_(
        op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
        b.var(captured.name, captured.ty),
        b.app(b.var(f.name, f.ty), &[decrement(&b, n)], INT),
    );
    let program = b.let_(
        captured,
        b.int(42),
        b.let_rec(
            &[rec(&b, f, &[n], body)],
            b.app(b.var(f.name, f.ty), &[b.int(3)], INT),
        ),
    );
    assert_eq!(eval(&Builder::new(&a), program), "(con integer 42)");
}

#[test]
fn mutual_overapplication_applies_arguments_to_dispatch_result() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let f = Binder {
        ty: Ty::Term(arena.alloc(TermTy::Fun(&[INT], Ty::Term(&TermTy::Fun(&[INT], INT))))),
        ..function(&b, "f", 1)
    };
    let g = function(&b, "g", 2);
    let n = binding(&b, "n");
    let x = binding(&b, "x");
    let m = binding(&b, "m");
    let y = binding(&b, "y");
    let first = rec(
        &b,
        f,
        &[n],
        b.lam(
            &[x],
            b.if_(
                op(&b, F::EqualsInteger, b.var(n.name, n.ty), b.int(0)),
                b.var(x.name, x.ty),
                b.app(
                    b.var(g.name, g.ty),
                    &[decrement(&b, n), b.var(x.name, x.ty)],
                    INT,
                ),
            ),
        ),
    );
    let second = rec(
        &b,
        g,
        &[m, y],
        b.app(
            b.var(f.name, f.ty),
            &[b.var(m.name, m.ty), b.var(y.name, y.ty)],
            INT,
        ),
    );
    let program = b.let_rec(
        &[first, second],
        b.app(b.var(g.name, g.ty), &[b.int(3), b.int(42)], INT),
    );
    assert_eq!(eval(&b, program), "(con integer 42)");
}
