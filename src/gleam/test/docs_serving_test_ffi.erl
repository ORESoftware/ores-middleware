-module(docs_serving_test_ffi).

-export([fixture_rows/0]).

fixture_rows() ->
    Contents = read_fixture([
        "../../fixtures/docs-serving-conformance.tsv",
        "fixtures/docs-serving-conformance.tsv"
    ]),
    lists:filtermap(
        fun parse_line/1,
        binary:split(Contents, <<"\n">>, [global])
    ).

read_fixture([Path | Remaining]) ->
    case file:read_file(Path) of
        {ok, Contents} ->
            Contents;
        {error, enoent} ->
            read_fixture(Remaining);
        {error, Reason} ->
            erlang:error({docs_serving_fixture_read_failed, Path, Reason})
    end;
read_fixture([]) ->
    erlang:error(docs_serving_fixture_not_found).

parse_line(RawLine) ->
    Line = binary:replace(RawLine, <<"\r">>, <<>>, [global]),
    case Line of
        <<>> ->
            false;
        <<"#", _/binary>> ->
            false;
        _ ->
            case binary:split(Line, <<"\t">>, [global]) of
                [Name, Method, Path, Accept, Format, RuntimeDigest, DocsDigest,
                 Action, Status, Representation, HeadOnly] ->
                    {true,
                     {Name, Method, Path, Accept, Format, RuntimeDigest,
                      DocsDigest, Action, Status, Representation, HeadOnly}};
                Fields ->
                    erlang:error({invalid_docs_serving_fixture_row,
                                  length(Fields), Line})
            end
    end.
