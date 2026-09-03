-module(ores_middleware_context_access).

-export([
    request_id/1,
    trace_id/1,
    user_id/1,
    logged_in_user_id/1,
    tenant_id/1,
    current_request_id/0,
    current_trace_id/0,
    current_user_id/0,
    current_logged_in_user_id/0,
    current_tenant_id/0
]).

request_id(Context) when is_map(Context) -> maps:get(request_id, Context, undefined).
trace_id(Context) when is_map(Context) -> maps:get(trace_id, Context, undefined).
user_id(Context) when is_map(Context) -> maps:get(user_id, Context, undefined).
logged_in_user_id(Context) when is_map(Context) -> user_id(Context).
tenant_id(Context) when is_map(Context) -> maps:get(tenant_id, Context, undefined).

current_request_id() -> current_value(request_id).
current_trace_id() -> current_value(trace_id).
current_user_id() -> current_value(user_id).
current_logged_in_user_id() -> current_user_id().
current_tenant_id() -> current_value(tenant_id).

current_value(Key) ->
    case ores_middleware_context:current() of
        Context when is_map(Context) -> maps:get(Key, Context, undefined);
        _ -> undefined
    end.
