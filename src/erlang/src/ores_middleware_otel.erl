-module(ores_middleware_otel).

-compile({no_auto_import, [error/2, error/3]}).

-export([
    new_logger/1,
    new_logger/3,
    new_logger/4,
    otel_transport/1,
    supabase_transport/1,
    use_otel/1,
    not_otel/1,
    to_log_context/1,
    request_logger/2,
    root_logger/1,
    attach/3,
    with_context/2,
    trace/2,
    trace/3,
    debug/2,
    debug/3,
    info/2,
    info/3,
    warn/2,
    warn/3,
    error/2,
    error/3,
    fatal/2,
    fatal/3,
    create_middleware/3
]).

%% Re-expose the canonical Erlang logger constructors and transports so a
%% service may depend on ores_middleware as its integration surface.
new_logger(Options) -> next_loggers:new(Options).
new_logger(AppName, Runtime, Transports) -> next_loggers:new(AppName, Runtime, Transports).
new_logger(AppName, Runtime, Fields, Transports) ->
    next_loggers:new(AppName, Runtime, Fields, Transports).
otel_transport(Sink) -> next_loggers:otel_transport(Sink).
supabase_transport(Sender) -> next_loggers:supabase_transport(Sender).
use_otel(Value) -> next_loggers:use_otel(Value).
not_otel(Value) -> next_loggers:not_otel(Value).

%% The wire/request context remains a data map. Logger handles live beside it.
to_log_context(Context) when is_map(Context) ->
    Fields0 = #{},
    Fields1 = maybe_put(Fields0, <<"request.id">>, maps:get(request_id, Context, undefined)),
    Fields2 = maybe_put(Fields1, <<"trace.id">>, maps:get(trace_id, Context, undefined)),
    Fields3 = maybe_put(
        Fields2,
        <<"request.started_at_unix_ms">>,
        maps:get(started_at_unix_ms, Context, undefined)
    ),
    Fields4 = maybe_put(
        Fields3,
        <<"request.deadline_unix_ms">>,
        maps:get(deadline_unix_ms, Context, undefined)
    ),
    Fields5 = maybe_put(Fields4, <<"user.id">>, maps:get(user_id, Context, undefined)),
    Fields6 = maybe_put(Fields5, <<"tenant.id">>, maps:get(tenant_id, Context, undefined)),
    Fields7 = maybe_put(Fields6, <<"request.locale">>, maps:get(locale, Context, undefined)),
    Baggage = filter_otel_baggage(maps:get(baggage, Context, #{})),
    Fields = case map_size(Baggage) of
        0 -> Fields7;
        _ -> Fields7#{<<"otel.baggage">> => Baggage}
    end,
    TraceId = maps:get(trace_id, Context, undefined),
    TraceIds = case empty(TraceId) of true -> []; false -> [TraceId] end,
    #{
        fields => Fields,
        trace_id => TraceId,
        trace_ids => TraceIds,
        span_id => maps:get(span_id, Context, undefined),
        trace_flags => maps:get(trace_flags, Context, 0),
        trace_state => maps:get(trace_state, Context, undefined),
        baggage => Baggage,
        routine_id => maps:get(request_id, Context, undefined),
        tags => [<<"ores-middleware">>, <<"request">>]
    }.

request_logger(Logger, Context) when is_map(Logger), is_map(Context) ->
    LogContext = to_log_context(Context),
    ContextFields = maps:get(fields, LogContext),
    UserId = maps:get(user_id, Context, undefined),
    LoggedInUser = case empty(UserId) of
        true -> maps:get(logged_in_user, Logger, #{});
        false -> maps:merge(maps:get(logged_in_user, Logger, #{}), #{id => UserId})
    end,
    Child = Logger#{
        name => request_logger_name(maps:get(name, Logger, undefined)),
        fields => maps:merge(maps:get(fields, Logger, #{}), ContextFields),
        logged_in_user => LoggedInUser
    },
    Pinned = #{logger => Child, context => LogContext},
    Pinned#{
        trace => fun(Values) -> trace(Pinned, Values) end,
        debug => fun(Values) -> debug(Pinned, Values) end,
        info => fun(Values) -> info(Pinned, Values) end,
        warn => fun(Values) -> warn(Pinned, Values) end,
        error => fun(Values) -> error(Pinned, Values) end,
        fatal => fun(Values) -> fatal(Pinned, Values) end,
        trace_fields => fun(Values, Fields) -> trace(Pinned, Values, Fields) end,
        debug_fields => fun(Values, Fields) -> debug(Pinned, Values, Fields) end,
        info_fields => fun(Values, Fields) -> info(Pinned, Values, Fields) end,
        warn_fields => fun(Values, Fields) -> warn(Pinned, Values, Fields) end,
        error_fields => fun(Values, Fields) -> error(Pinned, Values, Fields) end,
        fatal_fields => fun(Values, Fields) -> fatal(Pinned, Values, Fields) end
    }.

root_logger(#{logger := Logger}) -> Logger.

attach(Request, Logger, Context) when is_map(Request), is_map(Logger), is_map(Context) ->
    RequestLogger = request_logger(Logger, Context),
    Request#{log => RequestLogger, ores_log => RequestLogger}.

with_context(#{context := Context}, Fun) when is_function(Fun, 0) ->
    next_loggers:with_context(Context, Fun);
with_context(Context, Fun) when is_map(Context), is_function(Fun, 0) ->
    next_loggers:with_context(to_log_context(Context), Fun).

trace(RequestLogger, Values) -> trace(RequestLogger, Values, #{}).
trace(RequestLogger, Values, Fields) -> send_level(RequestLogger, trace, Values, Fields).
debug(RequestLogger, Values) -> debug(RequestLogger, Values, #{}).
debug(RequestLogger, Values, Fields) -> send_level(RequestLogger, debug, Values, Fields).
info(RequestLogger, Values) -> info(RequestLogger, Values, #{}).
info(RequestLogger, Values, Fields) -> send_level(RequestLogger, info, Values, Fields).
warn(RequestLogger, Values) -> warn(RequestLogger, Values, #{}).
warn(RequestLogger, Values, Fields) -> send_level(RequestLogger, warn, Values, Fields).
error(RequestLogger, Values) -> error(RequestLogger, Values, #{}).
error(RequestLogger, Values, Fields) -> send_level(RequestLogger, error, Values, Fields).
fatal(RequestLogger, Values) -> fatal(RequestLogger, Values, #{}).
fatal(RequestLogger, Values, Fields) -> send_level(RequestLogger, fatal, Values, Fields).

send_level(#{logger := Logger, context := Context}, Level, Values, Fields)
        when is_atom(Level), is_map(Fields) ->
    Event0 = level_event(Level, Logger, Values),
    Event1 = next_loggers:add_fields(Event0, Fields),
    Event2 = maybe_add_trace(Event1, maps:get(trace_id, Context, undefined)),
    Event3 = maybe_add_routine(Event2, maps:get(routine_id, Context, undefined)),
    Event4 = next_loggers:add_tags(Event3, maps:get(tags, Context, [])),
    next_loggers:send(Event4).

level_event(trace, Logger, Values) -> next_loggers:trace(Logger, Values);
level_event(debug, Logger, Values) -> next_loggers:debug(Logger, Values);
level_event(info, Logger, Values) -> next_loggers:info(Logger, Values);
level_event(warn, Logger, Values) -> next_loggers:warn(Logger, Values);
level_event(error, Logger, Values) -> next_loggers:error(Logger, Values);
level_event(fatal, Logger, Values) -> next_loggers:fatal(Logger, Values).

%% Authentication and the portable policy stack run first. The handler then
%% receives Request#{log := RequestLogger}; ordinary imported file loggers share
%% the same process-local context while the callback executes.
create_middleware(Config, Hooks, Logger) when is_map(Logger) ->
    case ores_middleware:create_middleware(Config, Hooks) of
        {error, Issues} -> {error, Issues};
        {ok, Base} ->
            {ok, fun(Request, Next) ->
                Base(Request, fun(ScopedRequest) ->
                    case ores_middleware:current_context() of
                        Context when is_map(Context) ->
                            RequestWithLog = attach(ScopedRequest, Logger, Context),
                            RequestLogger = maps:get(log, RequestWithLog),
                            RequestFields = #{
                                <<"http.request.method">> => maps:get(method, ScopedRequest, undefined),
                                <<"url.path">> => maps:get(path, ScopedRequest, undefined)
                            },
                            _ = info(RequestLogger, <<"request handler started">>, RequestFields),
                            try
                                Response = with_context(RequestLogger, fun() -> Next(RequestWithLog) end),
                                _ = info(
                                    RequestLogger,
                                    <<"request handler completed">>,
                                    maybe_put(
                                        RequestFields,
                                        <<"http.response.status_code">>,
                                        maps:get(status, Response, undefined)
                                    )
                                ),
                                Response
                            catch
                                Class:Reason:Stacktrace ->
                                    _ = error(RequestLogger, <<"request handler failed">>, RequestFields),
                                    erlang:raise(Class, Reason, Stacktrace)
                            end;
                        _ -> Next(ScopedRequest)
                    end
                end)
            end}
    end.

request_logger_name(undefined) -> <<"request">>;
request_logger_name(<<>>) -> <<"request">>;
request_logger_name(Name) when is_binary(Name) -> <<Name/binary, ":request">>;
request_logger_name(Name) ->
    NameBinary = iolist_to_binary(io_lib:format("~tp", [Name])),
    <<NameBinary/binary, ":request">>.

maybe_add_trace(Event, Value) ->
    case empty(Value) of true -> Event; false -> next_loggers:add_trace(Event, Value) end.

maybe_add_routine(Event, Value) ->
    case empty(Value) of true -> Event; false -> next_loggers:add_routine_id(Event, Value) end.

maybe_put(Map, _Key, undefined) -> Map;
maybe_put(Map, _Key, <<>>) -> Map;
maybe_put(Map, Key, Value) -> Map#{Key => Value}.

empty(undefined) -> true;
empty(<<>>) -> true;
empty(_) -> false.

filter_otel_baggage(Baggage) when is_map(Baggage) ->
    maps:filter(fun(Key, _Value) -> has_otel_prefix(Key) end, Baggage);
filter_otel_baggage(_) -> #{}.

has_otel_prefix(Key) when is_binary(Key) ->
    case Key of <<"otel.", _/binary>> -> true; _ -> false end;
has_otel_prefix(Key) when is_atom(Key) -> has_otel_prefix(atom_to_binary(Key));
has_otel_prefix(_) -> false.
