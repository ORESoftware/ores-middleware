import "@typespec/json-schema";

namespace Ores.Middleware.Persistence;

// Independent TypeSpec authority for one persistence-bearing middleware model.
// Retain these declarations for the native persistence generator.
// @ores.sql.table middleware_idempotency
// @ores.sql.primary-key id
// @ores.sql.unique tenantId,idempotencyKey

enum IdempotencyStatus {
  pending: "pending",
  succeeded: "succeeded",
  failed: "failed",
}

// State the existing flat closed wire shape and SQL policy in compiler-visible form.
// Both closure keywords are explicit peer-source policy for this non-composed model;
// they are not a general rewrite rule for inherited/composed schemas.
@TypeSpec.JsonSchema.extension("additionalProperties", false)
@TypeSpec.JsonSchema.extension("unevaluatedProperties", false)
@TypeSpec.JsonSchema.extension("x-ores-sql", #{
  table: "middleware_idempotency",
  primaryKey: #["id"],
  unique: #[#["tenantId", "idempotencyKey"]],
})
model IdempotencyRecord {
  id: string;
  tenantId: string;
  idempotencyKey: string;
  requestHash: string;
  status: IdempotencyStatus;
  responseStatus?: int32;
  responseBody?: string;
  createdAt: utcDateTime;
  expiresAt: utcDateTime;
}
