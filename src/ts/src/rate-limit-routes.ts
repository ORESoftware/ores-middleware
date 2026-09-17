export interface RateLimitRouteSelector {
  methods?: readonly string[];
  path_template?: string;
  operation_id?: string;
}

export interface RouteRateLimitRule<Policy> {
  selector: RateLimitRouteSelector;
  policy: Policy;
}

export interface RouteRateLimitTable<Policy> {
  default_policy?: Policy;
  routes: readonly RouteRateLimitRule<Policy>[];
}

export interface RouteRateLimitRequest {
  method: string;
  path: string;
  route_template?: string;
  operation_id?: string;
}

export type RouteRateLimitPolicySource = "route" | "default";

export interface ResolvedRouteRateLimitPolicy<Policy> {
  policy: Policy;
  source: RouteRateLimitPolicySource;
}

export interface RouteRateLimitViolation {
  code: string;
  path: string;
  message: string;
}

export class RouteRateLimitResolutionError extends Error {
  constructor(
    public readonly request: RouteRateLimitRequest,
    public readonly matches: readonly number[]
  ) {
    super(`ambiguous route-specific rate-limit policy for ${request.method} ${request.path}`);
    this.name = "RouteRateLimitResolutionError";
  }
}

export function validateRouteRateLimitTable<Policy>(
  table: RouteRateLimitTable<Policy>
): RouteRateLimitViolation[] {
  const violations: RouteRateLimitViolation[] = [];
  const selectorKeys = new Set<string>();

  table.routes.forEach((rule, ruleIndex) => {
    const prefix = `routes[${ruleIndex}].selector`;
    const methods = rule.selector.methods ?? [];

    if (methods.length === 0 && rule.selector.path_template === undefined && rule.selector.operation_id === undefined) {
      violations.push({
        code: "empty-route-selector",
        path: prefix,
        message: "route selector must constrain method, path_template, or operation_id"
      });
    }

    const seenMethods = new Set<string>();
    methods.forEach((method, methodIndex) => {
      if (!/^[A-Z][A-Z-]*$/.test(method)) {
        violations.push({
          code: "invalid-http-method",
          path: `${prefix}.methods[${methodIndex}]`,
          message: "HTTP methods must be canonical uppercase ASCII tokens"
        });
      }
      if (seenMethods.has(method)) {
        violations.push({
          code: "duplicate-http-method",
          path: `${prefix}.methods[${methodIndex}]`,
          message: "HTTP method may appear only once in a selector"
        });
      }
      seenMethods.add(method);
    });

    if (rule.selector.path_template !== undefined) {
      const pathError = validatePathTemplate(rule.selector.path_template);
      if (pathError !== undefined) {
        violations.push({
          code: pathError,
          path: `${prefix}.path_template`,
          message: "path_template must be an absolute normalized route template"
        });
      }
    }

    if (rule.selector.operation_id !== undefined && !/^[A-Za-z0-9_.:/-]{1,160}$/.test(rule.selector.operation_id)) {
      violations.push({
        code: "invalid-operation-id",
        path: `${prefix}.operation_id`,
        message: "operation_id must be a bounded stable ASCII identifier"
      });
    }

    const selectorKey = canonicalSelectorKey(rule.selector);
    if (selectorKeys.has(selectorKey)) {
      violations.push({
        code: "duplicate-route-selector",
        path: prefix,
        message: "identical route selectors are ambiguous and must be combined"
      });
    }
    selectorKeys.add(selectorKey);
  });

  return violations;
}

export function resolveRouteRateLimitPolicy<Policy>(
  table: RouteRateLimitTable<Policy>,
  request: RouteRateLimitRequest
): ResolvedRouteRateLimitPolicy<Policy> | undefined {
  let bestScore: number | undefined;
  let matches: Array<{ index: number; rule: RouteRateLimitRule<Policy> }> = [];

  table.routes.forEach((rule, index) => {
    const score = matchScore(rule.selector, request);
    if (score === undefined) return;

    if (bestScore === undefined || score > bestScore) {
      bestScore = score;
      matches = [{ index, rule }];
      return;
    }
    if (score === bestScore) matches.push({ index, rule });
  });

  if (matches.length > 1) {
    throw new RouteRateLimitResolutionError(request, matches.map(({ index }) => index));
  }
  if (matches.length === 1) {
    return { policy: matches[0]!.rule.policy, source: "route" };
  }
  return table.default_policy === undefined
    ? undefined
    : { policy: table.default_policy, source: "default" };
}

function matchScore(selector: RateLimitRouteSelector, request: RouteRateLimitRequest): number | undefined {
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
    const registeredTemplateMatch = request.route_template === selector.path_template;
    if (!registeredTemplateMatch && !pathTemplateMatches(selector.path_template, request.path)) {
      return undefined;
    }
    score += 100 + staticSegmentCount(selector.path_template) * 10;
    if (registeredTemplateMatch) score += 5;
  }

  return score;
}

function canonicalSelectorKey(selector: RateLimitRouteSelector): string {
  return JSON.stringify({
    methods: [...(selector.methods ?? [])].sort(),
    path_template: selector.path_template ?? null,
    operation_id: selector.operation_id ?? null
  });
}

function validatePathTemplate(template: string): string | undefined {
  if (!template.startsWith("/") || template.includes("?") || template.includes("#")) return "invalid-path-template";
  if (template.length > 1 && template.endsWith("/")) return "non-canonical-path-template";

  const segments = pathSegments(template);
  for (let index = 0; index < segments.length; index += 1) {
    const segment = segments[index]!;
    if (segment.length === 0) return "empty-path-segment";
    if (segment === "*" && index !== segments.length - 1) return "catch-all-must-be-terminal";
    if (segment.startsWith("{") && (!segment.endsWith("}") || segment.length <= 2)) return "invalid-path-parameter";
    if (segment === ":") return "invalid-path-parameter";
  }
  return undefined;
}

function pathTemplateMatches(template: string, requestPath: string): boolean {
  const path = requestPath.split("?", 1)[0] ?? requestPath;
  const templateSegments = pathSegments(template);
  const requestSegments = pathSegments(path);
  let pathIndex = 0;

  for (let templateIndex = 0; templateIndex < templateSegments.length; templateIndex += 1) {
    const templateSegment = templateSegments[templateIndex]!;
    if (templateSegment === "*" && templateIndex === templateSegments.length - 1) return true;
    const requestSegment = requestSegments[pathIndex];
    if (requestSegment === undefined) return false;
    if (!isParameterSegment(templateSegment) && templateSegment !== requestSegment) return false;
    pathIndex += 1;
  }

  return pathIndex === requestSegments.length;
}

function pathSegments(path: string): string[] {
  return path === "/" ? [] : path.replace(/^\/+|\/+$/g, "").split("/");
}

function isParameterSegment(segment: string): boolean {
  return (segment.startsWith("{") && segment.endsWith("}") && segment.length > 2)
    || (segment.startsWith(":") && segment.length > 1);
}

function staticSegmentCount(template: string): number {
  return pathSegments(template).filter((segment) => !isParameterSegment(segment) && segment !== "*").length;
}
