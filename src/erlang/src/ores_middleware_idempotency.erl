-module(ores_middleware_idempotency).
-behaviour(gen_server).

-export([start_link/0, get/1, put/3]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

start_link() -> gen_server:start_link({local, ?MODULE}, ?MODULE, #{}, []).

get(Key) -> ensure_started(), gen_server:call(?MODULE, {get, Key}).
put(Key, Response, TtlSeconds) -> ensure_started(), gen_server:call(?MODULE, {put, Key, Response, TtlSeconds}).

init(State) -> {ok, State}.

handle_call({get, Key}, _From, State) ->
    Now = erlang:monotonic_time(millisecond),
    case maps:get(Key, State, undefined) of
        #{expires_at := ExpiresAt, response := Response} when ExpiresAt > Now -> {reply, {ok, Response}, State};
        undefined -> {reply, miss, State};
        _Expired -> {reply, miss, maps:remove(Key, State)}
    end;
handle_call({put, Key, Response, TtlSeconds}, _From, State) ->
    ExpiresAt = erlang:monotonic_time(millisecond) + TtlSeconds * 1000,
    {reply, ok, State#{Key => #{expires_at => ExpiresAt, response => Response}}}.

handle_cast(_Message, State) -> {noreply, State}.
handle_info(_Message, State) -> {noreply, State}.

ensure_started() ->
    case whereis(?MODULE) of
        undefined -> case start_link() of {ok, _Pid} -> ok; {error, {already_started, _Pid}} -> ok end;
        _Pid -> ok
    end.
