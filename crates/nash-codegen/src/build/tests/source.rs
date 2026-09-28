const BASE_MODULES: &[&str] = &[
    include_str!("../../../../nash-driver/base/src/Function.nash"),
    include_str!("../../../../nash-driver/base/src/Bool.nash"),
    include_str!("../../../../nash-driver/base/src/Unit.nash"),
    include_str!("../../../../nash-driver/base/src/Ordering.nash"),
    include_str!("../../../../nash-driver/base/src/Functor.nash"),
    include_str!("../../../../nash-driver/base/src/Applicative.nash"),
    include_str!("../../../../nash-driver/base/src/Monad.nash"),
    include_str!("../../../../nash-driver/base/src/Option.nash"),
    include_str!("../../../../nash-driver/base/src/Data.nash"),
];

macro_rules! case {
    ($name:ident, $source:literal) => {
        case!(@run $name, $source, false, crate::build::TraceConfig::default());
    };
    (@run $name:ident, $source:literal, $fails:literal, $trace:expr) => {
        #[test]
        fn $name() {
            crate::build::tests::with_base_modules(
                indoc::indoc!($source),
                crate::build::tests::source::BASE_MODULES,
                |arena, build, root| {
                    let compiled = build.compile(arena, root, None, $trace).expect("source compiles to Core");
                    let core = crate::recursion::rewrite(&nash_ir::build::Builder::new(arena), compiled.core).expect("recursion rewrites");
                    let evaluated = crate::harness::eval_core_raw(arena, core);
                    assert_eq!(evaluated.result.starts_with("error:"), $fails, "unexpected evaluation category: {}", evaluated.result);
                    insta::assert_snapshot!(stringify!($name), format!("--- core\n{}\n{evaluated}", nash_ir::pretty::pretty(core)));
                    crate::harness::assert_candidate_equivalent(arena, compiled.core, &evaluated);
                },
            );
        }
    };
}
macro_rules! error_case {
    ($name:ident, $source:literal) => {
        case!(@run $name, $source, true, crate::build::TraceConfig::default());
    };
}

macro_rules! validator_case {
    ($name:ident, $source:literal) => {
        #[test]
        fn $name() {
            crate::build::tests::with_base_modules(
                indoc::indoc!($source),
                crate::build::tests::source::BASE_MODULES,
                |arena, build, root| {
                    let compiled = build
                        .compile(arena, root, None, crate::build::TraceConfig::default())
                        .expect("validator compiles to Core");
                    let assembled = crate::program::assemble_core(arena, compiled.core)
                        .expect("validator lowers to closed UPLC");
                    insta::assert_snapshot!(
                        stringify!($name),
                        format!(
                            "--- core\n{}\n--- uplc\n{}",
                            nash_ir::pretty::pretty(compiled.core),
                            nash_plutus::pretty::program(assembled.program)
                        )
                    );
                },
            );
        }
    };
}

macro_rules! traced_case {
    ($name:ident, $silent_name:ident, $source:literal) => {
        case!($name, $source);
        case!(@run $silent_name, $source, false, crate::build::TraceConfig { user: crate::build::TraceLevel::Silent, compiler: false });
    };
}
macro_rules! traced_error_case {
    ($name:ident, $silent_name:ident, $source:literal) => {
        error_case!($name, $source);
        case!(@run $silent_name, $source, true, crate::build::TraceConfig { user: crate::build::TraceLevel::Silent, compiler: false });
    };
}

mod builtins;
mod collections;
mod lists;
mod patterns;
