-module(ores_middleware_context_ffi).

-export([
    get_context/0,
    put_context/1,
    clear_context/0,
    run_operation/5,
    run_with_deadline/3,
    system_time_ms/0,
    new_id/0,
    random_float/0,
    sleep/1
]).

-define(CONTEXT_KEY, '$ores_middleware_context').
-define(OWNED_METADATA_KEYS, [request_id, trace_id, tenant_id, user_id]).

get_context() ->
    case erlang:get(?CONTEXT_KEY) of
        undefined -> {error, nil};
        Value -> {ok, Value}
    end.

put_context(Value) ->
    erlang:put(?CONTEXT_KEY, Value),
    nil.

clear_context() ->
    erlang:erase(?CONTEXT_KEY),
    nil.

run_operation(Context, Transport, Scope, Name, Fun) ->
    with_context(Context, fun() ->
        try
            {ok, Fun()}
        catch
            Class:_Reason:_Stacktrace ->
                safe_log_failure(Transport, Scope, Name, Class),
                {error, <<"operation_failed">>}
        end
    end).

run_with_deadline(Fun, TimeoutMs, Context) ->
    Parent = self(),
    Ref = make_ref(),
    {Pid, MonitorRef} = spawn_monitor(fun() ->
        Result = run_operation(
            Context,
            <<"http">>,
            <<"request">>,
            <<"middleware.handler">>,
            Fun
        ),
        Parent ! {Ref, Result}
    end),
    receive
        {Ref, Result} ->
            erlang:demonitor(MonitorRef, [flush]),
            Result;
        {'DOWN', MonitorRef, process, Pid, _Reason} ->
            {error, <<"handler_failed">>}
    after TimeoutMs ->
        exit(Pid, kill),
        receive
            {'DOWN', MonitorRef, process, Pid, _Reason} -> ok
        end,
        receive
            {Ref, _LateResult} -> ok
        after 0 -> ok
        end,
        {error, <<"deadline_exceeded">>}
    end.

with_context(Context, Fun) ->
    PreviousContext = erlang:get(?CONTEXT_KEY),
    PreviousMetadata = logger:get_process_metadata(),
    erlang:put(?CONTEXT_KEY, Context),
    install_metadata(Context, PreviousMetadata),
    try
        Fun()
    after
        restore_context(PreviousContext),
        restore_metadata(PreviousMetadata)
    end.

%% Preserve metadata owned by the application while replacing, rather than
%% merging, the request-owned keys. This prevents an anonymous nested scope
%% from inheriting an outer request's user or tenant identifiers.
install_metadata(Context, PreviousMetadata) ->
    Base = remove_owned_metadata(PreviousMetadata),
    Next = maps:merge(Base, context_metadata(Context)),
    case map_size(Next) of
        0 -> logger:unset_process_metadata();
        _ -> logger:set_process_metadata(Next)
    end.

remove_owned_metadata(Metadata) when is_map(Metadata) ->
    maps:without(?OWNED_METADATA_KEYS, Metadata);
remove_owned_metadata(_) ->
    #{}.

context_metadata({request_context, RequestId, TraceId, TenantId, UserId, _, _, _, _}) ->
    compact_metadata(#{
        request_id => RequestId,
        trace_id => TraceId,
        tenant_id => TenantId,
        user_id => UserId
    });
context_metadata(_) ->
    #{}.

compact_metadata(Metadata) ->
    maps:filter(
        fun(_, Value) -> is_binary(Value) andalso byte_size(Value) > 0 end,
        Metadata
    ).

safe_log_failure(Transport, Scope, Name, Class) ->
    try
        logger:error(#{
            event => operation_failed,
            operation_name => safe_name(Name),
            operation_transport => safe_name(Transport),
            operation_scope => safe_name(Scope),
            error_type => safe_class(Class)
        })
    catch
        _:_ -> ok
    end.

safe_name(Value) when is_binary(Value), byte_size(Value) > 0, byte_size(Value) =< 128 ->
    case lists:all(fun safe_name_byte/1, binary:bin_to_list(Value)) of
        true -> Value;
        false -> <<"operation">>
    end;
safe_name(_) ->
    <<"operation">>.

safe_name_byte(Byte) ->
    (Byte >= $a andalso Byte =< $z) orelse
    (Byte >= $A andalso Byte =< $Z) orelse
    (Byte >= $0 andalso Byte =< $9) orelse
    lists:member(Byte, "_.:-").

safe_class(error) -> error;
safe_class(exit) -> exit;
safe_class(throw) -> throw;
safe_class(_) -> unknown.

restore_context(undefined) -> erlang:erase(?CONTEXT_KEY), ok;
restore_context(Context) -> erlang:put(?CONTEXT_KEY, Context), ok.

restore_metadata(undefined) -> logger:unset_process_metadata();
restore_metadata(Metadata) -> logger:set_process_metadata(Metadata).

system_time_ms() -> erlang:system_time(millisecond).
new_id() -> binary:encode_hex(crypto:strong_rand_bytes(16), lowercase).
random_float() -> rand:uniform().
sleep(Milliseconds) -> timer:sleep(Milliseconds), nil.
