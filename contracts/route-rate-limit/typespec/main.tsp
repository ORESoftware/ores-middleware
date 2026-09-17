import "@typespec/json-schema";

using TypeSpec.JsonSchema;

/**
 * Independently authored route-to-rate-limit-policy binding authority.
 * Numeric quotas, algorithms, storage and failure posture remain owned by
 * ores-rate-limit. This contract selects stable policy ids only.
 */
namespace Ores.Middleware.RouteRateLimit;

@minLength(1)
@maxLength(80)
@pattern("^[a-z][a-z0-9]*(?:[-_.][a-z0-9]+)*$")
scalar RouteClassId extends string;

@minLength(1)
@maxLength(160)
@pattern("^[a-z][a-z0-9]*(?:[-_.:][a-z0-9]+)*$")
scalar PolicyId extends string;

@minLength(1)
@maxLength(32)
@pattern("^[A-Z][A-Z0-9-]{0,31}$")
scalar HttpMethod extends string;

@minLength(1)
@maxLength(32)
@pattern("^[A-Za-z][A-Za-z0-9-]{0,31}$")
scalar RequestHttpMethod extends string;

@minLength(1)
@maxLength(4096)
@pattern("^/[^#\\r\\n\\u0000]{0,4095}$")
scalar RequestPath extends string;

@minLength(1)
@maxLength(512)
@pattern("^/[^?#\\r\\n]{0,511}$")
scalar PathTemplate extends string;

@minLength(1)
@maxLength(160)
@pattern("^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,159}$")
scalar OperationId extends string;

@TypeSpec.JsonSchema.extension("additionalProperties", false)
@TypeSpec.JsonSchema.extension("anyOf", #[
  #{ `type`: "object", `required`: #["path_template"] },
  #{ `type`: "object", `required`: #["operation_id"] },
  #{
    `type`: "object",
    `properties`: #{ `methods`: #{ `type`: "array", `minItems`: 1 } },
    `required`: #["methods"]
  }
])
model RouteRateLimitBindingSelector {
  @maxItems(8)
  @TypeSpec.JsonSchema.uniqueItems
  methods?: HttpMethod[];
  path_template?: PathTemplate;
  operation_id?: OperationId;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteRateLimitBindingRequest {
  method: RequestHttpMethod;
  path: RequestPath;
  route_template?: PathTemplate;
  operation_id?: OperationId;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteRateLimitBinding {
  route_class_id: RouteClassId;
  policy_id: PolicyId;
  selector: RouteRateLimitBindingSelector;
}

@jsonSchema
@TypeSpec.JsonSchema.extension("additionalProperties", false)
model RouteRateLimitBindingTable {
  schema: "ores.middleware.route-rate-limit-bindings/v1";
  default_policy_id?: PolicyId;

  @maxItems(128)
  routes: RouteRateLimitBinding[];
}
