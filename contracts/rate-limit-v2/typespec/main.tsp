import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored rate-limit policy authority.
 * This file is not generated from JSON Schema and must pass the parity gate.
 */
namespace Ores.Middleware.RateLimitV2;

enum RateLimitAlgorithmV2 {
  tokenBucket: "token-bucket",
  slidingWindowCounter: "sliding-window-counter",
  fixedWindow: "fixed-window",
  gcra: "gcra",
  concurrency: "concurrency",
}

enum RateLimitConsistency {
  strict: "strict",
  bounded: "bounded",
  advisory: "advisory",
}

enum RateLimitEnforcementMode {
  disabled: "disabled",
  audit: "audit",
  enforce: "enforce",
}

enum RateLimitFailureMode {
  failOpen: "fail-open",
  failClosed: "fail-closed",
  localOnly: "local-only",
}

enum RateLimitLayer {
  cloudflareEdge: "cloudflare-edge",
  kubernetesIngress: "kubernetes-ingress",
  serviceMesh: "service-mesh",
  application: "application",
  authorization: "authorization",
}

enum OperationClass {
  healthRead: "health-read",
  publicRead: "public-read",
  authAttempt: "auth-attempt",
  authRecovery: "auth-recovery",
  mutation: "mutation",
  paymentOrLedgerWrite: "payment-or-ledger-write",
  jobAdmission: "job-admission",
}

/**
 * Version-two policy used by application middleware and audit rollouts.
 * Conditional algorithm and consistency invariants are enforced independently
 * by Rust runtime validation and the peer JSON Schema authority.
 */
model RateLimitPolicyV2 {
  policyId: string;
  operationClass: OperationClass;
  algorithm: RateLimitAlgorithmV2;
  consistency: RateLimitConsistency;
  enforcementMode: RateLimitEnforcementMode;
  failureMode: RateLimitFailureMode;
  layer: RateLimitLayer;
  capacity: uint64;
  windowMs?: uint64;
  refillTokens?: uint64;
  refillIntervalMs?: uint64;
  maximumOvershoot: uint64;
  localDenyCacheEntries: uint32;
  coordinatorRequired: boolean;
  keyVersion: string;
}
