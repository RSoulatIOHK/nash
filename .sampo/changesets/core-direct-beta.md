---
cargo/nash-ir: minor
---

Add direct lambda application reduction with strict ANF bindings and iterate it with alias propagation to a fixed point.

Permit repeated-use propagation of integer and BLS constants and byte strings up to 64 bytes.
