const HTTP_HEADER_NAME_RE = /^[!#$%&'*+.^_`|~0-9a-z-]+$/;
const RUNTIME_OWNED_REQUEST_HEADERS = new Set([
    "authorization",
    "baggage",
    "connection",
    "content-encoding",
    "content-length",
    "content-type",
    "cookie",
    "forwarded",
    "host",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "set-cookie",
    "te",
    "traceparent",
    "tracestate",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "x-forwarded-for",
    "x-forwarded-host",
    "x-forwarded-proto",
    "x-real-ip"
]);
function freezePairs(pairs) {
    return Object.freeze(Array.from(pairs, ([key, value]) => Object.freeze([key, value])));
}
function freezePathParams(values) {
    return Object.freeze({ ...(values ?? {}) });
}
function normalizeHeaderNames(names) {
    if (names === undefined)
        return Object.freeze([]);
    if (!Array.isArray(names)) {
        throw new TypeError("request contract headerNames must be an array");
    }
    const seen = new Set();
    const normalized = [];
    for (const [index, name] of names.entries()) {
        if (typeof name !== "string" || !HTTP_HEADER_NAME_RE.test(name)) {
            throw new TypeError(`request contract headerNames[${index}] must be one canonical lower-case HTTP token`);
        }
        if (RUNTIME_OWNED_REQUEST_HEADERS.has(name)) {
            throw new TypeError(`request contract header ${name} is owned by authentication, tracing, proxy, or HTTP framing middleware`);
        }
        if (seen.has(name)) {
            throw new TypeError(`request contract header ${name} is declared more than once`);
        }
        seen.add(name);
        normalized.push(name);
    }
    return Object.freeze(normalized);
}
function projectHeaders(headers, names) {
    const projected = [];
    for (const name of names) {
        const value = headers.get(name);
        if (value !== null)
            projected.push([name, value]);
    }
    return freezePairs(projected);
}
function createBodyReaders(request) {
    const jsonRequest = request.clone();
    const textRequest = request.clone();
    let jsonResult;
    let textResult;
    return Object.freeze({
        present: request.body !== null,
        json: () => (jsonResult ??= jsonRequest.json()),
        text: () => (textResult ??= textRequest.text())
    });
}
function normalizeIssues(issues) {
    if (!Array.isArray(issues)) {
        throw new TypeError("request contract validator must return an issue array");
    }
    return Object.freeze(issues.map((issue, index) => {
        if (!issue ||
            typeof issue.path !== "string" ||
            typeof issue.code !== "string" ||
            typeof issue.message !== "string") {
            throw new TypeError(`request contract issue ${index} is malformed`);
        }
        return Object.freeze({
            path: issue.path,
            code: issue.code,
            message: issue.message
        });
    }));
}
export async function checkRequestContract(validator, request, url = new URL(request.url)) {
    if (!validator)
        return undefined;
    const method = request.method.toUpperCase();
    const pathname = url.pathname;
    const match = await validator.resolve(method, pathname);
    if (!match) {
        return Object.freeze({
            status: 404,
            code: "unknown_operation",
            issues: Object.freeze([
                Object.freeze({
                    path: "/",
                    code: "unknown_operation",
                    message: "no request contract matched the HTTP method and pathname"
                })
            ])
        });
    }
    if (!match.pathTemplate.startsWith("/")) {
        throw new TypeError("request contract pathTemplate must begin with '/'");
    }
    const headerNames = normalizeHeaderNames(match.headerNames);
    const input = Object.freeze({
        method,
        pathname,
        pathTemplate: match.pathTemplate,
        pathParams: freezePathParams(match.pathParams),
        query: freezePairs(url.searchParams.entries()),
        headers: projectHeaders(request.headers, headerNames),
        body: createBodyReaders(request)
    });
    const issues = normalizeIssues(await match.validate(input));
    if (issues.length === 0)
        return undefined;
    return Object.freeze({
        status: 400,
        code: "request_contract_validation_failed",
        issues
    });
}
//# sourceMappingURL=request-contract.js.map