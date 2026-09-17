export const ROUTE_CLASS_POLICY_SCHEMA = "ores.middleware.route-class-policies/v1" as const;
export const MAX_ROUTE_CLASS_POLICIES = 64;
export const MAX_ROUTE_CLASS_INHERITANCE_DEPTH = 8;

export interface RouteClassPolicyRefs {
  readonly auth_policy_id?: string;
  readonly authorization_policy_id?: string;
  readonly rate_limit_policy_id?: string;
  readonly cache_policy_id?: string;
  readonly timeout_policy_id?: string;
  readonly cors_policy_id?: string;
  readonly csrf_policy_id?: string;
  readonly validation_policy_id?: string;
  readonly resilience_policy_id?: string;
  readonly idempotency_policy_id?: string;
}

export type SecurityOverrideIntent = "preserve-or-strengthen" | "weaken";

export interface SecurityWeakeningException {
  readonly exception_id: string;
  readonly ticket_id: string;
  readonly reason: string;
}

export interface RouteClassPolicy {
  readonly id: string;
  readonly extends?: string;
  readonly overrides: RouteClassPolicyRefs;
  readonly security_override_intent?: SecurityOverrideIntent;
  readonly security_exception?: SecurityWeakeningException;
}

export interface RouteClassPolicyTable {
  readonly schema: string;
  readonly default_class_id: string;
  readonly classes: readonly RouteClassPolicy[];
}

export interface RouteClassPolicyViolation {
  readonly code: string;
  readonly path: string;
  readonly message: string;
}

export interface ResolvedRouteClassPolicy {
  readonly route_class_id: string;
  readonly lineage: readonly string[];
  readonly policies: RouteClassPolicyRefs;
}

export class RouteClassPolicyResolutionError extends Error {
  constructor(
    public readonly kind: "invalid-table" | "unknown-route-class",
    public readonly violations: readonly RouteClassPolicyViolation[] = [],
    public readonly route_class_id?: string
  ) {
    super(
      kind === "invalid-table"
        ? "route class policy table is invalid"
        : `unknown route class policy: ${route_class_id ?? "<missing>"}`
    );
    this.name = "RouteClassPolicyResolutionError";
  }
}

const POLICY_REF_FIELDS = [
  "auth_policy_id",
  "authorization_policy_id",
  "rate_limit_policy_id",
  "cache_policy_id",
  "timeout_policy_id",
  "cors_policy_id",
  "csrf_policy_id",
  "validation_policy_id",
  "resilience_policy_id",
  "idempotency_policy_id"
] as const;

const SECURITY_CRITICAL_FIELDS = new Set<keyof RouteClassPolicyRefs>([
  "auth_policy_id",
  "authorization_policy_id",
  "csrf_policy_id",
  "validation_policy_id",
  "idempotency_policy_id"
]);

export function validateRouteClassPolicyTable(
  table: RouteClassPolicyTable
): RouteClassPolicyViolation[] {
  const violations: RouteClassPolicyViolation[] = [];

  if (table.schema !== ROUTE_CLASS_POLICY_SCHEMA) {
    violations.push({
      code: "invalid-route-class-policy-schema",
      path: "schema",
      message: "route class policy schema identifier is not supported"
    });
  }

  if (table.classes.length === 0) {
    violations.push({
      code: "empty-route-class-policy-table",
      path: "classes",
      message: "at least one route class policy is required"
    });
  }
  if (table.classes.length > MAX_ROUTE_CLASS_POLICIES) {
    violations.push({
      code: "too-many-route-class-policies",
      path: "classes",
      message: "route class policy table exceeds its bounded class count"
    });
  }

  if (!validRouteClassId(table.default_class_id)) {
    violations.push({
      code: "invalid-default-route-class-id",
      path: "default_class_id",
      message: "default_class_id must be a bounded stable lowercase identifier"
    });
  }

  const ids = new Set<string>();
  table.classes.forEach((entry, index) => {
    const prefix = `classes[${index}]`;
    if (!validRouteClassId(entry.id)) {
      violations.push({
        code: "invalid-route-class-id",
        path: `${prefix}.id`,
        message: "route class ids must be bounded stable lowercase identifiers"
      });
    }
    if (ids.has(entry.id)) {
      violations.push({
        code: "duplicate-route-class-id",
        path: `${prefix}.id`,
        message: "route class ids must be unique within a policy table"
      });
    }
    ids.add(entry.id);

    if (entry.extends !== undefined && !validRouteClassId(entry.extends)) {
      violations.push({
        code: "invalid-parent-route-class-id",
        path: `${prefix}.extends`,
        message: "extends must name a bounded stable route class id"
      });
    }

    for (const field of POLICY_REF_FIELDS) {
      const value = entry.overrides[field];
      if (value !== undefined && !validPolicyId(value)) {
        violations.push({
          code: "invalid-policy-id",
          path: `${prefix}.overrides.${field}`,
          message: "policy references must be bounded stable lowercase identifiers"
        });
      }
    }

    const exception = entry.security_exception;
    if (exception !== undefined) {
      if (!validAuditId(exception.exception_id)) {
        violations.push({
          code: "invalid-security-exception-id",
          path: `${prefix}.security_exception.exception_id`,
          message: "security exception id must be a bounded stable audit identifier"
        });
      }
      if (!validAuditId(exception.ticket_id)) {
        violations.push({
          code: "invalid-security-ticket-id",
          path: `${prefix}.security_exception.ticket_id`,
          message: "security exception ticket id must be a bounded stable audit identifier"
        });
      }
      if (exception.reason.trim().length === 0 || [...exception.reason].length > 512) {
        violations.push({
          code: "invalid-security-exception-reason",
          path: `${prefix}.security_exception.reason`,
          message: "security exception reason must be non-empty and at most 512 characters"
        });
      }
    }

    const critical = hasSecurityCriticalOverride(entry.overrides);
    if (entry.extends !== undefined && critical && entry.security_override_intent === undefined) {
      violations.push({
        code: "security-override-intent-required",
        path: `${prefix}.security_override_intent`,
        message:
          "security-critical inherited overrides must declare preserve-or-strengthen or weaken intent"
      });
    }

    if (entry.security_override_intent === "weaken") {
      if (!critical) {
        violations.push({
          code: "security-intent-without-critical-override",
          path: `${prefix}.security_override_intent`,
          message: "weaken intent requires an actual security-critical override"
        });
      }
      if (exception === undefined) {
        violations.push({
          code: "security-weakening-exception-required",
          path: `${prefix}.security_exception`,
          message: "security weakening requires explicit reviewed audit metadata"
        });
      }
    } else if (entry.security_override_intent === "preserve-or-strengthen") {
      if (!critical) {
        violations.push({
          code: "security-intent-without-critical-override",
          path: `${prefix}.security_override_intent`,
          message: "security override intent requires an actual security-critical override"
        });
      }
      if (exception !== undefined) {
        violations.push({
          code: "unexpected-security-exception",
          path: `${prefix}.security_exception`,
          message: "security exceptions are reserved for explicit weakening"
        });
      }
    } else if (exception !== undefined) {
      violations.push({
        code: "security-exception-without-intent",
        path: `${prefix}.security_exception`,
        message: "security exception metadata requires explicit weaken intent"
      });
    }
  });

  const byId = new Map(table.classes.map((entry) => [entry.id, entry] as const));
  if (validRouteClassId(table.default_class_id) && !byId.has(table.default_class_id)) {
    violations.push({
      code: "default-route-class-missing",
      path: "default_class_id",
      message: "default_class_id must name a declared route class"
    });
  }

  table.classes.forEach((entry, index) => {
    if (
      entry.extends !== undefined
      && validRouteClassId(entry.extends)
      && !byId.has(entry.extends)
    ) {
      violations.push({
        code: "parent-route-class-missing",
        path: `classes[${index}].extends`,
        message: "extends must name a declared route class"
      });
    }
  });

  const reportedCycles = new Set<string>();
  table.classes.forEach((entry, index) => {
    if (!validRouteClassId(entry.id)) return;
    const seen = new Map<string, number>();
    const chain: string[] = [];
    let current: string | undefined = entry.id;
    let depth = 0;

    while (current !== undefined) {
      const start = seen.get(current);
      if (start !== undefined) {
        const cycle = [...new Set(chain.slice(start))].sort();
        const key = JSON.stringify(cycle);
        if (!reportedCycles.has(key)) {
          reportedCycles.add(key);
          violations.push({
            code: "route-class-inheritance-cycle",
            path: `classes[${index}].extends`,
            message: "route class inheritance must be acyclic"
          });
        }
        break;
      }

      const node = byId.get(current);
      if (node === undefined) break;
      seen.set(current, chain.length);
      chain.push(current);
      depth += 1;
      if (depth > MAX_ROUTE_CLASS_INHERITANCE_DEPTH) {
        violations.push({
          code: "route-class-inheritance-too-deep",
          path: `classes[${index}].extends`,
          message: "route class inheritance exceeds the bounded depth"
        });
        break;
      }
      current = node.extends;
    }
  });

  return violations;
}

export function resolveRouteClassPolicy(
  table: RouteClassPolicyTable,
  routeClassId?: string
): ResolvedRouteClassPolicy {
  const violations = validateRouteClassPolicyTable(table);
  if (violations.length > 0) {
    throw new RouteClassPolicyResolutionError("invalid-table", violations);
  }

  const selected = routeClassId ?? table.default_class_id;
  const byId = new Map(table.classes.map((entry) => [entry.id, entry] as const));
  if (!byId.has(selected)) {
    throw new RouteClassPolicyResolutionError(
      "unknown-route-class",
      [],
      selected
    );
  }

  const lineage: RouteClassPolicy[] = [];
  let current: string | undefined = selected;
  while (current !== undefined) {
    const node = byId.get(current);
    if (node === undefined) {
      throw new RouteClassPolicyResolutionError("unknown-route-class", [], current);
    }
    lineage.push(node);
    current = node.extends;
  }
  lineage.reverse();

  const policies: Record<string, string> = {};
  for (const node of lineage) {
    for (const field of POLICY_REF_FIELDS) {
      const value = node.overrides[field];
      if (value !== undefined) policies[field] = value;
    }
  }

  return {
    route_class_id: selected,
    lineage: lineage.map((node) => node.id),
    policies: policies as RouteClassPolicyRefs
  };
}

function hasSecurityCriticalOverride(overrides: RouteClassPolicyRefs): boolean {
  return [...SECURITY_CRITICAL_FIELDS].some((field) => overrides[field] !== undefined);
}

function validRouteClassId(value: string): boolean {
  return stableLowerId(value, 80, false);
}

function validPolicyId(value: string): boolean {
  return stableLowerId(value, 160, true);
}

function validAuditId(value: string): boolean {
  return (
    value.length >= 1
    && value.length <= 128
    && /^[A-Za-z0-9][A-Za-z0-9_.:/-]*$/.test(value)
  );
}

function stableLowerId(value: string, maxLength: number, allowColon: boolean): boolean {
  if (value.length < 1 || value.length > maxLength) return false;
  const separator = allowColon ? "[-_.:]" : "[-_.]";
  return new RegExp(`^[a-z][a-z0-9]*(?:${separator}[a-z0-9]+)*$`).test(value);
}
