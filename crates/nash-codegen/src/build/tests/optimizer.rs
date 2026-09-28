//! Source-to-optimized snapshots reuse the normal Base fixture compiler.
use super::*;
use nash_ir::{
    anf, build::Builder, hygiene, known_bool, pretty::pretty, small_inline, static_lift,
    unused_params,
};

macro_rules! boolean_case_snapshot {
    ($name:ident, $source:literal, $expected:literal, $logs:expr) => {
        #[test]
        fn $name() {
            let source = indoc::indoc!($source);
            with_base(source, |arena, build, root| {
                let compiled = build.compile(arena, root, None, TraceConfig::default()).expect("source compiles to Core");
                let b = Builder::new(arena);
                let fresh = hygiene::freshen(&b, compiled.core);
                let lifted = static_lift::lift(&b, fresh);
                let shortened = unused_params::reduce(&b, lifted);
                let accepted = small_inline::simplify(&b, anf::normalize(&b, shortened));
                let mut after = accepted;
                loop {
                    let next = small_inline::simplify(&b, known_bool::reduce(&b, after));
                    if std::ptr::eq(after, next) { break; }
                    after = next;
                }
                anf::validate(after).unwrap();
                hygiene::validate(after, &[]).unwrap();
                assert_eq!(compiled.core.ty, after.ty);
                let lower = |core| {
                    let rewritten = crate::recursion::rewrite(&b, core).unwrap();
                    let term = crate::lower::lower_with_constant_sharing(arena, rewritten).unwrap();
                    crate::harness::eval_named(arena, term)
                };
                let baseline = lower(accepted);
                let optimized = lower(after);
                assert_eq!(baseline.observable, optimized.observable);
                assert_eq!(baseline.logs, optimized.logs);
                assert_eq!(optimized.result, $expected);
                assert_eq!(optimized.logs, $logs);
                insta::with_settings!({description => source, omit_expression => true}, {
                    insta::assert_snapshot!(stringify!($name), format!(
                        "--- source Core\n{}\n--- Core before Boolean folding\n{}\n--- Core after Boolean folding and cleanup\n{}\n--- UPLC before\n{}\n--- UPLC after\n{}\n--- result\n{}\n--- logs\n{:?}",
                        pretty(compiled.core), pretty(accepted), pretty(after), baseline.uplc, optimized.uplc, optimized.result, optimized.logs));
                });
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
"#,
    "(con bool True)",
    Vec::<String>::new()
);
boolean_case_snapshot!(
    cold_trace,
    r#"
    module Main exposing (..)
    import Primitive exposing (..)
    import Builtin exposing (..)
    main : bool
    main = if True then False else trace "wrong" True
"#,
    "(con bool False)",
    Vec::<String>::new()
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
"#,
    "(con bool False)",
    vec!["before".to_owned()]
);
