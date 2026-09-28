//! Source-to-optimized snapshots reuse the normal Base fixture compiler.
use super::*;
use nash_ir::{anf, build::Builder, hygiene, known_bool, pretty::pretty, small_inline};

macro_rules! boolean_case_snapshot {
    ($name:ident, $source:literal) => {
        #[test]
        fn $name() {
            let source = indoc::indoc!($source);
            with_base(source, |arena, build, root| {
                let compiled = build.compile(arena, root, None, TraceConfig::default()).expect("source compiles to Core");
                let b = Builder::new(arena);
                let accepted = crate::snapshot_optimizer::optimize(arena, compiled.core);
                let mut after = accepted;
                loop {
                    let next = small_inline::simplify(&b, known_bool::reduce(&b, after));
                    if std::ptr::eq(after, next) { break; }
                    after = next;
                }
                // O0 includes only the recursion encoding required by lowering.
                let baseline_core = crate::recursion::rewrite(&b, compiled.core).unwrap();
                let baseline = crate::harness::eval_core_raw(arena, baseline_core);
                let optimized_core = crate::recursion::rewrite(&b, after).unwrap();
                let term = crate::lower::lower_with_constant_sharing(arena, optimized_core).unwrap();
                let optimized = crate::harness::eval_named(arena, term);
                assert!(!optimized.result.starts_with("error:"), "{}", optimized.result);

                insta::with_settings!({description => source, omit_expression => true}, {
                    insta::assert_snapshot!(stringify!($name), format!(
                        "--- unoptimized Core\n{}\n--- unoptimized UPLC\n{}\n--- optimized Core\n{}\n--- optimized UPLC\n{}\n--- result\n{}\n--- logs\n{:?}",
                        pretty(compiled.core), baseline.uplc, pretty(after), optimized.uplc, optimized.result, optimized.logs));
                });
                anf::validate(after).unwrap();
                hygiene::validate(after, &[]).unwrap();
                assert_eq!(compiled.core.ty, after.ty);
                assert_eq!(baseline.observable, optimized.observable);
                assert_eq!(baseline.logs, optimized.logs);
            });
        }
    };
}
boolean_case_snapshot!(
    boolean_helpers,
    r#"
    module Main exposing (..)
    import Primitive exposing (..)
    import Logic exposing ((&&), (||))
    main : bool
    main = (True && False) || (True && True)
"#
);
boolean_case_snapshot!(
    cold_trace,
    r#"
    module Main exposing (..)
    import Primitive exposing (..)
    import Builtin exposing (..)
    main : bool
    main = if True then False else trace "wrong" True
"#
);
boolean_case_snapshot!(
    retained_trace,
    r#"
    module Main exposing (..)
    import Primitive exposing (..)
    import Builtin exposing (..)
    main : bool
    main =
        let
            strict = trace "before" False
        in
        if True then strict else True
"#
);
