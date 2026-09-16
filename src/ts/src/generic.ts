export interface Provider<Input, Output> {
  verify(input: Input): Promise<Output>;
}

export type ProviderFunction<Input, Output> =
  (input: Input) => Output | Promise<Output>;

/**
 * Wrap a consumer-owned function or SDK call in the portable provider port.
 * The concrete SDK type stays captured by the consumer closure; ores-middleware
 * does not import, version, or otherwise own it.
 */
export function providerFrom<Input, Output>(
  verify: ProviderFunction<Input, Output>
): Provider<Input, Output> {
  return Object.freeze({
    verify: async (input: Input): Promise<Output> => verify(input)
  });
}

export interface ContextualInput<Request, Context> {
  readonly request: Request;
  readonly context: Context;
}

export type ContextualProvider<Request, Context, Output> =
  Provider<ContextualInput<Request, Context>, Output>;

export function contextualProviderFrom<Request, Context, Output>(
  verify: (
    request: Request,
    context: Context
  ) => Output | Promise<Output>
): ContextualProvider<Request, Context, Output> {
  return providerFrom(({ request, context }) => verify(request, context));
}

export type Handler<Request, Response> =
  (request: Request) => Promise<Response>;

export type Middleware<Request, Response> =
  (next: Handler<Request, Response>) => Handler<Request, Response>;

/**
 * Compose arbitrary middleware in declaration order. The first middleware is
 * outermost and therefore runs first on the request path. No ORES-specific
 * stage names or ordering rules are imposed here.
 */
export function composeMiddleware<Request, Response>(
  handler: Handler<Request, Response>,
  ...middleware: readonly Middleware<Request, Response>[]
): Handler<Request, Response> {
  return middleware.reduceRight(
    (next, stage) => stage(next),
    handler
  );
}

export interface NamedMiddleware<Request, Response> {
  readonly name: string;
  readonly middleware: Middleware<Request, Response>;
}

/**
 * Preserve consumer-selected stage metadata without interpreting it. This is
 * useful for application-owned validation/configuration while keeping the
 * generic runtime independent of any particular auth, rate-limit, or router.
 */
export function composeNamedMiddleware<Request, Response>(
  handler: Handler<Request, Response>,
  stages: readonly NamedMiddleware<Request, Response>[]
): Handler<Request, Response> {
  return composeMiddleware(handler, ...stages.map((stage) => stage.middleware));
}
