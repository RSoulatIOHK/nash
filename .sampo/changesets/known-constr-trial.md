---
cargo/nash-ir: minor
---

Add a standalone Core pass for folding direct native-constructor cases while preserving strict field evaluation.

Group Boolean and constructor folding in `known_case`, and unused-binding and recursive reachability cleanup in `dead_code`.
