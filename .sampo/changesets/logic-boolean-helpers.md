---
cargo/nash-can: minor
cargo/nash-codegen: patch
cargo/nash-driver: minor
---

Move boolean helpers and &&/|| into Logic, below Eq and Ord. Preserve fully
applied short-circuit behavior and default application scope. Qualified callers
must use Logic.and/or/not/xor instead of Bool; explicit operator imports use
Logic instead of Prelude. Simplify equality and boolean ordering with the helpers.
