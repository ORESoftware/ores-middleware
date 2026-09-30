-module(ores_middleware_rate_limiter).
-behaviour(gen_server).

-export([start_link/0, allow/3]).
-export([init/1, handle_call/3, handle_cast/2, handle_info/2]).

-define(MAX_ENTRIES, 10000).

start_link() -> gen_server:start_link({local, ?MODULE}, ?MODULE, #{buckets => #{}, order => queue:new()}, []).

allow(Key, Capacity, RefillPerSecond) ->
    ensure_started(),
    gen_server:call(?MODULE, {allow, Key, Capacity, RefillPerSecond}).

init(State) -> {ok, State}.

handle_call({allow, Key, Capacity, Refill}, _From, State0) ->
    Now = erlang:monotonic_time(microsecond),
    State = normalize_state(State0),
    {State1, Bucket} = ensure_bucket(State, Key, Capacity, Now),
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

normalize_state(#{buckets := Buckets, order := Order} = State)
        when is_map(Buckets) ->
    case queue:is_queue(Order) of
        true -> State;
        false -> migrate_legacy_state(Buckets)
    end;
normalize_state(State) when is_map(State) -> migrate_legacy_state(State).

migrate_legacy_state(LegacyBuckets) ->
    %% One-time hot-upgrade path from the previous unbounded map state.
    Keys = lists:sublist(maps:keys(LegacyBuckets), ?MAX_ENTRIES),
    #{buckets => maps:with(Keys, LegacyBuckets), order => queue:from_list(Keys)}.

ensure_bucket(State, Key, Capacity, Now) ->
    Buckets = maps:get(buckets, State),
    case maps:find(Key, Buckets) of
        {ok, Bucket} -> {State, Bucket};
        error ->
            %% HOT-PATH: one FIFO eviction per new key keeps attacker-controlled
            %% IP cardinality bounded without scanning the bucket map.
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
        undefined -> case start_link() of {ok, _Pid} -> ok; {error, {already_started, _Pid}} -> ok end;
        _Pid -> ok
    end.
