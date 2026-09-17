export type MaybePromise<Value> = Value | Promise<Value>;

export interface Provider<Input, Output> {
  verify(input: Input): Promise<Output>;
}

export type ProviderFunction<Input, Output> =
  (input: Input) => MaybePromise<Output>;

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

/**
 * Typed result for provider boundaries that should not use exceptions as their
 * public failure channel. Failure remains consumer-defined and ORES-agnostic.
 */
export type ProviderResult<Output, Failure> =
  | Readonly<{ ok: true; value: Output }>
  | Readonly<{ ok: false; error: Failure }>;

export interface FallibleProvider<Input, Output, Failure> {
  verify(input: Input): Promise<ProviderResult<Output, Failure>>;
}

export type FallibleProviderFunction<Input, Output, Failure> =
  (input: Input) => MaybePromise<ProviderResult<Output, Failure>>;

export function providerOk<Output>(value: Output): ProviderResult<Output, never> {
  return Object.freeze({ ok: true as const, value });
}

export function providerError<Failure>(error: Failure): ProviderResult<never, Failure> {
  return Object.freeze({ ok: false as const, error });
}

export function fallibleProviderFrom<Input, Output, Failure>(
  verify: FallibleProviderFunction<Input, Output, Failure>
): FallibleProvider<Input, Output, Failure> {
  return Object.freeze({
    verify: async (input: Input): Promise<ProviderResult<Output, Failure>> => verify(input)
  });
}

export interface ContextualInput<Request, Context> {
  readonly request: Request;
  readonly context: Context;
}

export type ContextualProvider<Request, Context, Output> =
  Provider<ContextualInput<Request, Context>, Output>;

export type ContextualFallibleProvider<Request, Context, Output, Failure> =
  FallibleProvider<ContextualInput<Request, Context>, Output, Failure>;

export function contextualProviderFrom<Request, Context, Output>(
  verify: (
    request: Request,
    context: Context
  ) => MaybePromise<Output>
): ContextualProvider<Request, Context, Output> {
  return providerFrom(({ request, context }) => verify(request, context));
}

export function contextualFallibleProviderFrom<Request, Context, Output, Failure>(
  verify: (
    request: Request,
    context: Context
  ) => MaybePromise<ProviderResult<Output, Failure>>
): ContextualFallibleProvider<Request, Context, Output, Failure> {
  return fallibleProviderFrom(({ request, context }) => verify(request, context));
}

export type Handler<Request, Response> =
  (request: Request) => MaybePromise<Response>;

export type Middleware<Request, Response> =
  (next: Handler<Request, Response>) => Handler<Request, Response>;

export type ContextualHandler<Request, Context, Response> =
  (request: Request, context: Context) => MaybePromise<Response>;

export type ContextualMiddleware<Request, Context, Response> =
  (next: ContextualHandler<Request, Context, Response>) =>
    ContextualHandler<Request, Context, Response>;

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

/**
 * Context-aware counterpart to composeMiddleware. Context shape, ownership,
 * mutation policy, and lifecycle are deliberately left to the consumer.
 */
export function composeContextualMiddleware<Request, Context, Response>(
  handler: ContextualHandler<Request, Context, Response>,
  ...middleware: readonly ContextualMiddleware<Request, Context, Response>[]
): ContextualHandler<Request, Context, Response> {
  return middleware.reduceRight(
    (next, stage) => stage(next),
    handler
  );
}

export interface NamedMiddleware<Request, Response> {
  readonly name: string;
  readonly middleware: Middleware<Request, Response>;
}

export interface NamedContextualMiddleware<Request, Context, Response> {
  readonly name: string;
  readonly middleware: ContextualMiddleware<Request, Context, Response>;
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

export function composeNamedContextualMiddleware<Request, Context, Response>(
  handler: ContextualHandler<Request, Context, Response>,
  stages: readonly NamedContextualMiddleware<Request, Context, Response>[]
): ContextualHandler<Request, Context, Response> {
  return composeContextualMiddleware(
    handler,
    ...stages.map((stage) => stage.middleware)
  );
}
