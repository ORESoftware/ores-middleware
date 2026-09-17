import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored route-class policy inheritance authority.
 * Request classification happens before this layer. This contract resolves
 * only stable route_class_id values into named policy references.
 */
namespace Ores.Middleware.RouteClassPolicy;

@minLength(1)
@maxLength(80)
@pattern("^[a-z][a-z0-9]*(?:[-_.][a-z0-9]+)*$")
scalar RouteClassId extends string;

@minLength(1)
@maxLength(160)
@pattern("^[a-z][a-z0-9]*(?:[-_.:][a-z0-9]+)*$")
scalar PolicyId extends string;

@minLength(1)
@maxLength(128)
@pattern("^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,127}$")
scalar AuditId extends string;

@minLength(1)
@maxLength(512)
scalar AuditReason extends string;

enum SecurityOverrideIntent {
  preserveOrStrengthen: "preserve-or-strengthen",
  weaken: "weaken",
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteClassPolicyRefs {
  auth_policy_id?: PolicyId;
  authorization_policy_id?: PolicyId;
  rate_limit_policy_id?: PolicyId;
  cache_policy_id?: PolicyId;
  timeout_policy_id?: PolicyId;
  cors_policy_id?: PolicyId;
  csrf_policy_id?: PolicyId;
  validation_policy_id?: PolicyId;
  resilience_policy_id?: PolicyId;
  idempotency_policy_id?: PolicyId;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model SecurityWeakeningException {
  exception_id: AuditId;
  ticket_id: AuditId;
  reason: AuditReason;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteClassPolicy {
  id: RouteClassId;
  extends?: RouteClassId;
  overrides: RouteClassPolicyRefs;
  security_override_intent?: SecurityOverrideIntent;
  security_exception?: SecurityWeakeningException;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteClassPolicyTable {
  schema: "ores.middleware.route-class-policies/v1";
  default_class_id: RouteClassId;

  @minItems(1)
  @maxItems(64)
  classes: RouteClassPolicy[];
}
