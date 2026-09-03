-module(ores_middleware_context_ffi).

-export([
    get_context/0,
    put_context/1,
    clear_context/0,
    run_with_deadline/3,
    system_time_ms/0,
    new_id/0,
    random_float/0,
    sleep/1
]).

-define(CONTEXT_KEY, '$ores_middleware_context').

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

run_with_deadline(Fun, TimeoutMs, Context) ->
    Parent = self(),
    Ref = make_ref(),
    Pid = spawn(fun() ->
        erlang:put(?CONTEXT_KEY, Context),
        Result = try {ok, Fun()} catch _:_ -> {error, <<"handler_failed">>} end,
        Parent ! {Ref, Result}
    end),
    receive
        {Ref, Result} -> Result
    after TimeoutMs ->
        exit(Pid, kill),
        {error, <<"deadline_exceeded">>}
    end.

system_time_ms() -> erlang:system_time(millisecond).
new_id() -> binary:encode_hex(crypto:strong_rand_bytes(16), lowercase).
random_float() -> rand:uniform().
sleep(Milliseconds) -> timer:sleep(Milliseconds), nil.
