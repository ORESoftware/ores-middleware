-module(ores_middleware_cowboy).
-behaviour(cowboy_middleware).

-export([execute/2]).

execute(Req0, Env0) ->
    Config = maps:get(ores_middleware_config, Env0, ores_middleware:default_config(<<"cowboy-service">>)),
    Hooks = maps:merge(ores_middleware:default_hooks(), maps:get(ores_middleware_hooks, Env0, #{})),
    Headers = cowboy_req:headers(Req0),
    BodySize = case maps:get(<<"content-length">>, Headers, undefined) of undefined -> 0; Value -> try binary_to_integer(Value) catch _:_ -> 0 end end,
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
                maps:get(request_id_header, maps:get(settings, Config)) => maps:get(request_id, Context),
                <<"traceparent">> => <<"00-", (maps:get(trace_id, Context))/binary, "-0000000000000000-01">>
            },
            ResponseHeaders = case maps:get(enabled, Security) of
                true -> ResponseHeaders0#{<<"x-content-type-options">> => <<"nosniff">>, <<"x-frame-options">> => maps:get(frame_options, Security), <<"referrer-policy">> => <<"strict-origin-when-cross-origin">>};
                false -> ResponseHeaders0
            end,
            Req1 = cowboy_req:set_resp_headers(ResponseHeaders, Req0),
            Req2 = cowboy_req:set_meta(ores_middleware_context, Context, Req1),
            Env = Env0#{ores_middleware_context => Context, ores_middleware_request => Request, ores_middleware_idempotency_key => IdempotencyKey},
            {ok, Req2, Env}
    end.

reply(#{status := Status, headers := Headers, body := Body}, Req) ->
    Req1 = cowboy_req:reply(Status, Headers, Body, Req),
    {stop, Req1}.
