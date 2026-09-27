---
cargo/nash-driver: patch
---

Scan each module for its reserved-name check and imports with a single parse, and run the scans concurrently on Tokio's blocking pool.
