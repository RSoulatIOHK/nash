import PlutusCore.UPLC.ScriptEncoding
import PlutusCore.UPLC.Builtins
import PlutusCore.UPLC.CekMachine

/- Kernel certificates for captured O0 output, not a formalization of the Rust compiler.
   The reference is independent of Nash's builtin metadata. No SMT or admitted theorem
   is used. The generic syntax includes tags missing from PlutusCoreBlaster's model. -/
namespace NashBuiltinCompilation

open PlutusCore.UPLC.Term
open PlutusCore.UPLC.ScriptEncoding
open PlutusCore.UPLC.Builtins
open PlutusCore.UPLC.CekMachine

inductive Syntax where
  | var : Nat → Syntax
  | lam : Syntax → Syntax
  | apply : Syntax → Syntax → Syntax
  | force : Syntax → Syntax
  | builtin : Nat → Syntax

def forced (tag : Nat) : Nat → Syntax
  | 0 => .builtin tag
  | n + 1 => .force (forced tag n)

def lambdas : Nat → Syntax → Syntax
  | 0, term => term
  | n + 1, term => .lam (lambdas n term)

def reference (tag forces supplied : Nat) (bindResult : Bool) : Syntax :=
  let call := (List.range supplied).foldl
    (fun term i => .apply term (.var (supplied - i - 1))) (forced tag forces)
  let call := if bindResult then .apply (.lam (.var 0)) call else call
  .apply (.lam (.var 0)) (lambdas supplied call)

def callingConvention : ExpectedBuiltinArgs → Nat × Nat
  | .One .ArgQ => (1, 0)
  | .One .ArgV => (0, 1)
  | .More arg rest =>
      let (forces, values) := callingConvention rest
      match arg with
      | .ArgQ => (forces + 1, values)
      | .ArgV => (forces, values + 1)

def forcedTerm (builtin : BuiltinFun) : Nat → Term
  | 0 => .Builtin builtin
  | n + 1 => .Force (forcedTerm builtin n)

-- Match the Flat decoder's display-only binder names as well as its relative indices.
def lambdaTerms : Nat → Nat → Term → Term
  | 0, _, term => term
  | n + 1, depth, term => .Lam s!"dbi_{depth}" (lambdaTerms n (depth + 1) term)

def referenceTerm (builtin : BuiltinFun) (forces supplied : Nat) (bindResult : Bool) : Term :=
  let call := (List.range supplied).foldl
    (fun term i => .Apply term (.Var (supplied - i - 1))) (forcedTerm builtin forces)
  let call := if bindResult then .Apply (.Lam s!"dbi_{supplied}" (.Var 0)) call else call
  .Apply (.Lam "dbi_0" (.Var 0)) (lambdaTerms supplied 0 call)

def body : Program → Term
  | .Program _ term => term

-- Preserve unfinished states at exhaustion, as Nash's proof backend does.
def runKeepingFuel (variant : PlutusCore.Default.BuiltinSemanticsVariant)
    (state : State) (fuel : Nat) : State :=
  match fuel, state with
  | _, .Halt _ => state
  | _, .Error => state
  | 0, _ => state
  | n + 1, _ => runKeepingFuel variant (step variant state) n
