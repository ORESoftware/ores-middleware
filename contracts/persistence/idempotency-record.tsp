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

// State the existing closed wire shape and SQL policy in compiler-visible form.
// These are authored declarations, not copied from generated comparison output.
@TypeSpec.JsonSchema.extension("additionalProperties", false)
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
