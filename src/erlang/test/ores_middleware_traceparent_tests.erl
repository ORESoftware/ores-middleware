-module(ores_middleware_traceparent_tests).

-include_lib("eunit/include/eunit.hrl").

-define(ZERO_TRACE_ID, <<"00000000000000000000000000000000">>).
-define(VALID_TRACE_ID, <<"0123456789abcdef0123456789abcdef">>).
-define(VALID_PARENT_SPAN_ID, <<"0123456789abcdef">>).
-define(VALID_SERVER_SPAN_ID, <<"fedcba9876543210">>).

config() ->
    Config0 = ores_middleware:default_config(<<"traceparent-policy-test">>),
    Settings0 = maps:get(settings, Config0),
    Tls0 = maps:get(tls, Settings0),
    Rate0 = maps:get(rate_limit, Settings0),
    Idempotency0 = maps:get(idempotency, Settings0),
    Config0#{
        environment => test,
        settings => Settings0#{
            tls => Tls0#{require_https => false},
            rate_limit => Rate0#{enabled => false},
            idempotency => Idempotency0#{enabled => false}
        }
    }.

request(TraceId) ->
    #{
        method => <<"GET">>,
        path => <<"/trace">>,
        scheme => <<"http">>,
        headers => #{
            <<"accept">> => <<"application/json">>,
            <<"traceparent">> =>
                <<"00-", TraceId/binary, "-", ?VALID_PARENT_SPAN_ID/binary, "-01">>
        },
        body_size => 0,
        remote_ip => <<"127.0.0.1">>
    }.

middleware() ->
    {ok, Middleware} = ores_middleware:create_middleware(config(), #{}),
    Middleware.

inbound_parent_is_not_relabelled_as_response_span_test() ->
    Middleware = middleware(),
    Response = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, Response))).

only_valid_tracer_owned_response_traceparent_is_preserved_test() ->
    Middleware = middleware(),
    Valid = <<"00-", ?VALID_TRACE_ID/binary, "-", ?VALID_SERVER_SPAN_ID/binary, "-01">>,
    ValidResponse = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => string:uppercase(Valid)}, body => <<>>}
    end),
    ?assertEqual(Valid, maps:get(<<"traceparent">>, maps:get(headers, ValidResponse))),

    Invalid = <<"00-", ?VALID_TRACE_ID/binary, "-0000000000000000-01">>,
    InvalidResponse = Middleware(request(?VALID_TRACE_ID), fun(_Request) ->
        #{status => 204, headers => #{<<"traceparent">> => Invalid}, body => <<>>}
    end),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, maps:get(headers, InvalidResponse))).

all_zero_inbound_trace_id_is_replaced_test() ->
    Middleware = middleware(),
    Parent = self(),
    Response = Middleware(request(?ZERO_TRACE_ID), fun(_Request) ->
        Parent ! {observed_trace_id, ores_middleware:current_trace_id()},
        #{status => 204, headers => #{}, body => <<>>}
    end),
    ?assertEqual(204, maps:get(status, Response)),
    receive
        {observed_trace_id, TraceId} ->
            ?assertNotEqual(?ZERO_TRACE_ID, TraceId),
            ?assertEqual(32, byte_size(TraceId))
    after 1000 ->
        ?assert(false)
    end.
