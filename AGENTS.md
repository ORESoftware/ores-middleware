# Repository agent instructions

This repository inherits and must follow [`ORESoftware/my-ai/AGENTS.md`](https://github.com/ORESoftware/my-ai/blob/main/AGENTS.md) and `original-agents.md`.

Additional repository rules:

- TypeSpec and JSON Schema/OpenAPI are independent, top-level, human-authored contract authorities. Neither may be generated from the other and treated as canonical.
- Any discrepancy between the two authorities, generated artifacts, or language adapter descriptors is a fail-closed release gate.
- Never commit credentials, decrypted environment files, or request payloads containing secrets or personal data.
- Middleware test bypass and fault injection are disabled unless the runtime environment is explicitly `test` or `staging`, and production startup must fail if either is enabled.
- TLS may terminate in-process or at a trusted proxy. Forwarded headers are ignored unless the peer is in the configured trusted-proxy set.
