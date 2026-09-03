-module(ores_middleware_rate_limiter).
-behaviour(gen_server).

-export([start_link/0, allow/3]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

start_link() -> gen_server:start_link({local, ?MODULE}, ?MODULE, #{}, []).

allow(Key, Capacity, RefillPerSecond) ->
    ensure_started(),
    gen_server:call(?MODULE, {allow, Key, Capacity, RefillPerSecond}).

init(State) -> {ok, State}.

handle_call({allow, Key, Capacity, Refill}, _From, State) ->
    Now = erlang:monotonic_time(microsecond),
    #{tokens := OldTokens, updated := Updated} = maps:get(Key, State, #{tokens => Capacity * 1.0, updated => Now}),
    Tokens0 = OldTokens + ((Now - Updated) / 1000000) * Refill,
    Tokens1 = erlang:min(Capacity * 1.0, Tokens0),
    Allowed = Tokens1 >= 1.0,
    Tokens2 = case Allowed of true -> Tokens1 - 1.0; false -> Tokens1 end,
    {reply, Allowed, State#{Key => #{tokens => Tokens2, updated => Now}}}.

handle_cast(_Message, State) -> {noreply, State}.
handle_info(_Message, State) -> {noreply, State}.

ensure_started() ->
    case whereis(?MODULE) of
        undefined -> case start_link() of {ok, _Pid} -> ok; {error, {already_started, _Pid}} -> ok end;
        _Pid -> ok
    end.
