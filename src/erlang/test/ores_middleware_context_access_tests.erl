-module(ores_middleware_context_access_tests).

-include_lib("eunit/include/eunit.hrl").

context() ->
    #{
        request_id => <<"request-42">>,
        trace_id => <<"0123456789abcdef0123456789abcdef">>,
        user_id => <<"user-42">>,
        tenant_id => <<"tenant-7">>,
        baggage => #{<<"otel.vendor">> => <<"test">>}
    }.

parent_context() ->
    #{
        request_id => <<"parent-request">>,
        trace_id => <<"abcdef0123456789abcdef0123456789">>,
        user_id => <<"parent-user">>,
        tenant_id => <<"parent-tenant">>,
        baggage => #{}
    }.

direct_context_accessors_test() ->
    Context = context(),
    ?assertEqual(<<"request-42">>, ores_middleware_context_access:request_id(Context)),
    ?assertEqual(
        <<"0123456789abcdef0123456789abcdef">>,
        ores_middleware_context_access:trace_id(Context)
    ),
    ?assertEqual(<<"user-42">>, ores_middleware_context_access:user_id(Context)),
    ?assertEqual(
        <<"user-42">>,
        ores_middleware_context_access:logged_in_user_id(Context)
    ),
    ?assertEqual(<<"tenant-7">>, ores_middleware_context_access:tenant_id(Context)).

ambient_context_accessors_restore_process_state_test() ->
    Previous = ores_middleware:current_context(),
    Result = ores_middleware:run_with_context(parent_context(), fun() ->
        ?assertEqual(<<"parent-request">>, ores_middleware:current_request_id()),
        Values = ores_middleware:run_with_context(context(), fun() ->
            {
                ores_middleware:current_request_id(),
                ores_middleware:current_trace_id(),
                ores_middleware:current_user_id(),
                ores_middleware:current_logged_in_user_id(),
                ores_middleware:current_tenant_id()
            }
        end),
        ?assertEqual(<<"parent-request">>, ores_middleware:current_request_id()),
        Values
    end),
    ?assertEqual(
        {
            <<"request-42">>,
            <<"0123456789abcdef0123456789abcdef">>,
            <<"user-42">>,
            <<"user-42">>,
            <<"tenant-7">>
        },
        Result
    ),
    ?assertEqual(Previous, ores_middleware:current_context()).
