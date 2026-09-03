-module(ores_middleware_otel_adversarial_tests).
-include_lib("eunit/include/eunit.hrl").

caller_context_and_metadata_are_restored_test() ->
    PreviousContext = ores_middleware:current_context(),
    PreviousMetadata = logger:get_process_metadata(),
    OuterContext = #{
        request_id => <<"outer-request">>,
        trace_id => <<"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa">>,
        user_id => <<"outer-user">>,
        tenant_id => <<"outer-tenant">>,
        baggage => #{}
    },
    OuterMetadata = #{outer_marker => true, request_id => <<"outer-request">>},
    ores_middleware_context:put(OuterContext),
    logger:set_process_metadata(OuterMetadata),
    try
        Parent = self(),
        Logger = test_logger(Parent),
        {ok, Middleware} = ores_middleware_otel:create_middleware(
            test_config(1000),
            fixed_auth_hooks(),
            Logger
        ),
        Response = Middleware(request(<<"inner-request">>, <<"/restore">>), fun(_Request) ->
            ?assertEqual(<<"inner-request">>, maps:get(request_id, ores_middleware:current_context())),
            #{status => 204, headers => #{}, body => <<>>}
        end),
        ?assertEqual(204, maps:get(status, Response)),
        _ = collect_records(2, []),
        ?assertEqual(OuterContext, ores_middleware:current_context()),
        ?assertEqual(OuterMetadata, logger:get_process_metadata())
    after
        restore_context(PreviousContext),
        restore_metadata(PreviousMetadata)
    end.

parallel_request_processes_never_cross_contaminate_test() ->
    Parent = self(),
    Logger = test_logger(Parent),
    Hooks = #{
        authenticate => fun(Request, _Context) ->
            Headers = maps:get(headers, Request),
            Slot = maps:get(<<"x-test-slot">>, Headers),
            {ok, #{
                user_id => <<"user-", Slot/binary>>,
                tenant_id => <<"tenant-", Slot/binary>>,
                baggage => #{
                    <<"otel.slot">> => Slot,
                    <<"authorization">> => <<"must-not-propagate">>
                }
            }}
        end
    },
    {ok, Middleware} = ores_middleware_otel:create_middleware(test_config(2000), Hooks, Logger),
    RequestCount = 32,
    lists:foreach(fun(Index) ->
        Slot = integer_to_binary(Index),
        spawn(fun() ->
            Response = Middleware(request_for_slot(Slot), fun(RequestWithLog) ->
                RequestLogger = maps:get(log, RequestWithLog),
                {ok, _} = ores_middleware_otel:info(RequestLogger, <<"request:", Slot/binary>>),
                {ok, _} = next_loggers:send(next_loggers:info(Logger, <<"file:", Slot/binary>>)),
                timer:sleep(Index rem 5),
                #{status => 204, headers => #{}, body => <<>>}
            end),
            Parent ! {
                worker_done,
                Slot,
                maps:get(status, Response),
                ores_middleware:current_context(),
                next_loggers:current_context()
            }
        end)
    end, lists:seq(0, RequestCount - 1)),

    {Records, WorkerResults} = collect_parallel(RequestCount, RequestCount * 4, [], []),
    lists:foreach(fun({Slot, Status, PortableContext, LogContext}) ->
        ?assertEqual(204, Status),
        ?assertEqual(undefined, PortableContext),
        ?assertEqual(undefined, LogContext),
        verify_correlated_record(Records, <<"request:", Slot/binary>>, Slot),
        verify_correlated_record(Records, <<"file:", Slot/binary>>, Slot)
    end, WorkerResults).

timeout_is_logged_without_late_completion_and_context_is_restored_test() ->
    PreviousContext = ores_middleware:current_context(),
    PreviousMetadata = logger:get_process_metadata(),
    ores_middleware_context:clear(),
    logger:unset_process_metadata(),
    try
        Parent = self(),
        Logger = test_logger(Parent),
        {ok, Middleware} = ores_middleware_otel:create_middleware(
            test_config(15),
            fixed_auth_hooks(),
            Logger
        ),
        Response = Middleware(request(<<"request-timeout">>, <<"/timeout">>), fun(_Request) ->
            timer:sleep(60),
            #{status => 204, headers => #{}, body => <<>>}
        end),
        ?assertEqual(504, maps:get(status, Response)),
        Records = collect_records(2, []),
        Messages = [maps:get(message, Record) || Record <- Records],
        ?assert(lists:member(<<"request handler started">>, Messages)),
        ?assert(lists:member(<<"request handler timed out">>, Messages)),
        ?assertEqual(false, lists:member(<<"request handler completed">>, Messages)),
        ?assertEqual(undefined, ores_middleware:current_context()),
        ?assertEqual(undefined, next_loggers:current_context()),
        ?assertEqual(undefined, logger:get_process_metadata())
    after
        restore_context(PreviousContext),
        restore_metadata(PreviousMetadata)
    end.

logger_transport_failure_does_not_change_response_test() ->
    PreviousContext = ores_middleware:current_context(),
    PreviousMetadata = logger:get_process_metadata(),
    FailingLogger = ores_middleware_otel:new_logger(#{
        app_name => <<"middleware-adversarial-test">>,
        runtime => <<"erlang">>,
        console => false,
        transport => fun(_Record) -> {error, sink_unavailable} end
    }),
    try
        {ok, Middleware} = ores_middleware_otel:create_middleware(
            test_config(1000),
            fixed_auth_hooks(),
            FailingLogger
        ),
        Response = Middleware(request(<<"request-transport">>, <<"/transport">>), fun(_Request) ->
            #{status => 204, headers => #{}, body => <<>>}
        end),
        ?assertEqual(204, maps:get(status, Response)),
        ?assertEqual(PreviousContext, ores_middleware:current_context()),
        ?assertEqual(PreviousMetadata, logger:get_process_metadata())
    after
        restore_context(PreviousContext),
        restore_metadata(PreviousMetadata)
    end.

verify_correlated_record(Records, Message, Slot) ->
    Matches = [Record || Record <- Records, maps:get(message, Record) =:= Message],
    ?assertEqual(1, length(Matches)),
    [Record] = Matches,
    Fields = maps:get(fields, Record),
    ?assertEqual(<<"request-", Slot/binary>>, maps:get(<<"request.id">>, Fields)),
    ?assertEqual(<<"user-", Slot/binary>>, maps:get(<<"user.id">>, Fields)),
    ?assertEqual(<<"tenant-", Slot/binary>>, maps:get(<<"tenant.id">>, Fields)),
    Baggage = maps:get(<<"otel.baggage">>, Fields),
    ?assertEqual(Slot, maps:get(<<"otel.slot">>, Baggage)),
    ?assertEqual(false, maps:is_key(<<"authorization">>, Baggage)),
    ?assertEqual(
        #{<<"id">> => <<"user-", Slot/binary>>},
        maps:get(loggedInUser, Record)
    ),
    ?assertEqual(nomatch, binary:match(term_to_binary(Record), <<"must-not-propagate">>)).

fixed_auth_hooks() -> #{
    authenticate => fun(_Request, _Context) ->
        {ok, #{
            user_id => <<"user-fixed">>,
            tenant_id => <<"tenant-fixed">>,
            baggage => #{<<"otel.test">> => <<"allowed">>}
        }}
    end
}.

test_config(Timeout) ->
    Config0 = ores_middleware:default_config(<<"middleware-adversarial-test">>),
    Settings0 = maps:get(settings, Config0),
    Tls0 = maps:get(tls, Settings0),
    Rate0 = maps:get(rate_limit, Settings0),
    Idempotency0 = maps:get(idempotency, Settings0),
    Compression0 = maps:get(compression, Settings0),
    Config0#{
        environment => test,
        settings => Settings0#{
            timeout_ms => Timeout,
            tls => Tls0#{mode => disabled, require_https => false},
            rate_limit => Rate0#{enabled => false},
            idempotency => Idempotency0#{enabled => false},
            compression => Compression0#{enabled => false}
        }
    }.

request(RequestId, Path) -> #{
    method => <<"GET">>,
    path => Path,
    scheme => <<"http">>,
    headers => #{
        <<"accept">> => <<"application/json">>,
        <<"x-request-id">> => RequestId,
        <<"traceparent">> =>
            <<"00-0123456789abcdef0123456789abcdef-0123456789abcdef-01">>
    },
    body_size => 0,
    remote_ip => <<"127.0.0.1">>
}.

request_for_slot(Slot) ->
    Base = request(<<"request-", Slot/binary>>, <<"/orders/", Slot/binary>>),
    Headers = maps:get(headers, Base),
    Base#{headers => Headers#{<<"x-test-slot">> => Slot}}.

test_logger(Parent) ->
    ores_middleware_otel:new_logger(#{
        app_name => <<"middleware-adversarial-test">>,
        name => <<"server">>,
        runtime => <<"erlang">>,
        console => false,
        id_factory => fun() -> binary:encode_hex(crypto:strong_rand_bytes(8), lowercase) end,
        clock => fun() -> <<"2026-09-03T00:00:00Z">> end,
        transport => fun(Record) -> Parent ! {record, Record}, ok end
    }).

collect_parallel(0, 0, Records, Workers) ->
    {lists:reverse(Records), lists:reverse(Workers)};
collect_parallel(RemainingWorkers, RemainingRecords, Records, Workers) ->
    receive
        {record, Record} ->
            collect_parallel(RemainingWorkers, RemainingRecords - 1, [Record | Records], Workers);
        {worker_done, Slot, Status, PortableContext, LogContext} ->
            collect_parallel(
                RemainingWorkers - 1,
                RemainingRecords,
                Records,
                [{Slot, Status, PortableContext, LogContext} | Workers]
            )
    after 5000 ->
        erlang:error({parallel_collection_timeout, RemainingWorkers, RemainingRecords})
    end.

collect_records(0, Records) -> lists:reverse(Records);
collect_records(Remaining, Records) ->
    receive
        {record, Record} -> collect_records(Remaining - 1, [Record | Records])
    after 2000 ->
        erlang:error({record_collection_timeout, Remaining})
    end.

restore_context(undefined) -> ores_middleware_context:clear();
restore_context(Context) -> ores_middleware_context:put(Context).

restore_metadata(undefined) -> logger:unset_process_metadata();
restore_metadata(Metadata) -> logger:set_process_metadata(Metadata).
