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
