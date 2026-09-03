-module(ores_middleware_otel_tests).
-include_lib("eunit/include/eunit.hrl").

request_logger_is_pinned_to_authenticated_context_test() ->
    Parent = self(),
    Logger = test_logger(Parent),
    Context = test_context(),
    RequestLogger = ores_middleware_otel:request_logger(Logger, Context),
    {ok, Event} = ores_middleware_otel:warn(RequestLogger, <<"slow dependency">>),
    Record = next_loggers:record(Event),
    Fields = maps:get(fields, Record),
    ?assertEqual(<<"WARN">>, maps:get(level, Record)),
    ?assertEqual(maps:get(trace_id, Context), maps:get(traceId, Record)),
    ?assertEqual(<<"request-42">>, maps:get(<<"request.id">>, Fields)),
    ?assertEqual(<<"user-42">>, maps:get(<<"user.id">>, Fields)),
    ?assertEqual(<<"tenant-7">>, maps:get(<<"tenant.id">>, Fields)),
    ?assertEqual(#{<<"otel.vendor">> => <<"allowed">>}, maps:get(<<"otel.baggage">>, Fields)),
    ?assertEqual(false, maps:is_key(<<"authorization">>, maps:get(<<"otel.baggage">>, Fields))),
    ?assertEqual(#{<<"id">> => <<"user-42">>}, maps:get(loggedInUser, Record)),
    receive {record, Record} -> ok after 1000 -> ?assert(false) end,
    ?assertEqual(undefined, next_loggers:current_context()).

file_logger_inherits_middleware_process_context_test() ->
    Parent = self(),
    Logger = test_logger(Parent),
    Context = test_context(),
    {ok, Event} = ores_middleware:run_with_context(Context, fun() ->
        next_loggers:send(next_loggers:info(Logger, <<"handler reached">>))
    end),
    Record = next_loggers:record(Event),
    Fields = maps:get(fields, Record),
    ?assertEqual(<<"request-42">>, maps:get(<<"request.id">>, Fields)),
    ?assertEqual(<<"user-42">>, maps:get(<<"user.id">>, Fields)),
    ?assertEqual(<<"tenant-7">>, maps:get(<<"tenant.id">>, Fields)),
    receive {record, Record} -> ok after 1000 -> ?assert(false) end,
    ?assertEqual(undefined, next_loggers:current_context()).

middleware_attaches_log_object_to_request_test() ->
    Parent = self(),
    Logger = test_logger(Parent),
    Config0 = ores_middleware:default_config(<<"middleware-test">>),
    Settings0 = maps:get(settings, Config0),
    Tls0 = maps:get(tls, Settings0),
    Rate0 = maps:get(rate_limit, Settings0),
    Config = Config0#{
        settings => Settings0#{
            tls => Tls0#{require_https => false},
            rate_limit => Rate0#{enabled => false}
        }
    },
    Hooks = #{
        authenticate => fun(_Request, _Context) ->
            {ok, #{
                user_id => <<"user-42">>,
                tenant_id => <<"tenant-7">>,
                baggage => #{
                    <<"otel.vendor">> => <<"allowed">>,
                    <<"authorization">> => <<"must-not-propagate">>
                }
            }}
        end
    },
    {ok, Middleware} = ores_middleware_otel:create_middleware(Config, Hooks, Logger),
    Request = #{
        method => <<"GET">>,
        path => <<"/orders/42">>,
        scheme => <<"http">>,
        headers => #{
            <<"accept">> => <<"application/json">>,
            <<"x-request-id">> => <<"request-42">>,
            <<"traceparent">> =>
                <<"00-0123456789abcdef0123456789abcdef-0123456789abcdef-01">>
        },
        body_size => 0,
        remote_ip => <<"127.0.0.1">>
    },
    Response = Middleware(Request, fun(RequestWithLog) ->
        RequestLogger = maps:get(log, RequestWithLog),
        ?assertEqual(RequestLogger, maps:get(ores_log, RequestWithLog)),
        Info = maps:get(info, RequestLogger),
        {ok, _} = Info(<<"inside request">>),
        {ok, _} = next_loggers:send(next_loggers:info(Logger, <<"file logger">>)),
        #{
            status => 202,
            headers => #{<<"content-type">> => <<"application/json">>},
            body => <<"{\"ok\":true}">>
        }
    end),
    ?assertEqual(202, maps:get(status, Response)),
    Records = collect_records(4, []),
    Messages = [maps:get(message, Record) || Record <- Records],
    ?assert(lists:member(<<"request handler started">>, Messages)),
    ?assert(lists:member(<<"inside request">>, Messages)),
    ?assert(lists:member(<<"file logger">>, Messages)),
    ?assert(lists:member(<<"request handler completed">>, Messages)),
    lists:foreach(fun(Record) ->
        Fields = maps:get(fields, Record),
        ?assertEqual(<<"request-42">>, maps:get(<<"request.id">>, Fields)),
        ?assertEqual(<<"user-42">>, maps:get(<<"user.id">>, Fields))
    end, Records).

test_logger(Parent) ->
    ores_middleware_otel:new_logger(#{
        app_name => <<"middleware-test">>,
        name => <<"orders">>,
        runtime => <<"erlang">>,
        console => false,
        id_factory => fun() -> <<"record-1">> end,
        clock => fun() -> <<"2026-09-03T00:00:00Z">> end,
        transport => fun(Record) -> Parent ! {record, Record}, ok end
    }).

test_context() -> #{
    request_id => <<"request-42">>,
    trace_id => <<"0123456789abcdef0123456789abcdef">>,
    span_id => <<"0123456789abcdef">>,
    tenant_id => <<"tenant-7">>,
    user_id => <<"user-42">>,
    locale => <<"en-US">>,
    started_at_unix_ms => 1,
    deadline_unix_ms => 2,
    baggage => #{
        <<"otel.vendor">> => <<"allowed">>,
        <<"authorization">> => <<"must-not-propagate">>
    }
}.

collect_records(0, Records) -> lists:reverse(Records);
collect_records(Remaining, Records) ->
    receive
        {record, Record} -> collect_records(Remaining - 1, [Record | Records])
    after 1000 ->
        ?assert(false)
    end.
