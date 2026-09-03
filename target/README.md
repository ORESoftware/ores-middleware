# Generated targets

Build and audit commands write only below this directory:

- `target/audit/` — machine-readable execution receipts;
- `target/discrepancies/` — fail-closed semantic differences;
- `target/rust/`, `target/ts/`, `target/golang/`, and future language folders — build output.

Nothing under `target/` is a human-authored contract authority and generated
output must never overwrite `contracts/`.
