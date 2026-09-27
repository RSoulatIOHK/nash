use super::*;
use nash_plutus::{data::PlutusData, typ::Type};

#[test]
fn constants_retain_nested_representation_types() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let data = Constant::data(
        &arena,
        PlutusData::integer(&arena, nash_plutus::constant::integer_from(&arena, 42)),
    );
    let list = Constant::proto_list(&arena, Type::data(&arena), arena.alloc_slice_copy(&[data]));
    let pair = Constant::proto_pair(
        &arena,
        Type::integer(&arena),
        Type::list(&arena, Type::data(&arena)),
        Constant::integer_from(&arena, 1),
        list,
    );
    assert_eq!(
        b.lit(pair).ty,
        Ty::Const(&ConstTy::Pair(
            Ty::Const(&ConstTy::Int),
            Ty::Const(&ConstTy::List(Ty::Big(&BigTy::Data)))
        ))
    );
    assert_eq!(
        b.lit(Constant::proto_array(&arena, Type::data(&arena), &[]))
            .ty,
        Ty::Const(&ConstTy::Array(Ty::Big(&BigTy::Data)))
    );
}

#[test]
fn derived_types_follow_binding_and_suspension_semantics() {
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let int = Ty::Const(&ConstTy::Int);
    let x = Binder {
        name: b.fresh("x"),
        ty: int,
    };
    let body = b.var(x.name, x.ty);
    assert_eq!(b.lam(&[x], body).ty, Ty::Term(&TermTy::Fun(&[int], int)));
    assert_eq!(b.lam(&[], body).ty, int);
    assert_eq!(b.let_(x, b.int(1), body).ty, int);
    assert_eq!(b.let_rec(&[], body).ty, int);
    assert_eq!(
        b.trace(b.lit(Constant::string(&arena, "trace")), body).ty,
        int
    );
    let delayed = b.delay(body);
    assert_eq!(delayed.ty, Ty::Runtime(&RuntimeTy::Delay(int)));
    assert_eq!(b.force(delayed, int).ty, int);
}

#[test]
fn transformations_preserve_source_nominal_metadata() {
    use crate::{hygiene::freshen, ty::AdtRef};
    use nash_ast::{ModuleName, QualifiedName};
    let arena = Arena::new();
    let b = Builder::new(&arena);
    let nominal = Ty::Big(&BigTy::Adt(AdtRef {
        name: QualifiedName {
            home: ModuleName {
                package: None,
                name: "Test",
            },
            name: "Box",
        },
        args: &[],
    }));
    let raw = b.lit(Constant::data(
        &arena,
        PlutusData::integer(&arena, nash_plutus::constant::integer_from(&arena, 1)),
    ));
    let value = b.with_type(raw, nominal);
    let x = Binder {
        name: b.fresh("x"),
        ty: nominal,
    };
    let root = b.let_(x, value, b.var(x.name, nominal));
    let renamed = freshen(&b, root);
    assert_eq!(renamed.ty, nominal);
    let replacement = b.with_type(
        b.lit(Constant::data(
            &arena,
            PlutusData::integer(&arena, nash_plutus::constant::integer_from(&arena, 2)),
        )),
        nominal,
    );
    let mapped = root.map(&b, &mut |node| {
        std::ptr::eq(node, value).then_some(replacement)
    });
    assert_eq!(mapped.ty, nominal);
    assert_eq!(raw.ty, Ty::Big(&BigTy::Data));
    assert!(std::ptr::eq(b.with_type(value, nominal), value));
}
