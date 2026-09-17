-module(ores_middleware_generic).

-export([
    provider_from/1,
    contextual_provider_from/1,
    compose/2,
    compose_named/2,
    compose_contextual/2,
    compose_named_contextual/2
]).

-spec provider_from(fun((term()) -> term())) -> fun((term()) -> term()).
provider_from(Provider) when is_function(Provider, 1) ->
    Provider.

-spec contextual_provider_from(fun((term(), term()) -> term())) -> fun((map()) -> term()).
contextual_provider_from(Provider) when is_function(Provider, 2) ->
    fun(#{request := Request, context := Context}) -> Provider(Request, Context) end.

-spec compose(fun((term()) -> term()), [fun((fun((term()) -> term())) -> fun((term()) -> term()))]) ->
    fun((term()) -> term()).
compose(Handler, Middleware) when is_function(Handler, 1), is_list(Middleware) ->
    lists:foldr(
        fun(Stage, Next) when is_function(Stage, 1) -> Stage(Next) end,
        Handler,
        Middleware
    ).

-spec compose_named(fun((term()) -> term()), [map()]) -> fun((term()) -> term()).
compose_named(Handler, Stages) when is_list(Stages) ->
    Middleware = [
        case Stage of
            #{name := _, middleware := MiddlewareStage} when is_function(MiddlewareStage, 1) ->
                MiddlewareStage;
            _ ->
                error({invalid_named_middleware, Stage})
        end
     || Stage <- Stages
    ],
    compose(Handler, Middleware).

-spec compose_contextual(
    fun((term(), term()) -> term()),
    [fun((fun((term(), term()) -> term())) -> fun((term(), term()) -> term()))]
) -> fun((term(), term()) -> term()).
compose_contextual(Handler, Middleware) when is_function(Handler, 2), is_list(Middleware) ->
    lists:foldr(
        fun(Stage, Next) when is_function(Stage, 1) -> Stage(Next) end,
        Handler,
        Middleware
    ).

-spec compose_named_contextual(fun((term(), term()) -> term()), [map()]) ->
    fun((term(), term()) -> term()).
compose_named_contextual(Handler, Stages) when is_list(Stages) ->
    Middleware = [
        case Stage of
            #{name := _, middleware := MiddlewareStage} when is_function(MiddlewareStage, 1) ->
                MiddlewareStage;
            _ ->
                error({invalid_named_contextual_middleware, Stage})
        end
     || Stage <- Stages
    ],
    compose_contextual(Handler, Middleware).
