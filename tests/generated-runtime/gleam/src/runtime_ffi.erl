-module(runtime_ffi).
-export([get_env/1, read_text/1, valid_rfc3339/1, source_contains_all/2]).

get_env(Name) when is_binary(Name) ->
    case os:getenv(binary_to_list(Name)) of
        false -> {error, nil};
        Value -> {ok, list_to_binary(Value)}
    end.

read_text(Path) when is_binary(Path) ->
    case file:read_file(binary_to_list(Path)) of
        {ok, Binary} -> {ok, Binary};
        {error, _Reason} -> {error, nil}
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

source_contains_all(Path, Fragments) when is_binary(Path), is_list(Fragments) ->
    case file:read_file(binary_to_list(Path)) of
        {ok, Source} ->
            lists:all(
                fun(Fragment) when is_binary(Fragment) ->
                    binary:match(Source, Fragment) =/= nomatch
                end,
                Fragments
            );
        {error, _Reason} ->
            false
    end.
