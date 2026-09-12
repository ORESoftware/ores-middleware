/** Independently authored peer of idempotency-scope.schema.json. */
namespace Ores.Middleware;

model IdempotencyScope {
  @minLength(1) serviceName: string;
  tenantId: string;
  userId: string;
  @minLength(1) method: string;
  @minLength(1) @pattern("^/") path: string;
  query: string;
  @minLength(1) idempotencyKey: string;
}
