---
cargo/nash-ir: minor
cargo/nash-codegen: patch
---

Add static-parameter lifting, typed A-normalization and structural invariant
checks for Core. Preserve application staging and branch, lambda, delay and trace
execution boundaries. Support explicitly delayed recursive workers after lifting
all static parameters.
