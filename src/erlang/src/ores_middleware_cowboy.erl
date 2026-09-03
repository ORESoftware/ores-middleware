-module(ores_middleware_cowboy).
-behaviour(cowboy_middleware).

-export([execute/2]).

execute(Req0, Env0) ->
    Config = maps:get(
        ores_middleware_config,
        Env0,
        ores_middleware:default_config(<<"cowboy-service">>)
    ),
    Hooks = maps:merge(
        ores_middleware:default_hooks(),
        maps:get(ores_middleware_hooks, Env0, #{})
    ),
    Headers = cowboy_req:headers(Req0),
    BodySize = case maps:get(<<"content-length">>, Headers, undefined) of
        undefined -> 0;
        Value -> try binary_to_integer(Value) catch _:_ -> 0 end
    end,
    {PeerAddress, _PeerPort} = cowboy_req:peer(Req0),
    Request = #{
        method => cowboy_req:method(Req0),
        path => cowboy_req:path(Req0),
        scheme => cowboy_req:scheme(Req0),
        headers => Headers,
        body_size => BodySize,
        remote_ip => iolist_to_binary(inet:ntoa(PeerAddress))
    },
    case ores_middleware:prepare(Config, Hooks, Request) of
        {error, Response} -> reply(Response, Req0);
        {cached, Response} -> reply(Response, Req0);
        {ok, Context, IdempotencyKey} ->
            Security = maps:get(security_headers, maps:get(settings, Config)),
            ResponseHeaders0 = #{
                maps:get(request_id_header, maps:get(settings, Config)) =>
                    maps:get(request_id, Context)
            },
            ResponseHeaders = case maps:get(enabled, Security) of
                true -> ResponseHeaders0#{
                    <<"x-content-type-options">> => <<"nosniff">>,
                    <<"x-frame-options">> => maps:get(frame_options, Security),
                    <<"referrer-policy">> => <<"strict-origin-when-cross-origin">>
                };
                false -> ResponseHeaders0
            end,
            Req1 = cowboy_req:set_resp_headers(ResponseHeaders, Req0),
            Req2 = cowboy_req:set_meta(ores_middleware_context, Context, Req1),
            Env1 = Env0#{
                ores_middleware_context => Context,
                ores_middleware_request => Request,
                ores_middleware_idempotency_key => IdempotencyKey
            },
            {Req3, Env} = maybe_attach_logger(Req2, Env1, Context),
            {ok, Req3, Env}
    end.

%% Set `ores_otel_logger` in Cowboy middleware environment to expose the pinned
%% logger through both request metadata and handler environment. The pinned
%% logger remains safe even when used outside an ambient process context.
maybe_attach_logger(Req, Env, Context) ->
    case maps:get(ores_otel_logger, Env, undefined) of
        Logger when is_map(Logger) ->
            RequestLogger = ores_middleware_otel:request_logger(Logger, Context),
            Req1 = cowboy_req:set_meta(log, RequestLogger, Req),
            Req2 = cowboy_req:set_meta(ores_log, RequestLogger, Req1),
            {Req2, Env#{
                log => RequestLogger,
                ores_log => RequestLogger,
                ores_otel_context => ores_middleware_otel:to_log_context(Context)
            }};
        _ -> {Req, Env}
    end.

reply(#{status := Status, headers := Headers, body := Body}, Req) ->
    Req1 = cowboy_req:reply(Status, Headers, Body, Req),
    {stop, Req1}.
