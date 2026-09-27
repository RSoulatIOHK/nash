//! Shared executable Core fixture harness.
use std::collections::BTreeMap;

use nash_ir::core::Core;
use nash_plutus::{
    arena::Arena,
    debruijn,
    machine::ExBudget,
    pretty,
    program::{Program, Version},
};

pub fn dependency_order<'a>(
    sources: impl IntoIterator<Item = (&'a str, Option<nash_ast::PackageName<'a>>)>,
) -> Vec<(&'a str, Option<nash_ast::PackageName<'a>>)> {
    let bump = bumpalo::Bump::new();
    let sources: BTreeMap<_, _> = sources
        .into_iter()
        .map(|(source, package)| {
            let parsed = nash_parse::Parser::new(&bump, source).module().unwrap();
            (parsed.name.unwrap().value, (source, package, parsed))
        })
        .collect();
    let uri = |name| url::Url::parse(&format!("file:///fixtures/{name}.nash")).unwrap();
    let mut graph = nash_driver::DepGraph::new();
    let mut modules = BTreeMap::new();
    for (name, (source, package, parsed)) in &sources {
        let mut imports: Vec<_> = parsed
            .imports
            .iter()
            .chain(
                parsed
                    .tests
                    .into_iter()
                    .flat_map(|tests| tests.imports.iter()),
            )
            .map(|import| uri(import.import.value))
            .collect();
        if *package != Some(nash_ast::primitives::BASE) {
            // Fixture applications receive all supplied Base interfaces.
            imports.extend(
                sources
                    .iter()
                    .filter(|(_, (_, package, _))| *package == Some(nash_ast::primitives::BASE))
                    .map(|(name, _)| uri(name)),
            );
        }
        imports.sort();
        imports.dedup();
        let module_uri = uri(name);
        graph.add_module(module_uri.clone(), imports);
        modules.insert(module_uri, (*source, *package));
    }
    graph.compute_order().expect("acyclic fixture imports");
    graph
        .order
        .iter()
        .filter_map(|uri| modules.remove(uri))
        .collect()
}

pub struct Evaluated {
    pub uplc: String,
    pub result: String,
    pub logs: Vec<String>,
    pub budget: ExBudget,
    // Function values need application tests, not syntax equality.
    pub(crate) observable: Option<String>,
}

impl std::fmt::Display for Evaluated {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "--- uplc\n{}\n--- result\n{}\n--- logs\n{:?}\n--- budget\ncpu: {}, memory: {}",
            self.uplc, self.result, self.logs, self.budget.cpu, self.budget.mem
        )
    }
}

pub fn eval_core<'a>(arena: &'a Arena, core: &'a Core<'a>) -> Evaluated {
    let baseline = eval_core_raw(arena, core);
    assert_candidate_equivalent(arena, core, &baseline);
    baseline
}

pub(crate) fn assert_candidate_equivalent<'a>(
    arena: &'a Arena,
    core: &'a Core<'a>,
    baseline: &Evaluated,
) {
    let candidate = crate::anf_tests::candidate(arena, core);
    let named =
        crate::lower::lower_with_constant_sharing(arena, candidate).expect("shared lowering");
    let normalized = eval_named(arena, named);
    assert_eq!(
        baseline.observable, normalized.observable,
        "candidate passes preserve ground results and error category"
    );
    assert_eq!(
        baseline.logs, normalized.logs,
        "candidate passes preserve trace order"
    );
}

pub(crate) fn eval_core_raw<'a>(arena: &'a Arena, core: &'a Core<'a>) -> Evaluated {
    let named = crate::lower::lower(arena, core).expect("valid lowered Core");
    eval_named(arena, named)
}

pub(crate) fn eval_named<'a>(
    arena: &'a Arena,
    named: &'a nash_plutus::term::Term<'a, nash_plutus::binder::Name<'a>>,
) -> Evaluated {
    let term = debruijn::to_debruijn(arena, named).expect("closed term");
    let program = Program::new(arena, Version::plutus_v3(arena), term);
    let evaluation = program.eval(arena);
    fn ground(term: &nash_plutus::term::Term<'_, nash_plutus::binder::DeBruijn>) -> bool {
        match term {
            nash_plutus::term::Term::Constant(_) => true,
            nash_plutus::term::Term::Constr { fields, .. } => {
                fields.iter().all(|field| ground(field))
            }
            _ => false,
        }
    }
    let observable = match &evaluation.term {
        Ok(term) if ground(term) => Some(pretty::term(term)),
        Ok(_) => None,
        Err(error) => Some(format!(
            "error: {:?}: {error}",
            std::mem::discriminant(error)
        )),
    };
    Evaluated {
        observable,
        uplc: pretty::program(Program::new(arena, Version::plutus_v3(arena), named)),
        result: match evaluation.term {
            Ok(term) => pretty::term(term),
            Err(error) => format!("error: {error:?}"),
        },
        logs: evaluation.info.logs,
        budget: evaluation.info.consumed_budget,
    }
}
