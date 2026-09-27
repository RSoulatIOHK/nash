---
cargo/nash-ir: minor
cargo/nash-codegen: minor
---

Require result types on every Core node and retain source and compiler-generated
runtime metadata through codegen and recursion rewriting. Core operation variants
move to CoreKind; builder APIs require result types where they cannot be derived.
