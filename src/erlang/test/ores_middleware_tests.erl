-module(ores_middleware_tests).
-include_lib("eunit/include/eunit.hrl").

production_rejects_test_only_middleware_test() ->
    Config0 = ores_middleware:default_config(<<"test">>),
    Settings0 = maps:get(settings, Config0),
    Fault0 = maps:get(fault_injection, Settings0),
    Bypass0 = maps:get(test_auth_bypass, Settings0),
    Settings = Settings0#{fault_injection => Fault0#{enabled => true}, test_auth_bypass => Bypass0#{enabled => true}},
    Issues = ores_middleware:validate_config(Config0#{environment => production, settings => Settings}),
    ?assert(length(Issues) >= 2).

context_is_process_scoped_test() ->
    Previous = ores_middleware:current_context(),
    Context = #{request_id => <<"r1">>, trace_id => <<"0123456789abcdef0123456789abcdef">>, tenant_id => undefined, user_id => undefined},
    ?assertEqual(<<"r1">>, ores_middleware:run_with_context(Context, fun() -> maps:get(request_id, ores_middleware:current_context()) end)),
    ?assertEqual(Previous, ores_middleware:current_context()).

middleware_adds_correlation_and_security_headers_test() ->
    Previous = ores_middleware:current_context(),
    Config0 = ores_middleware:default_config(<<"test">>),
    Settings0 = maps:get(settings, Config0),
    Tls0 = maps:get(tls, Settings0),
    Rate0 = maps:get(rate_limit, Settings0),
    Config = Config0#{settings => Settings0#{tls => Tls0#{require_https => false}, rate_limit => Rate0#{enabled => false}}},
    {ok, Middleware} = ores_middleware:create_middleware(Config, #{}),
    Request = #{method => <<"GET">>, path => <<"/v1">>, scheme => <<"http">>, headers => #{<<"accept">> => <<"application/json">>}, body_size => 0, remote_ip => <<"127.0.0.1">>},
    Response = Middleware(Request, fun(_Request) -> #{status => 200, headers => #{<<"content-type">> => <<"application/json">>}, body => <<"{\"ok\":true}">>} end),
    Headers = maps:get(headers, Response),
    ?assertEqual(200, maps:get(status, Response)),
    ?assert(maps:is_key(<<"x-request-id">>, Headers)),
    ?assertEqual(false, maps:is_key(<<"traceparent">>, Headers)),
    ?assertEqual(<<"nosniff">>, maps:get(<<"x-content-type-options">>, Headers)),
    ?assertEqual(Previous, ores_middleware:current_context()).

descriptor_has_standard_surface_test() ->
    Descriptor = ores_middleware:descriptor(),
    ?assertEqual(23, length(maps:get(<<"capabilities">>, Descriptor))),
    ?assertEqual(7, map_size(maps:get(<<"operationSymbols">>, Descriptor))).


local_rate_limiter_bounds_source_ip_cardinality_test() ->
    lists:foreach(
        fun(Index) ->
            Key = iolist_to_binary(io_lib:format("bounded-ip-~B", [Index])),
            ?assertEqual(true, ores_middleware_rate_limiter:allow(Key, 1, 0.000001))
        end,
        lists:seq(0, 10000)
    ),
    State = sys:get_state(ores_middleware_rate_limiter),
    Buckets = maps:get(buckets, State),
    Order = maps:get(order, State),
    ?assert(map_size(Buckets) =< 10000),
    ?assert(queue:len(Order) =< 10000).
