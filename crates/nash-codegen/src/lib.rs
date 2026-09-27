//! Compile-time specialization and lowering to Untyped Plutus Core.
pub mod lower;
pub mod recursion;

#[cfg(test)]
pub(crate) mod harness;

pub mod builtins;

pub mod ty_of;

pub mod demand;

pub mod decision_tree;

pub mod evidence;

pub mod comptime;
pub mod program;

mod assertion;
pub mod build;
pub mod can_to_core;
pub mod tests;

#[cfg(test)]
mod anf_tests;
#[cfg(test)]
mod beta_tests;
#[cfg(test)]
mod propagate_tests;
#[cfg(test)]
mod single_use_tests;

#[cfg(test)]
mod small_inline_tests;

#[cfg(test)]
mod builtin_sharing_tests;

#[cfg(test)]
mod constant_sharing_tests;

#[cfg(test)]
mod dead_bindings_tests;
