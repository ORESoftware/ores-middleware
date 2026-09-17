-module(ores_middleware_generic_tests).

-include_lib("eunit/include/eunit.hrl").

provider_identity_and_failure_values_test() ->
    Prefix = <<"v17:">>,
    Provider = ores_middleware_generic:provider_from(fun(Token) ->
        case Token of
            <<Prefix/binary, Subject/binary>> -> {ok, Subject};
            _ -> {error, {bad_token, Token}}
        end
    end),
    ?assertEqual({ok, <<"alice">>}, Provider(<<"v17:alice">>)),
    ?assertEqual({error, {bad_token, <<"wrong">>}}, Provider(<<"wrong">>)).

contextual_provider_test() ->
    Provider = ores_middleware_generic:contextual_provider_from(fun(Request, Context) ->
        {maps:get(tenant, Context), maps:get(token, Request)}
    end),
    ?assertEqual(
        {<<"t-1">>, <<"abc">>},
        Provider(#{request => #{token => <<"abc">>}, context => #{tenant => <<"t-1">>}})
    ).

middleware_order_and_duplicate_names_test() ->
    Parent = self(),
    Stage = fun(Name) ->
        fun(Next) ->
            fun(Request) ->
                Parent ! {before, Name},
                Response = Next(Request),
                Parent ! {after, Name},
                Response
            end
        end
    end,
    Handler = fun(Request) ->
        Parent ! handler,
        <<Request/binary, ":ok">>
    end,
    Composed = ores_middleware_generic:compose_named(Handler, [
        #{name => auth, middleware => Stage(auth)},
        #{name => auth, middleware => Stage(auth)},
        #{name => audit, middleware => Stage(audit)}
    ]),
    ?assertEqual(<<"request:ok">>, Composed(<<"request">>)),
    ?assertEqual({before, auth}, receive_message()),
    ?assertEqual({before, auth}, receive_message()),
    ?assertEqual({before, audit}, receive_message()),
    ?assertEqual(handler, receive_message()),
    ?assertEqual({after, audit}, receive_message()),
    ?assertEqual({after, auth}, receive_message()),
    ?assertEqual({after, auth}, receive_message()).

empty_chains_preserve_handler_identity_test() ->
    Handler = fun(Request) -> Request end,
    Contextual = fun(Request, Context) -> {Request, Context} end,
    ?assert(ores_middleware_generic:compose(Handler, []) =:= Handler),
    ?assert(ores_middleware_generic:compose_named(Handler, []) =:= Handler),
    ?assert(ores_middleware_generic:compose_contextual(Contextual, []) =:= Contextual),
    ?assert(ores_middleware_generic:compose_named_contextual(Contextual, []) =:= Contextual).

contextual_middleware_test() ->
    Stage = fun(Next) ->
        fun(Request, Context) -> Next(Request, Context#{seen => true}) end
    end,
    Handler = fun(Request, Context) -> {Request, Context} end,
    Composed = ores_middleware_generic:compose_contextual(Handler, [Stage]),
    ?assertEqual(
        {request, #{tenant => <<"t-2">>, seen => true}},
        Composed(request, #{tenant => <<"t-2">>})
    ).

invalid_named_stage_fails_closed_test() ->
    Handler = fun(Request) -> Request end,
    ?assertError(
        {invalid_named_middleware, #{name => missing_middleware}},
        ores_middleware_generic:compose_named(Handler, [#{name => missing_middleware}])
    ).

receive_message() ->
    receive
        Message -> Message
    after 1000 ->
        timeout
    end.
