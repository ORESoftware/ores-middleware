-module(ores_middleware_context).

-export([current/0, put/1, clear/0, run/2]).

-define(KEY, '$ores_middleware_context').

current() -> erlang:get(?KEY).

put(Context) when is_map(Context) ->
    erlang:put(?KEY, Context),
    logger:update_process_metadata(#{
        request_id => maps:get(request_id, Context, undefined),
        trace_id => maps:get(trace_id, Context, undefined),
        tenant_id => maps:get(tenant_id, Context, undefined),
        user_id => maps:get(user_id, Context, undefined)
    }),
    ok.

clear() ->
    erlang:erase(?KEY),
    ok.

run(Context, Fun) when is_function(Fun, 0) ->
    PreviousContext = current(),
    PreviousMetadata = logger:get_process_metadata(),
    put(Context),
    try
        next_loggers:with_context(
            ores_middleware_otel:to_log_context(Context),
            Fun
        )
    after
        restore_context(PreviousContext),
        restore_metadata(PreviousMetadata)
    end.

restore_context(undefined) -> clear();
restore_context(Context) -> erlang:put(?KEY, Context), ok.

restore_metadata(undefined) -> logger:unset_process_metadata();
restore_metadata(Metadata) -> logger:set_process_metadata(Metadata).
