//! Compile proof bodies as UPLC functions over symbolic input domains.
use crate::{
    build::{Binding, Build, Engine, TraceConfig},
    decision_tree::{self, MatchBranch, MatchInputs},
};
use nash_ast::{Expr, ModuleName, NodeId, primitives};
use nash_ir::core::*;
use nash_plutus::{arena::Arena, flat};
pub use nash_proof::{Domain, ProofProgram};

#[derive(Debug, thiserror::Error)]
pub enum Error<'a> {
    #[error("unknown proof module {0:?}")]
    UnknownModule(ModuleName<'a>),
    #[error(
        "proof input must use a domain from the bundled Proof module; random generators and arbitrary values are not proof domains"
    )]
    Domain,
    #[error(
        "execution-budget constraints are not supported in proof blocks; use tests for measured budgets"
    )]
    Budget,
    #[error("ledger domain version must match the proof's Plutus version")]
    LedgerVersion,
    #[error("{0}")]
    Build(crate::build::Error<'a>),
    #[error("{0}")]
    Program(crate::program::Error<'a>),
    #[error("could not encode proof program: {0}")]
    Encoding(String),
}

impl<'a> From<crate::build::Error<'a>> for Error<'a> {
    fn from(error: crate::build::Error<'a>) -> Self {
        Self::Build(error)
    }
}
impl<'a> From<crate::program::Error<'a>> for Error<'a> {
    fn from(error: crate::program::Error<'a>) -> Self {
        Self::Program(error)
    }
}
impl<'a> From<crate::decision_tree::Error<'a>> for Error<'a> {
    fn from(error: crate::decision_tree::Error<'a>) -> Self {
        Self::Build(error.into())
    }
}

pub fn compile_proofs_matching<'a>(
    arena: &'a Arena,
    build: &Build<'a, '_>,
    module: ModuleName<'a>,
    version: nash_config::PlutusVersion,
    mut include: impl FnMut(&nash_ast::Test<'a>) -> bool,
) -> Result<Vec<ProofProgram>, Error<'a>> {
    let input = build
        .inputs
        .iter()
        .position(|i| i.module.name == module)
        .ok_or(Error::UnknownModule(module))?;
    let mut programs = Vec::new();
    for proof in build.inputs[input].module.proofs {
        if !include(proof) {
            continue;
        }
        if proof.budget.is_some() {
            return Err(Error::Budget);
        }
        let domains = proof
            .binders
            .iter()
            .map(|binder| {
                let Expr::VarForeign { reference, .. } = binder.generator.value else {
                    return Err(Error::Domain);
                };
                if reference.home.package != Some(primitives::BASE)
                    || reference.home.name != "Proof"
                {
                    return Err(Error::Domain);
                }
                let domain = Domain::named(reference.name).ok_or(Error::Domain)?;
                let target = match version {
                    nash_config::PlutusVersion::V1 => 1,
                    nash_config::PlutusVersion::V2 => 2,
                    nash_config::PlutusVersion::V3 => 3,
                };
                if matches!(domain, Domain::Spending(v) | Domain::Minting(v) if v != target) {
                    return Err(Error::LedgerVersion);
                }
                Ok(domain)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut engine = Engine::new(
            build,
            arena,
            TraceConfig {
                user: crate::build::TraceLevel::Silent,
                compiler: false,
            },
        );
        let mut ctx = engine.test_context(input);
        ctx.test = false; // Ordinary assertions: no test power-assert instrumentation.
        let mut params = Vec::new();
        let mut patterns = Vec::new();
        for binder in proof.binders {
            let ty = engine.ty(NodeId::pattern(binder.pattern), &ctx)?;
            let value = Binder {
                name: engine.ir.fresh("symbolic"),
                ty,
            };
            let (records, literals) = engine.pattern_inputs(binder.pattern, &ctx)?;
            let bindings = decision_tree::bindings(
                &engine.ir,
                &mut engine.types,
                ty,
                binder.pattern,
                &records,
            )?;
            for (name, bound) in &bindings {
                ctx.env.insert(name, Binding::Value(*bound));
            }
            params.push(value);
            patterns.push((binder, value, records, literals, bindings));
        }
        let mut body = engine.expr(proof.body, &ctx)?;
        for (binder, value, records, literals, bindings) in patterns.into_iter().rev() {
            body = decision_tree::compile(
                &engine.ir,
                &mut engine.types,
                value.ty,
                engine.ir.var(value.name, value.ty),
                &[MatchBranch {
                    pattern: binder.pattern,
                    bindings,
                    body,
                }],
                MatchInputs {
                    record_fields: &records,
                    literal_tests: &literals,
                },
                engine.ir.error(body.ty),
            )?;
        }
        let root = if params.is_empty() {
            body
        } else {
            engine.ir.lam(&params, body)
        };
        let core = engine.finish_root(root)?;
        let target = match version {
            nash_config::PlutusVersion::V1 => nash_plutus::machine::PlutusVersion::V1,
            nash_config::PlutusVersion::V2 => nash_plutus::machine::PlutusVersion::V2,
            nash_config::PlutusVersion::V3 => nash_plutus::machine::PlutusVersion::V3,
        };
        let compiled = crate::program::assemble_core_for_version(arena, core, target)?;
        programs.push(ProofProgram {
            module: module.name.to_owned(),
            name: proof.name.value.to_owned(),
            expect: proof.expect,
            domains,
            flat: flat::encode(compiled.program).map_err(|e| Error::Encoding(e.to_string()))?,
            plutus_version: version,
        });
    }
    Ok(programs)
}
