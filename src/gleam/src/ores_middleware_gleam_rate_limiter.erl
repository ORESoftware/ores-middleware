-module(ores_middleware_gleam_rate_limiter).
-behaviour(gen_server).

-export([allow/3]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

-define(MAX_ENTRIES, 10000).

allow(Key, Capacity, RefillPerSecond) ->
    ensure_started(),
    gen_server:call(?MODULE, {allow, Key, Capacity, RefillPerSecond}).

init(State) -> {ok, State}.

handle_call({allow, Key, Capacity, Refill}, _From, State0) ->
    Now = erlang:monotonic_time(microsecond),
    {State1, Bucket} = ensure_bucket(State0, Key, Capacity, Now),
    #{tokens := OldTokens, updated := Updated} = Bucket,
    Tokens0 = OldTokens + ((Now - Updated) / 1000000) * Refill,
    Tokens1 = erlang:min(Capacity * 1.0, Tokens0),
    Allowed = Tokens1 >= 1.0,
    Tokens2 = case Allowed of true -> Tokens1 - 1.0; false -> Tokens1 end,
    Buckets1 = maps:get(buckets, State1),
    State2 = State1#{buckets => Buckets1#{Key => #{tokens => Tokens2, updated => Now}}},
    {reply, Allowed, State2}.

handle_cast(_Message, State) -> {noreply, State}.
handle_info(_Message, State) -> {noreply, State}.

ensure_bucket(State, Key, Capacity, Now) ->
    Buckets = maps:get(buckets, State),
    case maps:find(Key, Buckets) of
        {ok, Bucket} -> {State, Bucket};
        error ->
            State1 = evict_if_full(State),
            Buckets1 = maps:get(buckets, State1),
            Bucket = #{tokens => Capacity * 1.0, updated => Now},
            Order1 = queue:in(Key, maps:get(order, State1)),
            {State1#{buckets => Buckets1#{Key => Bucket}, order => Order1}, Bucket}
    end.

evict_if_full(State) ->
    Buckets = maps:get(buckets, State),
    case map_size(Buckets) < ?MAX_ENTRIES of
        true -> State;
        false ->
            case queue:out(maps:get(order, State)) of
                {{value, Oldest}, Order1} ->
                    State#{buckets => maps:remove(Oldest, Buckets), order => Order1};
                {empty, _} -> State
            end
    end.

ensure_started() ->
    case whereis(?MODULE) of
        undefined ->
            case gen_server:start_link({local, ?MODULE}, ?MODULE, #{buckets => #{}, order => queue:new()}, []) of
                {ok, _Pid} -> ok;
                {error, {already_started, _Pid}} -> ok
            end;
        _Pid -> ok
    end.
