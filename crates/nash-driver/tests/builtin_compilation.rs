//! Translation validation against an independent UPLC calling convention.
//! No expected term is compiled through Nash or read from runtime metadata.
use nash_ast::{QualifiedName, Type, primitives};
use nash_codegen::build::{Build, Input, TraceConfig};
use nash_driver::{Database, InMemorySource, build_graph, build_with, bundled_base};
use nash_plutus::{arena::Arena, binder::Eval, flat, term::Term};
use std::{collections::BTreeSet, fmt::Write, sync::Arc};
use tokio::sync::Mutex;
use url::Url;

#[derive(Clone, Debug)]
struct Contract {
    name: &'static str,
    tag: usize,
    forces: usize,
    arity: usize,
    bind_result: bool,
    lean: Option<&'static str>,
}

fn contracts() -> Vec<Contract> {
    include_str!("fixtures/builtin-compilation/spec.tsv")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields = line.split('\t').collect::<Vec<_>>();
            assert_eq!(fields.len(), 6, "{line}");
            Contract {
                name: fields[0],
                tag: fields[1].parse().unwrap(),
                forces: fields[2].parse().unwrap(),
                arity: fields[3].parse().unwrap(),
                bind_result: fields[5] == "1",
                lean: (fields[4] != "-").then_some(fields[4]),
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Syntax {
    Var(usize),
    Lam(Box<Self>),
    Apply(Box<Self>, Box<Self>),
    Force(Box<Self>),
    Builtin(usize),
}

impl Syntax {
    fn lean(&self) -> String {
        match self {
            Self::Var(index) => format!("(.var {index})"),
            Self::Lam(body) => format!("(.lam {})", body.lean()),
            Self::Apply(function, argument) => {
                format!("(.apply {} {})", function.lean(), argument.lean())
            }
            Self::Force(body) => format!("(.force {})", body.lean()),
            Self::Builtin(tag) => format!("(.builtin {tag})"),
        }
    }
}

fn captured(term: &Term<'_, nash_plutus::binder::DeBruijn>) -> Syntax {
    match term {
        // Flat and Nash use one-based indices; the certificate uses zero-based.
        Term::Var(index) => Syntax::Var(index.index().checked_sub(1).unwrap()),
        Term::Lambda { body, .. } => Syntax::Lam(Box::new(captured(body))),
        Term::Apply { function, argument } => {
            Syntax::Apply(Box::new(captured(function)), Box::new(captured(argument)))
        }
        Term::Force(body) => Syntax::Force(Box::new(captured(body))),
        Term::Builtin(builtin) => Syntax::Builtin(**builtin as usize),
        other => panic!("unexpected builtin wrapper: {other:?}"),
    }
}

fn reference(contract: &Contract, supplied: usize) -> Syntax {
    let mut body = Syntax::Builtin(contract.tag);
    for _ in 0..contract.forces {
        body = Syntax::Force(Box::new(body));
    }
    for position in 0..supplied {
        body = Syntax::Apply(
            Box::new(body),
            Box::new(Syntax::Var(supplied - position - 1)),
        );
    }
    if contract.bind_result && supplied == contract.arity {
        body = Syntax::Apply(
            Box::new(Syntax::Lam(Box::new(Syntax::Var(0)))),
            Box::new(body),
        );
    }
    for _ in 0..supplied {
        body = Syntax::Lam(Box::new(body));
    }
    // Ordinary O0 assembly retains the root declaration's administrative let.
    Syntax::Apply(
        Box::new(Syntax::Lam(Box::new(Syntax::Var(0)))),
        Box::new(body),
    )
}

struct Case {
    contract: Contract,
    supplied: usize,
    flat: Vec<u8>,
    syntax: Syntax,
}

async fn compile_cases() -> Vec<Case> {
    let contracts = contracts();
    assert_eq!(contracts.len(), 101);
    assert_eq!(contracts.iter().filter(|c| c.lean.is_some()).count(), 91);
    let names = contracts.iter().map(|c| c.name).collect::<BTreeSet<_>>();
    assert_eq!(names.len(), contracts.len(), "duplicate specification name");
    assert_eq!(
        names,
        primitives::BUILTINS.iter().map(|b| b.name).collect(),
        "new, removed or renamed builtins require an independent contract"
    );
    assert_eq!(
        contracts.iter().map(|c| c.tag).collect::<BTreeSet<_>>(),
        nash_plutus::builtin::DefaultFunction::ALL
            .iter()
            .map(|f| *f as usize)
            .collect(),
        "the calling-convention table must cover every runtime tag"
    );
    let mut source = String::from("module BuiltinCompilation exposing (..)\nimport Builtin\n");
    for contract in &contracts {
        for supplied in 0..=contract.arity {
            let parameters = (0..supplied).map(|i| format!(" p{i}")).collect::<String>();
            writeln!(
                source,
                "builtin{}x{supplied}{parameters} = Builtin.{}{parameters}",
                contract.tag, contract.name
            )
            .unwrap();
        }
    }
    let memory = InMemorySource::new();
    let main = Url::parse("file:///builtin-certificates/BuiltinCompilation.nash").unwrap();
    memory.insert(main.clone(), source);
    let mut modules = bundled_base::modules();
    modules.insert(main.clone(), None);
    let db = Arc::new(Mutex::new(Database::new(memory)));
    let graph = build_graph(db.clone(), &modules.keys().cloned().collect::<Vec<_>>())
        .await
        .unwrap();
    let (report, cases) = build_with(db, &graph, &modules, move |solved| {
        let arena = Arena::new();
        let build = Build::new(solved.modules.iter().map(|m| Input {
            module: m.module,
            types: &m.types,
            tables: &m.tables,
        }));
        let module = solved.modules.iter().find(|m| m.uri == main).unwrap();
        let data = arena.alloc(nash_region::Located::at_zero(Type::Named {
            reference: QualifiedName {
                home: primitives::primitive_home(),
                name: "Data",
            },
            args: &[],
        }));
        let mut cases = Vec::new();
        for contract in contracts {
            let function = nash_codegen::builtins::by_name(contract.name).unwrap();
            assert_eq!(function as usize, contract.tag, "{} opcode", contract.name);
            assert_eq!(
                function.force_count(),
                contract.forces,
                "{} forces",
                contract.name
            );
            assert_eq!(function.arity(), contract.arity, "{} arity", contract.name);
            for supplied in 0..=contract.arity {
                let name = format!("builtin{}x{supplied}", contract.tag);
                let annotation = &module.annotations[name.as_str()];
                // Instantiate representation-polymorphic parameters as Data;
                // their emitted UPLC calling convention is representation-free.
                let args = vec![&*data; annotation.free_vars.len()];
                let core = build
                    .compile(
                        &arena,
                        QualifiedName {
                            home: module.module.name,
                            name: &name,
                        },
                        Some(&args),
                        TraceConfig::default(),
                    )
                    .unwrap();
                let compiled = nash_codegen::program::assemble_core(&arena, core.core).unwrap();
                let syntax = captured(compiled.program.term);
                assert_eq!(
                    syntax,
                    reference(&contract, supplied),
                    "{} with {supplied} supplied arguments",
                    contract.name
                );
                let flat = flat::encode(compiled.program).unwrap();
                let decoded = flat::decode::<nash_plutus::binder::DeBruijn>(&arena, &flat).unwrap();
                assert_eq!(
                    captured(decoded.term),
                    syntax,
                    "{} Flat roundtrip",
                    contract.name
                );
                cases.push(Case {
                    contract: contract.clone(),
                    supplied,
                    flat,
                    syntax,
                });
            }
        }
        cases
    })
    .await;
    assert!(report.is_success(), "{:#?}", report.ordered_reports());
    cases.unwrap()
}

fn certificate(cases: &[Case]) -> String {
    let mut text = include_str!("fixtures/builtin-compilation/Certificate.lean").to_owned();
    for case in cases {
        let contract = &case.contract;
        let label = format!("case_{}_{}", contract.tag, case.supplied);
        let bind_result = contract.bind_result && case.supplied == contract.arity;
        writeln!(
            text,
            "\n-- Builtin.{} with {} supplied arguments",
            contract.name, case.supplied
        )
        .unwrap();
        writeln!(text, "def {label} : Syntax := {}", case.syntax.lean()).unwrap();
        writeln!(
            text,
            "theorem {label}_syntax : {label} = reference {} {} {} {bind_result} := by rfl",
            contract.tag, contract.forces, case.supplied
        )
        .unwrap();
        writeln!(text, "theorem {label}_observation {{α : Sort u}} (observe : Syntax → α) (args : List Syntax) : observe (args.foldl Syntax.apply {label}) = observe (args.foldl Syntax.apply (reference {} {} {} {bind_result})) := by rw [{label}_syntax]", contract.tag, contract.forces, case.supplied).unwrap();
        if let Some(builtin) = contract.lean {
            if case.supplied == 0 {
                writeln!(text, "theorem convention_{} : callingConvention (expectedArgs .{builtin}) = ({}, {}) := by rfl", contract.tag, contract.forces, contract.arity).unwrap();
            }
            writeln!(
                text,
                "def {label}_program : Program := flatEncodedScriptFromHexM \"{}\"",
                hex::encode(&case.flat)
            )
            .unwrap();
            writeln!(text, "theorem {label}_model : body {label}_program = referenceTerm .{builtin} {} {} {bind_result} := by rfl", contract.forces, case.supplied).unwrap();
            writeln!(text, "theorem {label}_cek (variant : PlutusCore.Default.BuiltinSemanticsVariant) (args : List Term) (fuel : Nat) : runKeepingFuel variant (initialState (applyParams (body {label}_program) args)) fuel = runKeepingFuel variant (initialState (applyParams (referenceTerm .{builtin} {} {} {bind_result}) args)) fuel := by rw [{label}_model]", contract.forces, case.supplied).unwrap();
            writeln!(text, "#print axioms {label}_cek").unwrap();
        }
        writeln!(text, "#print axioms {label}_observation").unwrap();
    }
    text.push_str("\nend NashBuiltinCompilation\n");
    text
}

#[tokio::test]
async fn all_builtin_references_and_application_prefixes_match_the_contract() {
    let cases = compile_cases().await;
    assert_eq!(cases.len(), 283);
    assert_eq!(
        cases.iter().filter(|c| c.contract.lean.is_some()).count(),
        254
    );
    // The independent specification catches opcode, forcing, and argument-order errors.
    let add = &cases
        .iter()
        .find(|c| c.contract.tag == 0 && c.supplied == 2)
        .unwrap()
        .contract;
    let mut wrong_tag = add.clone();
    wrong_tag.tag = 1;
    assert_ne!(reference(add, 2), reference(&wrong_tag, 2));
    let mut wrong_forces = add.clone();
    wrong_forces.forces = 1;
    assert_ne!(reference(add, 2), reference(&wrong_forces, 2));
    let reversed = Syntax::Apply(
        Box::new(Syntax::Apply(
            Box::new(Syntax::Builtin(0)),
            Box::new(Syntax::Var(0)),
        )),
        Box::new(Syntax::Var(1)),
    );
    let wrapped_reversed = Syntax::Apply(
        Box::new(Syntax::Lam(Box::new(Syntax::Var(0)))),
        Box::new(Syntax::Lam(Box::new(Syntax::Lam(Box::new(reversed))))),
    );
    assert_ne!(reference(add, 2), wrapped_reversed);
    // Always render the complete proof export, even without the external toolchain.
    let text = certificate(&cases);
    assert_eq!(text.matches("_syntax :").count(), cases.len());
    assert!(!text.contains("sorry"));
    assert!(!text.contains("native_decide"));
}

#[tokio::test]
#[ignore = "requires NASH_PROOF_LEAN_PROJECT with pinned PlutusCoreBlaster and Lean 4.24"]
async fn kernel_checks_builtin_compilation_certificates() {
    let cases = compile_cases().await;
    let directory =
        std::env::temp_dir().join(format!("nash-builtin-certificates-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("BuiltinCompilation.lean");
    std::fs::write(&path, certificate(&cases)).unwrap();
    let result = std::process::Command::new("lake")
        .args(["env", "lean"])
        .arg(&path)
        .current_dir(std::env::var_os("NASH_PROOF_LEAN_PROJECT").expect("Lean project"))
        .output()
        .unwrap();
    let diagnostics = format!(
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    std::fs::write(directory.join("diagnostics.txt"), &diagnostics).unwrap();
    assert!(
        result.status.success(),
        "{diagnostics}\nCertificate preserved at {}",
        path.display()
    );
    assert!(!diagnostics.contains("sorryAx"), "{diagnostics}");
    assert!(!diagnostics.contains("ofReduceBool"), "{diagnostics}");
    assert_eq!(
        diagnostics.matches("does not depend on any axioms").count(),
        283,
        "{diagnostics}"
    );
    let cek_audits = diagnostics
        .lines()
        .filter(|line| line.contains("_cek' "))
        .collect::<Vec<_>>();
    assert_eq!(cek_audits.len(), 254, "{diagnostics}");
    for audit in cek_audits {
        assert!(
            audit.ends_with("depends on axioms: [propext, Classical.choice, Quot.sound]"),
            "unexpected model assumptions: {audit}"
        );
    }
    println!(
        "101 builtins, 283 syntax certificates, 254 CEK certificates (91 modeled builtins); preserved at {}",
        path.display()
    );
}
