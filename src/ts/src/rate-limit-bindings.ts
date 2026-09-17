export const ROUTE_RATE_LIMIT_BINDING_SCHEMA = "ores.middleware.route-rate-limit-bindings/v1" as const;
export const MAX_ROUTE_RATE_LIMIT_BINDINGS = 128;
export const MAX_ROUTE_METHODS = 8;
export const MAX_ROUTE_RATE_LIMIT_REQUEST_PATH_LENGTH = 4096;

export interface RouteRateLimitBindingSelector {
  readonly methods?: readonly string[];
  readonly path_template?: string;
  readonly operation_id?: string;
}

export interface RouteRateLimitBinding {
  readonly route_class_id: string;
  readonly policy_id: string;
  readonly selector: RouteRateLimitBindingSelector;
}

export interface RouteRateLimitBindingTable {
  readonly schema: string;
  readonly default_policy_id?: string;
  readonly routes: readonly RouteRateLimitBinding[];
}

export interface RouteRateLimitBindingRequest {
  readonly method: string;
  readonly path: string;
  readonly route_template?: string;
  readonly operation_id?: string;
}

export type RouteRateLimitBindingSource = "route" | "default";

export interface ResolvedRouteRateLimitBinding {
  readonly route_class_id?: string;
  readonly policy_id: string;
  readonly source: RouteRateLimitBindingSource;
}

export interface RouteRateLimitBindingViolation {
  readonly code: string;
  readonly path: string;
  readonly message: string;
}

export class RouteRateLimitBindingResolutionError extends Error {
  constructor(
    public readonly request: RouteRateLimitBindingRequest,
    public readonly route_class_ids: readonly string[],
    public readonly policy_ids: readonly string[]
  ) {
    super(`ambiguous route rate-limit binding for ${request.method} ${request.path}`);
    this.name = "RouteRateLimitBindingResolutionError";
  }
}

export class RouteRateLimitBindingValidationError extends Error {
  constructor(
    public readonly scope: "table" | "request",
    public readonly violations: readonly RouteRateLimitBindingViolation[]
  ) {
    super(`invalid route rate-limit binding ${scope}: ${violations.map((issue) => `${issue.path}:${issue.code}`).join(", ")}`);
    this.name = "RouteRateLimitBindingValidationError";
  }
}

export function validateRouteRateLimitBindingTable(
  table: RouteRateLimitBindingTable
): RouteRateLimitBindingViolation[] {
  const violations: RouteRateLimitBindingViolation[] = [];

  if (table.schema !== ROUTE_RATE_LIMIT_BINDING_SCHEMA) {
    violations.push({
      code: "invalid-binding-schema",
      path: "schema",
      message: "route binding schema identifier is not supported"
    });
  }

  if (table.routes.length > MAX_ROUTE_RATE_LIMIT_BINDINGS) {
    violations.push({
      code: "too-many-route-bindings",
      path: "routes",
      message: "route binding table exceeds its bounded route count"
    });
  }

  if (table.default_policy_id !== undefined && !validStableLowerId(table.default_policy_id, 160, true)) {
    violations.push({
      code: "invalid-policy-id",
      path: "default_policy_id",
      message: "default_policy_id must be a bounded stable policy identifier"
    });
  }

  const routeClasses = new Set<string>();
  const selectors = new Set<string>();
  table.routes.forEach((route, routeIndex) => {
    const prefix = `routes[${routeIndex}]`;

    if (!validStableLowerId(route.route_class_id, 80, false)) {
      violations.push({
        code: "invalid-route-class-id",
        path: `${prefix}.route_class_id`,
        message: "route_class_id must be a bounded stable lowercase identifier"
      });
    }
    if (routeClasses.has(route.route_class_id)) {
      violations.push({
        code: "duplicate-route-class-id",
        path: `${prefix}.route_class_id`,
        message: "route_class_id must be unique within a binding table"
      });
    }
    routeClasses.add(route.route_class_id);

    if (!validStableLowerId(route.policy_id, 160, true)) {
      violations.push({
        code: "invalid-policy-id",
        path: `${prefix}.policy_id`,
        message: "policy_id must be a bounded stable policy identifier"
      });
    }

    for (const issue of validateSelector(route.selector)) {
      violations.push({ ...issue, path: `${prefix}.${issue.path}` });
    }

    const selectorKey = canonicalSelectorKey(route.selector);
    if (selectors.has(selectorKey)) {
      violations.push({
        code: "duplicate-route-selector",
        path: `${prefix}.selector`,
        message: "identical route selectors are ambiguous and must be combined"
      });
    }
    selectors.add(selectorKey);
  });

  return violations;
}

export function validateRouteRateLimitBindingRequest(
  request: RouteRateLimitBindingRequest
): RouteRateLimitBindingViolation[] {
  const violations: RouteRateLimitBindingViolation[] = [];

  if (!/^[A-Za-z][A-Za-z0-9-]{0,31}$/.test(request.method)) {
    violations.push({
      code: "invalid-request-method",
      path: "method",
      message: "request method must be a bounded ASCII HTTP token"
    });
  }

  const pathLength = [...request.path].length;
  const basicPathValid =
    pathLength >= 1
    && pathLength <= MAX_ROUTE_RATE_LIMIT_REQUEST_PATH_LENGTH
    && request.path.startsWith("/")
    && !/[#\r\n\0]/.test(request.path);

  if (!basicPathValid) {
    violations.push({
      code: "invalid-request-path",
      path: "path",
      message: "request path must be bounded, absolute, and free of fragments/control separators"
    });
  } else {
    const requestPathWithoutQuery = request.path.split("?", 1)[0] ?? request.path;
    if (requestPathWithoutQuery.includes("//")) {
      violations.push({
        code: "repeated-request-path-separator",
        path: "path",
        message: "request path must not contain repeated slash separators"
      });
    }
    if (pathSegments(requestPathWithoutQuery).some((segment) => segment === "." || segment === "..")) {
      violations.push({
        code: "dot-request-path-segment",
        path: "path",
        message: "request path must not contain literal dot segments"
      });
    }
  }

  if (request.route_template !== undefined) {
    if (validatePathTemplate(request.route_template) !== undefined) {
      violations.push({
        code: "invalid-request-route-template",
        path: "route_template",
        message: "request route_template must use the canonical configured-template grammar"
      });
    } else if (basicPathValid && !pathTemplateMatches(request.route_template, request.path)) {
      violations.push({
        code: "route-template-path-mismatch",
        path: "route_template",
        message: "trusted route_template must describe the concrete request path"
      });
    }
  }

  if (
    request.operation_id !== undefined
    && !/^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,159}$/.test(request.operation_id)
  ) {
    violations.push({
      code: "invalid-request-operation-id",
      path: "operation_id",
      message: "request operation_id must be a bounded stable ASCII identifier"
    });
  }

  return violations;
}

export function resolveRouteRateLimitBinding(
  table: RouteRateLimitBindingTable,
  request: RouteRateLimitBindingRequest
): ResolvedRouteRateLimitBinding | undefined {
  const tableViolations = validateRouteRateLimitBindingTable(table);
  if (tableViolations.length > 0) {
    throw new RouteRateLimitBindingValidationError("table", tableViolations);
  }
  const requestViolations = validateRouteRateLimitBindingRequest(request);
  if (requestViolations.length > 0) {
    throw new RouteRateLimitBindingValidationError("request", requestViolations);
  }

  let bestScore: number | undefined;
  let matches: RouteRateLimitBinding[] = [];

  for (const route of table.routes) {
    const score = matchScore(route.selector, request);
    if (score === undefined) continue;
    if (bestScore === undefined || score > bestScore) {
      bestScore = score;
      matches = [route];
    } else if (score === bestScore) {
      matches.push(route);
    }
  }

  if (matches.length > 1) {
    const routeClassIds = [...new Set(matches.map((route) => route.route_class_id))].sort();
    const policyIds = [...new Set(matches.map((route) => route.policy_id))].sort();
    throw new RouteRateLimitBindingResolutionError(request, routeClassIds, policyIds);
  }

  const route = matches[0];
  if (route !== undefined) {
    return {
      route_class_id: route.route_class_id,
      policy_id: route.policy_id,
      source: "route"
    };
  }

  return table.default_policy_id === undefined
    ? undefined
    : { policy_id: table.default_policy_id, source: "default" };
}

function validateSelector(selector: RouteRateLimitBindingSelector): RouteRateLimitBindingViolation[] {
  const violations: RouteRateLimitBindingViolation[] = [];
  const methods = selector.methods ?? [];

  if (methods.length === 0 && selector.path_template === undefined && selector.operation_id === undefined) {
    violations.push({
      code: "empty-route-selector",
      path: "selector",
      message: "route selector must constrain method, path_template, or operation_id"
    });
  }

  if (methods.length > MAX_ROUTE_METHODS) {
    violations.push({
      code: "too-many-http-methods",
      path: "selector.methods",
      message: "route selector exceeds the bounded HTTP method count"
    });
  }

  const seenMethods = new Set<string>();
  methods.forEach((method, methodIndex) => {
    if (!/^[A-Z][A-Z0-9-]{0,31}$/.test(method)) {
      violations.push({
        code: "invalid-http-method",
        path: `selector.methods[${methodIndex}]`,
        message: "HTTP methods must be bounded canonical uppercase ASCII tokens"
      });
    }
    if (seenMethods.has(method)) {
      violations.push({
        code: "duplicate-http-method",
        path: `selector.methods[${methodIndex}]`,
        message: "HTTP method may appear only once in a selector"
      });
    }
    seenMethods.add(method);
  });

  if (selector.path_template !== undefined) {
    const code = validatePathTemplate(selector.path_template);
    if (code !== undefined) {
      violations.push({
        code,
        path: "selector.path_template",
        message: "path_template must be a bounded normalized absolute route template"
      });
    }
  }

  if (
    selector.operation_id !== undefined
    && !/^[A-Za-z0-9][A-Za-z0-9_.:/-]{0,159}$/.test(selector.operation_id)
  ) {
    violations.push({
      code: "invalid-operation-id",
      path: "selector.operation_id",
      message: "operation_id must be a bounded stable ASCII identifier"
    });
  }

  return violations;
}

function validStableLowerId(value: string, maxLength: number, allowColon: boolean): boolean {
  if (value.length < 1 || value.length > maxLength) return false;
  const separator = allowColon ? "[-_.:]" : "[-_.]";
  return new RegExp(`^[a-z][a-z0-9]*(?:${separator}[a-z0-9]+)*$`).test(value);
}

function matchScore(
  selector: RouteRateLimitBindingSelector,
  request: RouteRateLimitBindingRequest
): number | undefined {
  const methods = selector.methods ?? [];
  if (methods.length > 0 && !methods.some((method) => method.toUpperCase() === request.method.toUpperCase())) {
    return undefined;
  }
  if (selector.operation_id !== undefined && selector.operation_id !== request.operation_id) {
    return undefined;
  }

  let score = methods.length > 0 ? 1 : 0;
  if (selector.operation_id !== undefined) score += 10_000;
  if (selector.path_template !== undefined) {
    const concretePathMatch = pathTemplateMatches(selector.path_template, request.path);
    if (!concretePathMatch) return undefined;
    const registeredTemplateMatch = request.route_template === selector.path_template;
    score += 100 + staticSegmentCount(selector.path_template) * 10;
    if (registeredTemplateMatch) score += 5;
  }
  return score;
}

function canonicalSelectorKey(selector: RouteRateLimitBindingSelector): string {
  return JSON.stringify({
    methods: [...(selector.methods ?? [])].sort(),
    path_template: selector.path_template ?? null,
    operation_id: selector.operation_id ?? null
  });
}

function validatePathTemplate(template: string): string | undefined {
  if (template.length < 1 || template.length > 512 || !template.startsWith("/")) return "invalid-path-template";
  if (template.includes("?") || template.includes("#")) return "invalid-path-template";
  if (template.length > 1 && template.endsWith("/")) return "non-canonical-path-template";

  const segments = pathSegments(template);
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index]!;
    if (segment.length === 0) return "empty-path-segment";
    if (segment === "." || segment === "..") return "dot-path-segment";
    if (segment === "*") {
      if (index !== segments.length - 1) return "catch-all-must-be-terminal";
      continue;
    }
    if (segment.startsWith("{")) {
      if (!segment.endsWith("}")) return "invalid-path-parameter";
      if (!validParameterName(segment.slice(1, -1))) return "invalid-path-parameter";
      continue;
    }
    if (segment.startsWith(":")) {
      if (!validParameterName(segment.slice(1))) return "invalid-path-parameter";
      continue;
    }
    if (/[{}*]/.test(segment)) return "invalid-path-segment";
  }
  return undefined;
}

function validParameterName(value: string): boolean {
  return /^[A-Za-z_][A-Za-z0-9_]{0,63}$/.test(value);
}

function pathTemplateMatches(template: string, requestPath: string): boolean {
  const requestPathWithoutQuery = requestPath.split("?", 1)[0] ?? requestPath;
  const templateSegments = pathSegments(template);
  const requestSegments = pathSegments(requestPathWithoutQuery);
  let pathIndex = 0;

  for (let templateIndex = 0; templateIndex < templateSegments.length; templateIndex += 1) {
    const templateSegment = templateSegments[templateIndex]!;
    if (templateSegment === "*" && templateIndex === templateSegments.length - 1) return true;
    const requestSegment = requestSegments[pathIndex];
    if (requestSegment === undefined) return false;
    if (isParameterSegment(templateSegment)) {
      if (requestSegment.length === 0) return false;
    } else if (templateSegment !== requestSegment) {
      return false;
    }
    pathIndex += 1;
  }

  return pathIndex === requestSegments.length;
}

function pathSegments(path: string): string[] {
  return path === "/" ? [] : (path.startsWith("/") ? path.slice(1) : path).split("/");
}

function isParameterSegment(segment: string): boolean {
  return (segment.startsWith("{") && segment.endsWith("}") && segment.length > 2)
    || (segment.startsWith(":") && segment.length > 1);
}

function staticSegmentCount(template: string): number {
  return pathSegments(template).filter((segment) => !isParameterSegment(segment) && segment !== "*").length;
}
