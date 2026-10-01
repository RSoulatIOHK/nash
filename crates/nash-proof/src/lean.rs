use crate::{Domain, ProofProgram};
use nash_source::Expect;

pub fn lakefile() -> &'static str {
    r#"import Lake
open Lake DSL
package NashProof
require Blaster from git "https://github.com/input-output-hk/Lean-blaster" @ "bafdd4f7976037cd7bd8e443df04af096dc5b96e"
require PlutusCore from git "https://github.com/input-output-hk/PlutusCoreBlaster" @ "41fe7eadf460dc66bef22b85656bc36408638bdc"
require CardanoLedgerApi from git "https://github.com/input-output-hk/CardanoLedgerApiBlaster" @ "3f9f7c6b6ecf0012e1397bab0a814daa36e70ba2"
"#
}

/// A command-level result protocol avoids admissions and human-log parsing.
const SUPPORT: &str = r##"import Blaster
import PlutusCore.UPLC
import CardanoLedgerApi.V1
import CardanoLedgerApi.V2
import CardanoLedgerApi.V3
open PlutusCore.UPLC.Term (Term Const)
open PlutusCore.UPLC.Utils
open CardanoLedgerApi.IsData.Class (toTerm)
set_option maxRecDepth 100000
set_option maxHeartbeats 0

namespace NashProof

open PlutusCore.UPLC.CekMachine
open PlutusCore.UPLC.PlutusScript
open PlutusCore.UPLC.Term (Program)

-- Preserve a running state at fuel zero. Upstream runSteps turns it into Error.
def runSteps (semantics : PlutusCore.Default.BuiltinSemanticsVariant) (state : State) (fuel : Nat) : State :=
  match fuel, state with
  | _, .Halt _ => state
  | _, .Error => state
  | 0, _ => state
  | fuel + 1, _ => runSteps semantics (step semantics state) fuel

def execute (script : PlutusScript) (params : List PlutusCore.UPLC.Term.Term) (fuel : Nat) : State :=
  let semantics : PlutusCore.Default.BuiltinSemanticsVariant := match script.lang with
    | .PlutusV1 | .PlutusV2 => .defaultFunSemanticsVariantD
    | .PlutusV3 => .defaultFunSemanticsVariantE
  match script.script with
  | .Program _ body => runSteps semantics (initialState (applyParams body params)) fuel

-- Rejection includes an unfinished state at the configured execution limit.
def isRejected (state : State) : Prop := ¬ PlutusCore.UPLC.Utils.isSuccessful state

open Lean Elab Command Term Meta

syntax (name := prepare) "#nash_prep" ident ident ident num : command
@[command_elab prepare]
def prepareImpl : CommandElab := fun stx => do
  let declaration ← withoutModifyingEnv <| runTermElabM fun _ => do
    let some script ← resolveId? stx[2] | throwError "unknown proof script"
    let some inputs ← resolveId? stx[3] | throwError "unknown proof input conversion"
    let application ← Meta.lambdaTelescope (← Meta.etaExpand inputs) fun xs _ =>
      mkLambdaFVars xs (mkApp3 (mkConst ``execute) script
        (mkAppN inputs xs) (mkNatLit (TSyntax.getNat ⟨stx[4]⟩)))
    let (expression, _) ← Blaster.Optimize.Optimize.main application |>.run default
    return Declaration.defnDecl {
      name := stx[1].getId, levelParams := [], type := ← inferType expression,
      value := expression, hints := .abbrev, safety := .safe }
  modifyEnv (addNoncomputable · stx[1].getId)
  liftCoreM <| addDecl declaration

syntax (name := verify) "#nash_verify" str num "[" term "]" : command
@[command_elab verify]
def verifyImpl : CommandElab := fun stx => do
  let saved ← get
  let result ← runTermElabM fun _ => do
    let expression ← instantiateMVars (← withSynthesize (postpone := .partial) <| elabTerm stx[4] none)
    let options : Blaster.Options.BlasterOptions := { timeout := some (TSyntax.getNat ⟨stx[2]⟩) }
    let env : Blaster.Optimize.TranslateEnv := { (default : Blaster.Optimize.TranslateEnv) with optEnv.options.solverOptions := options }
    let ((result, _), _) ← Blaster.Smt.Translate.main expression |>.run env
    return result
  -- Blaster logs a mismatched expected result as an elaboration error. The structured
  -- protocol handles all outcomes, so remove only this command's solver messages.
  set saved
  let (status, cex) := match result with
    | .Valid => ("valid", ([] : List String))
    | .Falsified cex => ("falsified", cex)
    | .Undetermined => ("unknown", [])
  let json := Json.mkObj [("query", toJson (TSyntax.getString ⟨stx[1]⟩)), ("status", toJson status), ("counterexample", toJson cex)]
  liftIO <| IO.println ("@@NASH_PROOF@@" ++ json.compress)
end NashProof
"##;

pub fn render(program: &ProofProgram, fuel: u32, timeout: u32) -> String {
    let mut text = SUPPORT.to_owned();
    let version = match program.plutus_version {
        nash_config::PlutusVersion::V1 => 1,
        nash_config::PlutusVersion::V2 => 2,
        nash_config::PlutusVersion::V3 => 3,
    };
    // Embed flat bytes, so moving an exported project cannot break script paths.
    text.push_str(&format!("\ndef script : PlutusCore.UPLC.PlutusScript.PlutusScript :=\n  {{ lang := .PlutusV{version}, script := flatEncodedScriptFromHexM \"{}\" }}\n", hex::encode(&program.flat)));
    let mut binders = String::new();
    let mut args = Vec::new();
    let mut terms = Vec::new();
    let mut assumptions = Vec::new();
    for (i, domain) in program.domains.iter().enumerate() {
        let name = format!("input{i}");
        let (ty, term, assumption) = domain.input(&name);
        binders.push_str(&format!(" ({name} : {ty})"));
        args.push(name);
        terms.push(term);
        if let Some(assumption) = assumption {
            assumptions.push(assumption);
        }
    }
    text.push_str(&format!(
        "\ndef inputs{binders} : List Term := [{}]\n",
        terms.join(", ")
    ));
    text.push_str(&format!("\n#nash_prep prepared script inputs {fuel}\n"));
    let state = format!("(prepared {})", args.join(" "));
    let claim = match program.expect {
        Expect::Pass => format!("isSuccessful {state}"),
        Expect::Fail => format!("NashProof.isRejected {state}"),
        // Refuting universal success finds a script error or fuel exhaustion.
        Expect::FailOnce => format!("isSuccessful {state}"),
    };
    let quantify = |body: &str| {
        let pre = if assumptions.is_empty() {
            String::new()
        } else {
            format!("{} → ", assumptions.join(" → "))
        };
        if binders.is_empty() {
            format!("{pre}{body}")
        } else {
            format!("∀{binders}, {pre}{body}")
        }
    };
    text.push_str(&format!(
        "\n#nash_verify \"property\" {timeout} [{}]\n",
        quantify(&claim)
    ));
    text
}

impl Domain {
    fn input(self, name: &str) -> (String, String, Option<String>) {
        let (ty, term) = match self {
            Self::Int => (
                "PlutusCore.Integer.Integer",
                format!("Term.Const (Const.Integer {name})"),
            ),
            Self::Integer => ("PlutusCore.Integer.Integer", format!("toTerm {name}")),
            Self::Bool => ("Bool", format!("Term.Const (Const.Bool {name})")),
            Self::Bytes => (
                "PlutusCore.ByteString.ByteString",
                format!("Term.Const (Const.ByteString {name})"),
            ),
            Self::ByteString => ("PlutusCore.ByteString.ByteString", format!("toTerm {name}")),
            Self::String => ("String", format!("Term.Const (Const.String {name})")),
            Self::Data => ("PlutusCore.Data.Data", format!("toTerm {name}")),
            Self::Spending(v) | Self::Minting(v) => {
                let purpose = if matches!(self, Self::Spending(_)) {
                    "Spending"
                } else {
                    "Minting"
                };
                let ty = if v == 3 {
                    format!("CardanoLedgerApi.V{v}.ScriptContext")
                } else {
                    format!("CardanoLedgerApi.V{v}.{purpose}Input")
                };
                let term = if v == 3 {
                    format!("toTerm {name}")
                } else {
                    format!("toTerm {name}.ctx")
                };
                return (
                    ty,
                    term,
                    Some(format!(
                        "CardanoLedgerApi.V{v}.valid{purpose}Context {name}"
                    )),
                );
            }
        };
        (ty.to_owned(), term, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ledger_input_retains_validity_and_bytecode() {
        let program = ProofProgram {
            module: "M".into(),
            name: "p".into(),
            expect: Expect::Pass,
            domains: vec![Domain::Spending(3)],
            flat: vec![1, 2, 3],
            plutus_version: nash_config::PlutusVersion::V3,
        };
        let source = render(&program, 1000, 5);
        assert!(source.contains("flatEncodedScriptFromHexM \"010203\""));
        assert!(source.contains("∀ (input0 : CardanoLedgerApi.V3.ScriptContext), CardanoLedgerApi.V3.validSpendingContext input0 →"));
        assert!(source.contains("isSuccessful (prepared input0)"));
        assert!(!source.contains("by blaster"));
    }
}
