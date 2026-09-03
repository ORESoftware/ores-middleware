-module(ores_middleware_operation).

-export([run/4]).

%% Execute one protocol callback inside the repository request/log context.
%% Raw reasons and stack traces are deliberately discarded; reporters receive
%% only the bounded public failure map and are always fail-open.
run(Context, Descriptor0, Reporter, Fun)
        when is_map(Context), is_map(Descriptor0), is_function(Fun, 0) ->
    Descriptor = normalize_descriptor(Descriptor0),
    ores_middleware_context:run(Context, fun() ->
        case preflight_failure(Descriptor) of
            none -> invoke(Context, Descriptor, Reporter, Fun);
            Kind -> fail(Context, Descriptor, Reporter, Kind, atom_to_binary(Kind, utf8))
        end
    end).

invoke(Context, Descriptor, Reporter, Fun) ->
    try
        {ok, Fun()}
    catch
        Class:_Reason:_Stacktrace ->
            fail(Context, Descriptor, Reporter, panic, safe_class(Class))
    end.

preflight_failure(#{cancelled := true}) -> cancelled;
preflight_failure(#{deadline_unix_ms := Deadline}) when is_integer(Deadline) ->
    case Deadline =< erlang:system_time(millisecond) of
        true -> deadline_exceeded;
        false -> none
    end;
preflight_failure(_) -> none.

fail(Context, Descriptor, Reporter, Kind, ErrorType0) ->
    ErrorType = safe_token(ErrorType0, <<"error">>, 64),
    Failure = #{
        kind => Kind,
        code => failure_code(Kind),
        transport => maps:get(transport, Descriptor),
        scope => maps:get(scope, Descriptor),
        operation => maps:get(name, Descriptor),
        request_id => maps:get(request_id, Context, undefined),
        trace_id => maps:get(trace_id, Context, undefined),
        error_type => ErrorType
    },
    report_safely(Reporter, Failure),
    {error, Failure}.

report_safely(Reporter, Failure) when is_function(Reporter, 1) ->
    try
        _ = Reporter(Failure),
        ok
    catch
        _:_ -> ok
    end;
report_safely(undefined, Failure) ->
    try
        logger:error(#{
            event => operation_failed,
            operation_name => maps:get(operation, Failure),
            operation_transport => maps:get(transport, Failure),
            operation_scope => maps:get(scope, Failure),
            operation_outcome => maps:get(kind, Failure),
            error_type => maps:get(error_type, Failure),
            request_id => maps:get(request_id, Failure),
            trace_id => maps:get(trace_id, Failure)
        })
    catch
        _:_ -> ok
    end;
report_safely(_, _) -> ok.

normalize_descriptor(Descriptor) ->
    #{
        transport => maps:get(transport, Descriptor, http),
        scope => maps:get(scope, Descriptor, request),
        name => safe_token(maps:get(name, Descriptor, <<"operation">>), <<"operation">>, 128),
        cancelled => maps:get(cancelled, Descriptor, false),
        deadline_unix_ms => maps:get(deadline_unix_ms, Descriptor, undefined)
    }.

failure_code(error) -> <<"operation_failed">>;
failure_code(panic) -> <<"operation_panicked">>;
failure_code(cancelled) -> <<"operation_cancelled">>;
failure_code(deadline_exceeded) -> <<"operation_deadline_exceeded">>.

safe_class(error) -> <<"error">>;
safe_class(exit) -> <<"exit">>;
safe_class(throw) -> <<"throw">>;
safe_class(_) -> <<"panic">>.

safe_token(Value, Fallback, Maximum)
        when is_binary(Value), byte_size(Value) > 0, byte_size(Value) =< Maximum ->
    case lists:all(fun safe_token_byte/1, binary_to_list(Value)) of
        true -> Value;
        false -> Fallback
    end;
safe_token(Value, Fallback, Maximum) when is_atom(Value) ->
    safe_token(atom_to_binary(Value, utf8), Fallback, Maximum);
safe_token(_, Fallback, _) -> Fallback.

safe_token_byte(Byte) ->
    (Byte >= $a andalso Byte =< $z) orelse
    (Byte >= $A andalso Byte =< $Z) orelse
    (Byte >= $0 andalso Byte =< $9) orelse
    lists:member(Byte, "_.:-").
