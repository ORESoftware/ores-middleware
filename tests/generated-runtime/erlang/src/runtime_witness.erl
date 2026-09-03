-module(runtime_witness).
-export([main/1]).

-define(WITNESS_SCHEMA, <<"ores.generated-runtime-witness/v1">>).
-define(GENERATED, ores_middleware_generated_idempotency_record).

main([FixturePath, Authority, GeneratedPath]) ->
    ok = source_shape_guard(GeneratedPath),
    {ok, FixtureBytes} = file:read_file(FixturePath),
    Fixture = jsx:decode(FixtureBytes, [return_maps]),
    Cases = [case_result(TestCase, Fixture) || TestCase <- maps:get(<<"cases">>, Fixture)],
    Statuses = maps:get(<<"statuses">>, Fixture),
    StatusAcceptance = maps:from_list([
        {Status, ?GENERATED:valid_idempotency_status(Status)}
        || Status <- Statuses ++ [<<"__unknown__">>]
    ]),
    Witness = #{
        <<"schema">> => ?WITNESS_SCHEMA,
        <<"authority">> => list_to_binary(Authority),
        <<"language">> => <<"erlang">>,
        <<"model">> => maps:get(<<"model">>, Fixture),
        <<"wireFields">> => lists:sort(maps:get(<<"wireFields">>, Fixture)),
        <<"requiredFields">> => lists:sort(maps:get(<<"requiredFields">>, Fixture)),
        <<"optionalFields">> => lists:sort(maps:get(<<"optionalFields">>, Fixture)),
        <<"statuses">> => maps:get(<<"statuses">>, Fixture),
        <<"statusAcceptance">> => StatusAcceptance,
        <<"cases">> => Cases
    },
    io:put_chars([jsx:encode(Witness), $\n]);
main(_) ->
    erlang:error({usage, "runtime_witness <fixture.json> <authority> <generated.erl>"}).

case_result(TestCase, Fixture) ->
    Id = maps:get(<<"id">>, TestCase),
    Value = maps:get(<<"value">>, TestCase),
    case strict_decode(Value, Fixture) of
        {ok, Record} ->
            #{
                <<"id">> => Id,
                <<"accepted">> => true,
                <<"normalized">> => normalize(Record)
            };
        error ->
            #{
                <<"id">> => Id,
                <<"accepted">> => false,
                <<"normalized">> => null
            }
    end.

strict_decode(Value, Fixture) when is_map(Value) ->
    Allowed = maps:get(<<"wireFields">>, Fixture),
    Required = maps:get(<<"requiredFields">>, Fixture),
    KeysValid = lists:all(fun(Key) -> lists:member(Key, Allowed) end, maps:keys(Value)),
    RequiredValid = lists:all(fun(Key) -> required_binary(Value, Key) end, Required),
    OptionalValid = optional_binary(Value, <<"responseBody">>)
        andalso optional_i32(Value, <<"responseStatus">>),
    case KeysValid andalso RequiredValid andalso OptionalValid of
        false ->
            error;
        true ->
            CreatedAt = maps:get(<<"createdAt">>, Value),
            ExpiresAt = maps:get(<<"expiresAt">>, Value),
            Status = maps:get(<<"status">>, Value),
            case valid_rfc3339(CreatedAt)
                andalso valid_rfc3339(ExpiresAt)
                andalso ?GENERATED:valid_idempotency_status(Status) of
                false ->
                    error;
                true ->
                    Record0 = #{
                        created_at => CreatedAt,
                        expires_at => ExpiresAt,
                        id => maps:get(<<"id">>, Value),
                        idempotency_key => maps:get(<<"idempotencyKey">>, Value),
                        request_hash => maps:get(<<"requestHash">>, Value),
                        status => Status,
                        tenant_id => maps:get(<<"tenantId">>, Value)
                    },
                    Record1 = maybe_copy(Value, <<"responseBody">>, Record0, response_body),
                    Record2 = maybe_copy(Value, <<"responseStatus">>, Record1, response_status),
                    {ok, Record2}
            end
    end;
strict_decode(_, _) ->
    error.

required_binary(Value, Key) ->
    case maps:find(Key, Value) of
        {ok, Item} when is_binary(Item) -> true;
        _ -> false
    end.

optional_binary(Value, Key) ->
    case maps:find(Key, Value) of
        error -> true;
        {ok, Item} when is_binary(Item) -> true;
        _ -> false
    end.

optional_i32(Value, Key) ->
    case maps:find(Key, Value) of
        error -> true;
        {ok, Item} when is_integer(Item), Item >= -2147483648, Item =< 2147483647 -> true;
        _ -> false
    end.

valid_rfc3339(Value) when is_binary(Value) ->
    case re:run(
        Value,
        <<"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]+)?(Z|[+-][0-9]{2}:[0-9]{2})$">>,
        [{capture, none}, unicode]
    ) of
        match -> true;
        nomatch -> false
    end;
valid_rfc3339(_) ->
    false.

maybe_copy(Source, SourceKey, Target, TargetKey) ->
    case maps:find(SourceKey, Source) of
        {ok, Value} -> Target#{TargetKey => Value};
        error -> Target
    end.

normalize(Record) ->
    Base = #{
        <<"createdAt">> => maps:get(created_at, Record),
        <<"expiresAt">> => maps:get(expires_at, Record),
        <<"id">> => maps:get(id, Record),
        <<"idempotencyKey">> => maps:get(idempotency_key, Record),
        <<"requestHash">> => maps:get(request_hash, Record),
        <<"status">> => maps:get(status, Record),
        <<"tenantId">> => maps:get(tenant_id, Record)
    },
    WithBody = maybe_native_to_wire(Record, response_body, Base, <<"responseBody">>),
    maybe_native_to_wire(Record, response_status, WithBody, <<"responseStatus">>).

maybe_native_to_wire(Source, SourceKey, Target, TargetKey) ->
    case maps:find(SourceKey, Source) of
        {ok, Value} -> Target#{TargetKey => Value};
        error -> Target
    end.

source_shape_guard(Path) ->
    {ok, Source} = file:read_file(Path),
    Fragments = [
        <<"created_at := binary()">>,
        <<"expires_at := binary()">>,
        <<"id := binary()">>,
        <<"idempotency_key := binary()">>,
        <<"request_hash := binary()">>,
        <<"response_body => binary()">>,
        <<"response_status => integer()">>,
        <<"status := idempotency_status()">>,
        <<"tenant_id := binary()">>,
        <<"valid_idempotency_status(<<\"pending\">>) -> true;">>,
        <<"valid_idempotency_status(<<\"succeeded\">>) -> true;">>,
        <<"valid_idempotency_status(<<\"failed\">>) -> true;">>
    ],
    case lists:all(fun(Fragment) -> binary:match(Source, Fragment) =/= nomatch end, Fragments) of
        true -> ok;
        false -> erlang:error(generated_erlang_source_shape_mismatch)
    end.
