import "@typespec/json-schema";

namespace Ores.Middleware.Persistence;

// Independent TypeSpec authority for one persistence-bearing middleware model.
// @ores.sql.table middleware_idempotency
// @ores.sql.primary-key id
// @ores.sql.unique tenantId,idempotencyKey

enum IdempotencyStatus {
  pending: "pending",
  succeeded: "succeeded",
  failed: "failed",
}

// Keep the legacy SQL markers above for the independent persistence generator.
// These executable annotations expose the same intent to TJSV's compiler lane.
// Neither annotation nor comparison output is generated from the peer authority.
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
