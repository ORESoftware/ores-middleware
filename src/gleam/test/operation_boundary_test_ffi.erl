-module(operation_boundary_test_ffi).

-export([
    raise/0,
    set_outer_metadata/0,
    current_user_id/0,
    current_marker/0,
    clear_metadata/0
]).

raise() -> erlang:error(handler_failed).

set_outer_metadata() ->
    logger:set_process_metadata(#{
        user_id => <<"outer-user">>,
        tenant_id => <<"outer-tenant">>,
        operation_boundary_test_marker => <<"outer-marker">>
    }),
    nil.

current_user_id() ->
    maps:get(user_id, metadata(), <<>>).

current_marker() ->
    maps:get(operation_boundary_test_marker, metadata(), <<>>).

clear_metadata() ->
    logger:unset_process_metadata(),
    nil.

metadata() ->
    case logger:get_process_metadata() of
        Value when is_map(Value) -> Value;
        _ -> #{}
    end.
