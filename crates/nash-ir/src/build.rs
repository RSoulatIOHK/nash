//! Arena constructors. Use one builder per program to keep names unique.
use crate::{
    core::*,
    ty::{BigTy, ConstTy, RuntimeTy, TermTy, Ty},
};
use nash_plutus::{arena::Arena, builtin::DefaultFunction, constant::Constant};
use std::cell::Cell;

pub struct Builder<'a> {
    pub arena: &'a Arena,
    next_unique: Cell<u32>,
}

impl<'a> Builder<'a> {
    pub fn new(arena: &'a Arena) -> Self {
        Self {
            arena,
            next_unique: Cell::new(1),
        }
    }
    pub fn fresh(&self, text: &'a str) -> Name<'a> {
        let unique = self.next_unique.get();
        self.next_unique
            .set(unique.checked_add(1).expect("Core name supply exhausted"));
        Name { text, unique }
    }
    /// Construct a node with its established result type. Producers supply the
    /// type; later passes do not recover it from untyped expression shapes.
    pub fn alloc(&self, ty: Ty<'a>, kind: CoreKind<'a>) -> &'a Core<'a> {
        self.arena.alloc(Core { ty, kind })
    }
    /// Retain source-level nominal metadata around an existing runtime shape.
    /// This does not convert the value or infer a type.
    pub fn with_type(&self, core: &'a Core<'a>, ty: Ty<'a>) -> &'a Core<'a> {
        if core.ty == ty {
            core
        } else {
            self.alloc(ty, core.kind)
        }
    }
    pub fn var(&self, name: Name<'a>, ty: Ty<'a>) -> &'a Core<'a> {
        self.alloc(ty, CoreKind::Var(name))
    }
    pub fn lit(&self, value: &'a Constant<'a>) -> &'a Core<'a> {
        self.alloc(self.constant_type(value), CoreKind::Lit(value))
    }
    pub fn int(&self, value: i128) -> &'a Core<'a> {
        self.lit(Constant::integer_from(self.arena, value))
    }
    pub fn lam(&self, params: &[Binder<'a>], body: &'a Core<'a>) -> &'a Core<'a> {
        let types: Vec<_> = params.iter().map(|p| p.ty).collect();
        let ty = if params.is_empty() {
            body.ty
        } else {
            Ty::Term(
                self.arena
                    .alloc(TermTy::Fun(self.arena.alloc_slice_copy(&types), body.ty)),
            )
        };
        self.alloc(
            ty,
            CoreKind::Lam {
                params: self.arena.alloc_slice_copy(params),
                body,
            },
        )
    }
    pub fn app(&self, func: &'a Core<'a>, args: &[&'a Core<'a>], ty: Ty<'a>) -> &'a Core<'a> {
        self.alloc(
            ty,
            CoreKind::App {
                func,
                args: self.arena.alloc_slice_copy(args),
            },
        )
    }
    pub fn let_(
        &self,
        binder: Binder<'a>,
        value: &'a Core<'a>,
        body: &'a Core<'a>,
    ) -> &'a Core<'a> {
        self.alloc(
            body.ty,
            CoreKind::Let {
                binder,
                value,
                body,
            },
        )
    }
    pub fn let_rec(&self, binders: &[RecBinder<'a>], body: &'a Core<'a>) -> &'a Core<'a> {
        self.alloc(
            body.ty,
            CoreKind::LetRec {
                binders: self.arena.alloc_slice_copy(binders),
                body,
            },
        )
    }
    pub fn case(
        &self,
        kind: CaseKind,
        scrutinee: &'a Core<'a>,
        branches: &[Branch<'a>],
        default: Option<&'a Core<'a>>,
        ty: Ty<'a>,
    ) -> &'a Core<'a> {
        self.alloc(
            ty,
            CoreKind::Case {
                kind,
                scrutinee,
                branches: self.arena.alloc_slice_copy(branches),
                default,
            },
        )
    }
    pub fn if_(
        &self,
        condition: &'a Core<'a>,
        yes: &'a Core<'a>,
        no: &'a Core<'a>,
    ) -> &'a Core<'a> {
        self.case(
            CaseKind::Bool,
            condition,
            &[
                Branch {
                    test: Test::True,
                    binders: &[],
                    body: yes,
                },
                Branch {
                    test: Test::False,
                    binders: &[],
                    body: no,
                },
            ],
            None,
            yes.ty,
        )
    }
    pub fn constr(&self, tag: u16, fields: &[&'a Core<'a>], ty: Ty<'a>) -> &'a Core<'a> {
        self.alloc(
            ty,
            CoreKind::Constr {
                tag,
                fields: self.arena.alloc_slice_copy(fields),
            },
        )
    }
    pub fn field(&self, record: &'a Core<'a>, index: u16, arity: u16, ty: Ty<'a>) -> &'a Core<'a> {
        assert!(
            index < arity,
            "field index must be within constructor arity"
        );
        self.alloc(
            ty,
            CoreKind::Field {
                record,
                index,
                arity,
            },
        )
    }
    pub fn builtin(
        &self,
        func: DefaultFunction,
        args: &[&'a Core<'a>],
        ty: Ty<'a>,
    ) -> &'a Core<'a> {
        assert!(args.len() <= func.arity(), "builtin arguments exceed arity");
        self.alloc(
            ty,
            CoreKind::Builtin {
                func,
                args: self.arena.alloc_slice_copy(args),
            },
        )
    }
    pub fn trace(&self, message: &'a Core<'a>, body: &'a Core<'a>) -> &'a Core<'a> {
        self.alloc(body.ty, CoreKind::Trace { message, body })
    }
    pub fn error(&self, ty: Ty<'a>) -> &'a Core<'a> {
        self.alloc(ty, CoreKind::Error)
    }
    pub fn delay(&self, body: &'a Core<'a>) -> &'a Core<'a> {
        self.alloc(
            Ty::Runtime(self.arena.alloc(RuntimeTy::Delay(body.ty))),
            CoreKind::Delay(body),
        )
    }
    pub fn force(&self, body: &'a Core<'a>, ty: Ty<'a>) -> &'a Core<'a> {
        self.alloc(ty, CoreKind::Force(body))
    }
    fn constant_type(&self, value: &Constant<'a>) -> Ty<'a> {
        let ty = match value {
            Constant::Integer(_) => ConstTy::Int,
            Constant::ByteString(_) => ConstTy::Bytes,
            Constant::String(_) => ConstTy::String,
            Constant::Boolean(_) => ConstTy::Bool,
            Constant::Data(_) => return Ty::Big(&BigTy::Data),
            Constant::ProtoList(t, _) => ConstTy::List(self.plutus_type(t)),
            Constant::ProtoArray(t, _) => ConstTy::Array(self.plutus_type(t)),
            Constant::ProtoPair(a, b, _, _) => {
                ConstTy::Pair(self.plutus_type(a), self.plutus_type(b))
            }
            Constant::Unit => ConstTy::Unit,
            Constant::Bls12_381G1Element(_) => ConstTy::BlsG1,
            Constant::Bls12_381G2Element(_) => ConstTy::BlsG2,
            Constant::Bls12_381MlResult(_) => ConstTy::BlsMlr,
            Constant::Value(_) => ConstTy::Value,
        };
        Ty::Const(self.arena.alloc(ty))
    }
    fn plutus_type(&self, value: &nash_plutus::typ::Type<'a>) -> Ty<'a> {
        use nash_plutus::typ::Type;
        let ty = match value {
            Type::Integer => ConstTy::Int,
            Type::ByteString => ConstTy::Bytes,
            Type::String => ConstTy::String,
            Type::Bool => ConstTy::Bool,
            Type::Data => return Ty::Big(&BigTy::Data),
            Type::List(t) => ConstTy::List(self.plutus_type(t)),
            Type::Array(t) => ConstTy::Array(self.plutus_type(t)),
            Type::Pair(a, b) => ConstTy::Pair(self.plutus_type(a), self.plutus_type(b)),
            Type::Unit => ConstTy::Unit,
            Type::Bls12_381G1Element => ConstTy::BlsG1,
            Type::Bls12_381G2Element => ConstTy::BlsG2,
            Type::Bls12_381MlResult => ConstTy::BlsMlr,
            Type::Value => ConstTy::Value,
        };
        Ty::Const(self.arena.alloc(ty))
    }
}

#[cfg(test)]
mod tests;
