//! Source metadata survives lowering even when two values share a UPLC layout.
use super::*;
use nash_ir::{
    core::{Core, CoreKind},
    ty::{BigTy, ConstTy, TermTy, Ty},
};

fn binding<'a>(core: &'a Core<'a>, text: &str) -> &'a Core<'a> {
    let mut found = None;
    core.walk(&mut |node| {
        if let CoreKind::Let { binder, value, .. } = &node.kind
            && binder.name.text == text
        {
            assert_eq!(binder.ty, value.ty, "binding {text}");
            found = Some(*value);
        }
    });
    found.unwrap_or_else(|| panic!("missing binding {text}"))
}

#[test]
fn nominal_constructors_with_identical_layout_keep_distinct_types() {
    with_base(
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type apple = Apple unit
        type orange = Orange unit
        type AppleBig = AppleBig Int
        type OrangeBig = OrangeBig Int
        main = (Apple (), Orange (), AppleBig 1, OrangeBig 1)
    "#
        ),
        |arena, build, root| {
            let compiled = build
                .compile(arena, root, None, TraceConfig::default())
                .unwrap();
            let Ty::Term(TermTy::Tuple(fields)) = compiled.core.ty else {
                panic!("tuple metadata")
            };
            assert_ne!(fields[0], fields[1]);
            assert_ne!(fields[2], fields[3]);
            assert!(matches!(fields[0], Ty::Term(TermTy::Adt(_))));
            assert!(matches!(fields[2], Ty::Big(BigTy::Adt(_))));
            let mut constructors = Vec::new();
            compiled.core.walk(&mut |node| {
                if let CoreKind::Constr { tag, .. } = &node.kind
                    && matches!(node.ty, Ty::Term(TermTy::Adt(_)))
                {
                    constructors.push(format!("tag {tag}: {}", node.ty));
                }
                if let CoreKind::Builtin {
                    func: nash_plutus::builtin::DefaultFunction::ConstrData,
                    ..
                } = &node.kind
                    && matches!(node.ty, Ty::Big(BigTy::Adt(_)))
                {
                    constructors.push(format!("constrData: {}", node.ty));
                }
            });

            insta::assert_snapshot!(format!(
                "root: {}\n{}",
                compiled.core.ty,
                constructors.join("\n")
            ));
            assert!(
                !crate::harness::eval_core(arena, compiled.core)
                    .result
                    .starts_with("error:")
            );
        },
    );
}

#[test]
fn record_projection_and_tuple_result_retain_field_types() {
    with_base(
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type item = Item unit
        type alias record = { stored : item, flag : bool }
        project : record -> item
        project r = r.stored
        main : (item, unit)
        main = (project { stored = (Item ()) , flag = True }, ())
    "#
        ),
        |arena, build, root| {
            let compiled = build
                .compile(arena, root, None, TraceConfig::default())
                .unwrap();
            let mut fields = Vec::new();
            compiled.core.walk(&mut |node| {
                if let CoreKind::Field { record, index, .. } = &node.kind {
                    assert!(matches!(node.ty, Ty::Term(TermTy::Adt(_))));
                    fields.push(format!("field {index}: {} from {}", node.ty, record.ty));
                }
            });

            insta::assert_snapshot!(format!("root: {}\n{}", compiled.core.ty, fields.join("\n")));
            assert!(
                !crate::harness::eval_core(arena, compiled.core)
                    .result
                    .starts_with("error:")
            );
        },
    );
}

#[test]
fn partial_builtin_and_function_coercion_keep_remaining_arrows() {
    with_base(
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        partial : int -> int
        partial = addInteger 1
        coerced : int -> int
        coerced = coerce partial
        main = coerced 2
    "#
        ),
        |arena, build, root| {
            let compiled = build
                .compile(arena, root, None, TraceConfig::default())
                .unwrap();
            let partial = binding(compiled.core, "partial");
            let coerced = binding(compiled.core, "coerced");
            let Ty::Term(TermTy::Fun(args, result)) = partial.ty else {
                panic!("partial function metadata")
            };
            assert_eq!(*args, [Ty::Const(&ConstTy::Int)]);
            assert_eq!(*result, Ty::Const(&ConstTy::Int));
            assert_eq!(partial.ty, coerced.ty);
            let mut builtin_types = Vec::new();
            let mut coerce_count = 0;
            compiled.core.walk(&mut |node| {
                if let CoreKind::Builtin {
                    func: nash_plutus::builtin::DefaultFunction::AddInteger,
                    args,
                } = &node.kind
                {
                    builtin_types.push(format!("{} applied arguments: {}", node.ty, args.len()));
                }
                if let CoreKind::Lam { params, body } = &node.kind
                    && params.first().is_some_and(|p| p.name.text == "coerce")
                {
                    assert_eq!(body.ty, partial.ty);
                    coerce_count += 1;
                }
            });
            assert_eq!(builtin_types.len(), 1);
            assert_eq!(coerce_count, 1);
            insta::assert_snapshot!(format!(
                "builtin: {}\npartial: {}\ncoerced: {}\nroot: {}",
                builtin_types[0], partial.ty, coerced.ty, compiled.core.ty
            ));
            assert_eq!(
                crate::harness::eval_core(arena, compiled.core).result,
                "(con integer 3)"
            );
        },
    );
}

#[test]
fn generic_empty_lists_keep_nominal_element_metadata() {
    with_base(
        indoc::indoc!(
            r#"
        module Main exposing (..)
        import Primitive exposing (..)
        import Builtin exposing (..)
        type Token = Token Int
        empty : list 'a
        empty = []
        tokens : list Token
        tokens = empty
        integers : list Int
        integers = empty
        natives : list int
        natives = empty
        filled : list Token
        filled = [Token 1]
        main = (tokens, integers, natives, filled)
    "#
        ),
        |arena, build, root| {
            let compiled = build
                .compile(arena, root, None, TraceConfig::default())
                .unwrap();
            let mut lines = Vec::new();
            for name in ["tokens", "integers", "natives", "filled"] {
                let value = binding(compiled.core, name);
                assert!(matches!(value.ty, Ty::Const(ConstTy::List(_))));
                lines.push(format!("{name}: {}", value.ty));
            }
            let mut empty_types = Vec::new();
            compiled.core.walk(&mut |node| {
                if let CoreKind::Lit(nash_plutus::constant::Constant::ProtoList(_, items)) =
                    &node.kind
                    && items.is_empty()
                {
                    empty_types.push(node.ty.to_string());
                }
            });
            empty_types.sort();
            empty_types.dedup();

            lines.push(format!("empty constants: {}", empty_types.join(", ")));
            insta::assert_snapshot!(lines.join("\n"));
            assert!(
                !crate::harness::eval_core(arena, compiled.core)
                    .result
                    .starts_with("error:")
            );
        },
    );
}

#[test]
fn instantiated_function_occurrences_keep_intermediate_application_types() {
    with_base(
        indoc::indoc!(
            r#"
            module Main exposing (..)
            import Primitive exposing (..)
            import Builtin exposing (..)
            identity x = x
            increment : int -> int
            increment x = addInteger x 1
            main : int
            main = identity increment 41
        "#
        ),
        |arena, build, root| {
            let compiled = build
                .compile(arena, root, None, TraceConfig::default())
                .unwrap();
            let mut found = false;
            compiled.core.walk(&mut |node| {
                if let CoreKind::App { func, args } = &node.kind
                    && let CoreKind::Var(name) = func.kind
                    && name.text == "identity"
                    && args.len() == 2
                {
                    let Ty::Term(TermTy::Fun(params, result)) = func.ty else {
                        panic!("instantiated identity is callable")
                    };
                    assert_eq!(params.len(), 2);
                    assert_eq!(params[0], args[0].ty);
                    assert_eq!(params[1], Ty::Const(&ConstTy::Int));
                    assert_eq!(*result, node.ty);
                    found = true;
                }
            });
            assert!(found);
            assert_eq!(
                crate::harness::eval_core(arena, compiled.core).result,
                "(con integer 42)"
            );
        },
    );
}
