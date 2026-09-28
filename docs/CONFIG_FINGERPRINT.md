# Configuration fingerprint contract

Fingerprints are computed only from normalized **non-secret** configuration and the names/metadata of secret bindings. Raw secret values, hashes of raw secrets, cookies, bearer tokens, private keys, passwords, Redis credentials, JWTs, and HMAC material are excluded from the externally visible fingerprint preimage.

This contract is evidence only; runtime-specific normalization remains owned by each supported adapter until convergence tests bind them.
