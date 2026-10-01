//! Execute and export the checked-in semantic obligations, not copies in Rust.
use nash_driver::{Database, FileSystemSource, Project, build_graph_with_tests, test_with};
use nash_plutus::{
    arena::Arena, binder::DeBruijn, data::PlutusData, flat, machine::ExBudget, term::Term,
};
use nash_proof::{Domain, ProofProgram};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Mutex;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../proofs")
}

fn verified_obligations() -> BTreeSet<(String, String)> {
    include_str!("../../../proofs/verified.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let (module, name) = line.split_once('\t').unwrap();
            (module.to_owned(), name.to_owned())
        })
        .collect()
}

async fn compile(directory: &Path) -> Result<Vec<ProofProgram>, String> {
    let project = Project::load(directory).await.unwrap();
    let db = Arc::new(Mutex::new(Database::new(FileSystemSource::new())));
    let modules = project.discover_modules(&*db.lock().await).await.unwrap();
    let roots = project
        .discover_own_modules(&*db.lock().await)
        .await
        .unwrap();
    let graph = build_graph_with_tests(
        db.clone(),
        &modules.keys().cloned().collect::<Vec<_>>(),
        &roots.keys().cloned().collect::<Vec<_>>(),
    )
    .await
    .unwrap();
    let (report, programs) = test_with(db, &graph, &modules, move |solved| {
        nash_driver::build::compile_proofs_matching_with(
            solved,
            |uri| roots.contains_key(uri).then(nash_config::Build::default),
            |_, _| true,
        )
    })
    .await;
    if !report.is_success() {
        return Err(format!("{:#?}", report.ordered_reports()));
    }
    programs.unwrap().map_err(|e| e.message)
}

fn sample<'a>(
    arena: &'a Arena,
    program: &ProofProgram,
    domain: Domain,
    index: usize,
    case: usize,
) -> &'a Term<'a, DeBruijn> {
    let integers = [-7, 0, 1, 17, 18446744073709551616, -9223372036854775809];
    let n = integers[(case + index) % integers.len()];
    match domain {
        // The host Data representation stores constructor tags as u64. Lean's
        // Data model has arbitrary integer tags; that separate host-model gap
        // is documented and must not change the universal Nash obligation.
        Domain::Int
            if matches!(program.module.as_str(), "ConstrData" | "UnConstrData") && index == 0 =>
        {
            Term::integer_from(arena, [0, 1, 17, 255, 65536, 18446744073709551615][case])
        }
        Domain::Int => Term::integer_from(arena, n),
        Domain::Integer => Term::data_integer_from(arena, n),
        Domain::Bool => Term::bool(arena, (case + index).is_multiple_of(2)),
        Domain::String => Term::string(arena, ["", "éλ", "a\0b"][(case + index) % 3]),
        Domain::Bytes | Domain::ByteString => {
            let bytes: &[u8] = if case == 4 && program.module.starts_with("Bls12381") {
                let g2 = program.module.contains("G2")
                    || (!program.module.contains("G1") && index % 2 == 1);
                let mut encoded = vec![0; if g2 { 96 } else { 48 }];
                encoded[0] = 0xc0;
                arena.alloc_slice_copy(&encoded)
            } else {
                [b"".as_slice(), &[0], &[0, 1, 128, 255], &[255]][(case + index) % 4]
            };
            if domain == Domain::Bytes {
                Term::byte_string(arena, bytes)
            } else {
                Term::data_byte_string(arena, bytes)
            }
        }
        Domain::Data => {
            let value = match (case + index) % 5 {
                0 => PlutusData::integer_from(arena, n),
                1 => PlutusData::byte_string(arena, &[0, 255]),
                2 => PlutusData::constr(arena, 0, &[]),
                3 => PlutusData::list(arena, &[]),
                _ => PlutusData::map(arena, &[]),
            };
            Term::data(arena, value)
        }
        other => panic!("unexpected builtin domain {other:?}"),
    }
}

#[tokio::test]
async fn checked_in_builtin_obligations_compile_export_and_execute() {
    let programs = compile(&root()).await.unwrap();
    assert_eq!(
        programs
            .iter()
            .map(|p| &p.module)
            .collect::<BTreeSet<_>>()
            .len(),
        99
    );
    assert!(programs.len() > 300, "failure-mode obligations disappeared");
    let names = programs
        .iter()
        .map(|p| (p.module.clone(), p.name.clone()))
        .collect::<BTreeSet<_>>();
    assert!(
        verified_obligations().is_subset(&names),
        "a previously verified obligation disappeared"
    );

    let output =
        std::env::temp_dir().join(format!("nash-semantic-builtins-{}", std::process::id()));
    let files = nash_proof::export(&programs, &output, 300, 300, 10).unwrap();
    assert_eq!(files.len(), programs.len());
    let mut failures = Vec::new();
    for proof in &programs {
        let mut successful_returns = 0;
        for case in 0..6 {
            let arena = Arena::new();
            let mut computation = flat::decode::<DeBruijn>(&arena, &proof.flat).unwrap();
            let arguments = proof
                .domains
                .iter()
                .enumerate()
                .map(|(index, domain)| sample(&arena, proof, *domain, index, case))
                .collect::<Vec<_>>();
            for argument in &arguments {
                computation = computation.apply(&arena, argument);
            }
            let result = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                computation.eval_version_budget(
                    &arena,
                    nash_plutus::machine::PlutusVersion::V3,
                    ExBudget::max(),
                )
            })) {
                Ok(evaluated) => evaluated.term,
                Err(_) => {
                    failures.push(format!(
                        "{} / {} / sample {case}: evaluator panicked",
                        proof.module, proof.name
                    ));
                    continue;
                }
            };
            let passed = if let Some(checker) = &proof.postcondition {
                if let Ok(value) = result {
                    successful_returns += 1;
                    let mut condition = flat::decode::<DeBruijn>(&arena, &checker.flat).unwrap();
                    for argument in &arguments {
                        condition = condition.apply(&arena, argument);
                    }
                    let checked = condition.apply(&arena, value).eval_version_budget(
                        &arena,
                        nash_plutus::machine::PlutusVersion::V3,
                        ExBudget::max(),
                    );
                    checked
                        .term
                        .is_ok_and(|value| value == Term::bool(&arena, true))
                } else {
                    true
                }
            } else {
                match proof.expect {
                    nash_source::Expect::Pass => {
                        result.is_ok_and(|value| value == Term::unit(&arena))
                    }
                    nash_source::Expect::Fail => result.is_err(),
                    nash_source::Expect::FailOnce => {
                        panic!("builtin suite must not search for witnesses")
                    }
                }
            };
            if !passed {
                failures.push(format!("{} / {} / sample {case}", proof.module, proof.name));
            }
            if proof.domains.is_empty() {
                break;
            }
        }
        if proof.postcondition.is_some() && successful_returns == 0 {
            failures.push(format!(
                "{} / {}: all samples vacuous; provide a valid input",
                proof.module, proof.name
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn every_builtin_has_its_own_nash_specification() {
    let mut covered = BTreeSet::new();
    for builtin in nash_ast::primitives::BUILTINS {
        let module = builtin
            .name
            .split('_')
            .map(|part| format!("{}{}", part[..1].to_uppercase(), &part[1..]))
            .collect::<String>();
        let file = format!("{module}.nash");
        let active = root().join("builtins").join(&file);
        let pending = root().join("pending/builtins").join(&file);
        assert_ne!(
            active.is_file(),
            pending.is_file(),
            "exactly one spec for {}",
            builtin.name
        );
        let source =
            std::fs::read_to_string(if active.is_file() { active } else { pending }).unwrap();
        assert!(
            source.contains(&format!("Builtin.{}", builtin.name)) && source.contains("\nproof\n"),
            "{} has no executable obligation",
            builtin.name
        );
        covered.insert(module);
    }
    assert_eq!(covered.len(), 101);
}

#[tokio::test]
async fn pending_multi_scalar_obligations_fail_explicitly_in_codegen() {
    let error = compile(&root().join("pending")).await.unwrap_err();
    assert!(
        error.contains("Bls12_381G1Element") && error.contains("unavailable"),
        "{error}"
    );
}

#[tokio::test]
async fn semantic_obligations_detect_wrong_opcodes_argument_order_and_wrapping() {
    let mutations = [
        (
            "AddInteger",
            "add = Builtin.addInteger",
            "add = Builtin.subtractInteger",
            "commutative",
            0,
        ),
        (
            "AddInteger",
            "add = Builtin.addInteger",
            "add = Builtin.multiplyInteger",
            "left identity",
            1,
        ),
        (
            "AddInteger",
            "add = Builtin.addInteger",
            "add x _ = x",
            "commutative",
            0,
        ),
        (
            "AddInteger",
            "add = Builtin.addInteger",
            "add x y = Builtin.modInteger (Builtin.addInteger x y) 18446744073709551616",
            "successor never wraps at any integer width",
            18446744073709551615,
        ),
        (
            "AddInteger",
            "add = Builtin.addInteger",
            "add x y = let sum = Builtin.addInteger x y in if sum > 9223372036854775807 then 9223372036854775807 else sum",
            "successor never wraps at any integer width",
            9223372036854775807,
        ),
        (
            "AppendByteString",
            "Builtin.appendByteString p0 p1",
            "Builtin.appendByteString p1 p0",
            "preserves byte order",
            0,
        ),
        (
            "FstPair",
            "Builtin.fstPair p0",
            "Builtin.sndPair p0",
            "projects the correct component",
            0,
        ),
    ];
    let directory =
        std::env::temp_dir().join(format!("nash-builtin-mutants-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("src")).unwrap();
    std::fs::write(directory.join("nash.jsonc"), "{\"type\":\"application\"}\n").unwrap();
    for (i, (module, original, mutant, _, _)) in mutations.iter().enumerate() {
        let source =
            std::fs::read_to_string(root().join("builtins").join(format!("{module}.nash")))
                .unwrap();
        assert!(
            source.contains(original),
            "mutation no longer applies to {module}"
        );
        let source = source
            .replacen(
                &format!("module {module} "),
                &format!("module Mutant{i} "),
                1,
            )
            .replacen(original, mutant, 1);
        std::fs::write(
            directory.join("src").join(format!("Mutant{i}.nash")),
            source,
        )
        .unwrap();
    }
    let programs = compile(&directory).await.unwrap();
    for (i, (_, _, _, obligation, witness)) in mutations.iter().enumerate() {
        let proof = programs
            .iter()
            .find(|p| p.module == format!("Mutant{i}") && p.name == *obligation)
            .unwrap();
        let arena = Arena::new();
        let mut program = flat::decode::<DeBruijn>(&arena, &proof.flat).unwrap();
        for (index, domain) in proof.domains.iter().enumerate() {
            let input = match domain {
                Domain::Int => Term::integer_from(&arena, witness + index as i128),
                Domain::Data => Term::data_integer_from(&arena, index as i128),
                other => panic!("unexpected mutant domain {other:?}"),
            };
            program = program.apply(&arena, input);
        }
        assert!(
            program
                .eval_version_budget(
                    &arena,
                    nash_plutus::machine::PlutusVersion::V3,
                    ExBudget::max()
                )
                .term
                .is_err(),
            "mutation {i} escaped {obligation}"
        );
    }
}

#[tokio::test]
#[ignore = "requires NASH_PROOF_LEAN_PROJECT, the pinned Lean dependencies and Z3"]
async fn live_builtin_semantics() {
    let matching = std::env::var("NASH_PROOF_MATCH").ok();
    let verified = verified_obligations();
    let programs = compile(&root())
        .await
        .unwrap()
        .into_iter()
        .filter(|p| {
            matching.as_ref().map_or_else(
                || verified.contains(&(p.module.clone(), p.name.clone())),
                |pattern| format!("{}.{{{}}}", p.module, p.name).contains(pattern),
            )
        })
        .collect::<Vec<_>>();
    assert!(!programs.is_empty(), "no obligations matched {matching:?}");
    let config = nash_proof::Config {
        lean_project: std::env::var_os("NASH_PROOF_LEAN_PROJECT")
            .expect("Lean project")
            .into(),
        fuel: 300,
        postcondition_fuel: 300,
        solver_timeout: 10,
        wall_timeout: std::time::Duration::from_secs(60),
    };
    let output = std::env::temp_dir().join(format!(
        "nash-live-builtin-semantics-{}",
        std::process::id()
    ));
    let files = nash_proof::export(
        &programs,
        &output,
        config.fuel,
        config.postcondition_fuel,
        config.solver_timeout,
    )
    .unwrap();
    let mut failures = Vec::new();
    for (proof, source) in programs.iter().zip(files) {
        let outcome = nash_proof::run(proof, &source, &config).unwrap();
        eprintln!("{}.{{{}}}: {:?}", proof.module, proof.name, outcome.status);
        if !outcome.passed() {
            failures.push(format!(
                "{} / {}: {:?}: {}",
                proof.module, proof.name, outcome.status, outcome.diagnostics
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
