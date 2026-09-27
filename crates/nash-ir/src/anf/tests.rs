use super::*;
use crate::{hygiene, pretty::pretty, ty::ConstTy};
use nash_plutus::{arena::Arena, builtin::DefaultFunction};

fn snapshot<'a>(b: &Builder<'a>, before: &'a Core<'a>) -> String {
    let after = normalize(b, before);
    assert_eq!(validate(after), Ok(()));
    assert_eq!(hygiene::validate(after, &[]), Ok(()));
    assert_eq!(pretty(after), pretty(normalize(b, after)));
    assert_eq!(before.ty, after.ty);
    format!(
        "--- core before\n{}\n--- core after\n{}",
        pretty(before),
        pretty(after)
    )
}

#[test]
fn nested_operands_and_staged_application() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let x = Binder {
        name: b.fresh("x"),
        ty: int,
    };
    let y = Binder {
        name: b.fresh("y"),
        ty: int,
    };
    let f = b.lam(&[x, y], b.var(x.name, int));
    let add = b.builtin(DefaultFunction::AddInteger, &[b.int(1), b.int(2)], int);
    insta::assert_snapshot!(snapshot(&b, b.app(f, &[add, b.int(3)], int)));
}

#[test]
fn scopes_and_administrative_lets() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let x = Binder {
        name: b.fresh("x"),
        ty: int,
    };
    let y = Binder {
        name: b.fresh("y"),
        ty: int,
    };
    let nested = b.let_(x, b.int(1), b.var(x.name, int));
    let value = b.let_(y, nested, b.var(y.name, int));
    let delayed = b.delay(b.builtin(DefaultFunction::AddInteger, &[value, b.int(2)], int));
    insta::assert_snapshot!(snapshot(&b, b.force(delayed, int)));
}

#[test]
fn validator_rejects_compound_operands_and_accepts_atomic_calls() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let add = b.builtin(DefaultFunction::AddInteger, &[b.int(1), b.int(2)], int);
    assert!(validate(b.field(add, 0, 1, int)).is_err());
    let f = b.builtin(
        DefaultFunction::AddInteger,
        &[],
        Ty::Term(arena.alloc(TermTy::Fun(arena.alloc_slice_copy(&[int, int]), int))),
    );
    assert_eq!(validate(b.app(f, &[b.int(1), b.int(2)], int)), Ok(()));
}

#[test]
fn intermediate_function_types_and_fresh_supply() {
    let arena = Arena::new();
    let source = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let x = Binder {
        name: source.fresh("x"),
        ty: int,
    };
    let y = Binder {
        name: source.fresh("y"),
        ty: int,
    };
    let f = source.lam(&[x, y], source.var(x.name, int));
    let add = source.builtin(
        DefaultFunction::AddInteger,
        &[source.int(2), source.int(3)],
        int,
    );
    let expression = source.app(f, &[source.int(1), add], int);
    let fresh = Builder::new(&arena);
    insta::assert_snapshot!(snapshot(&fresh, expression));
}

#[test]
fn trace_body_and_branch_computations_stay_local() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let message = b.lit(nash_plutus::constant::Constant::string(&arena, "selected"));
    let branch = b.trace(
        message,
        b.builtin(
            DefaultFunction::AddInteger,
            &[b.force(b.delay(b.int(1)), int), b.int(2)],
            int,
        ),
    );
    let condition = b.lit(nash_plutus::constant::Constant::bool(&arena, true));
    insta::assert_snapshot!(snapshot(&b, b.if_(condition, branch, b.error(int))));
}

#[test]
fn empty_wrappers_and_binding_reassociation_keep_type_views() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let bytes = Ty::Const(&ConstTy::Bytes);
    let x = Binder {
        name: b.fresh("x"),
        ty: int,
    };
    let coercion = b.with_type(b.let_(x, b.int(1), b.var(x.name, int)), bytes);
    let wrapper = b.with_type(b.lam(&[], b.app(coercion, &[], bytes)), bytes);
    let after = normalize(&b, wrapper);
    assert_eq!(after.ty, bytes);
    let CoreKind::Let { body, .. } = after.kind else {
        panic!("expected binding")
    };
    assert_eq!(body.ty, bytes);
    assert_eq!(validate(after), Ok(()));
    assert_eq!(pretty(after), pretty(normalize(&b, after)));
    let y = Binder {
        name: b.fresh("y"),
        ty: bytes,
    };
    let reassociated = normalize(&b, b.let_(y, wrapper, b.var(y.name, bytes)));
    let CoreKind::Let { body, .. } = reassociated.kind else {
        panic!("expected outer binding")
    };
    let CoreKind::Let { value, .. } = body.kind else {
        panic!("expected inner binding")
    };
    assert_eq!(value.ty, bytes);
}
