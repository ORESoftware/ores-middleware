-module(ores_middleware_operation_tests).

-include_lib("eunit/include/eunit.hrl").

reporter_failure_is_ignored_and_failure_is_redacted_test() ->
    Outer = context(1),
    Inner = context(2),
    ok = ores_middleware_context:put(Outer),
    Reporter = fun(_Failure) -> erlang:error(reporter_unavailable) end,
    Operation = fun() -> erlang:error(<<"private body token">>) end,
    {error, Failure} = ores_middleware_operation:run(
        Inner,
        #{transport => http, scope => request, name => <<"orders.read">>},
        Reporter,
        Operation
    ),
    ?assertEqual(error, maps:get(kind, Failure)),
    ?assertEqual(<<"operation_failed">>, maps:get(code, Failure)),
    ?assertEqual(<<"orders.read">>, maps:get(operation, Failure)),
    ?assertEqual(maps:get(request_id, Inner), maps:get(request_id, Failure)),
    ?assertNot(maps:is_key(message, Failure)),
    ?assertNot(maps:is_key(stack, Failure)),
    ?assertNot(maps:is_key(cause, Failure)),
    ?assertEqual(nomatch, binary:match(term_to_binary(Failure), <<"private body token">>)),
    ?assertEqual(Outer, ores_middleware_context:current()),
    ok = ores_middleware_context:clear().

expired_deadline_prevents_invocation_test() ->
    Parent = self(),
    Context = context(3),
    Operation = fun() -> Parent ! operation_invoked, accepted end,
    {error, Failure} = ores_middleware_operation:run(
        Context,
        #{
            transport => tcp,
            scope => callback,
            name => <<"queue.consume">>,
            deadline_unix_ms => erlang:system_time(millisecond) - 1
        },
        fun(_Event) -> ok end,
        Operation
    ),
    ?assertEqual(deadline_exceeded, maps:get(kind, Failure)),
    ?assertEqual(<<"operation_deadline_exceeded">>, maps:get(code, Failure)),
    receive
        operation_invoked -> ?assert(false)
    after 0 -> ok
    end,
    ?assertEqual(undefined, ores_middleware_context:current()).

malformed_operation_name_is_bounded_test() ->
    {error, Failure} = ores_middleware_operation:run(
        context(4),
        #{transport => websocket, scope => message, name => <<"customer/secret">>},
        fun(_Event) -> ok end,
        fun() -> throw(private_reason) end
    ),
    ?assertEqual(panic, maps:get(kind, Failure)),
    ?assertEqual(<<"operation_panicked">>, maps:get(code, Failure)),
    ?assertEqual(<<"operation">>, maps:get(operation, Failure)),
    ?assertEqual(<<"throw">>, maps:get(error_type, Failure)),
    ?assertEqual(undefined, ores_middleware_context:current()).

context(Id) ->
    Hex = iolist_to_binary(io_lib:format("~32.16.0b", [Id])),
    #{
        request_id => iolist_to_binary(io_lib:format("request-~B", [Id])),
        trace_id => Hex,
        tenant_id => iolist_to_binary(io_lib:format("tenant-~B", [Id])),
        user_id => iolist_to_binary(io_lib:format("user-~B", [Id])),
        locale => undefined,
        started_at_unix_ms => 0,
        deadline_unix_ms => undefined,
        baggage => #{}
    }.
