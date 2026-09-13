/** Independently authored peer of idempotency-scope.schema.json. */
import "@typespec/json-schema";

namespace Ores.Middleware;

// The wire object is intentionally closed. The independent Draft 2020-12
// authority expresses the same policy with unevaluatedProperties: false, and
// the adversarial corpus includes an unexpected-property rejection witness.
@TypeSpec.JsonSchema.extension("unevaluatedProperties", false)
model IdempotencyScope {
  @minLength(1) serviceName: string;
  tenantId: string;
  userId: string;
  @minLength(1) method: string;
  @minLength(1) @pattern("^/") path: string;
  query: string;
  @minLength(1) idempotencyKey: string;
}