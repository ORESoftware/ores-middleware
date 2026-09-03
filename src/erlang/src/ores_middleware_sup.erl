-module(ores_middleware_sup).
-behaviour(supervisor).

-export([start_link/0, init/1]).

start_link() -> supervisor:start_link({local, ?MODULE}, ?MODULE, []).

init([]) ->
    Children = [
        #{id => ores_middleware_rate_limiter, start => {ores_middleware_rate_limiter, start_link, []}, restart => permanent, shutdown => 5000, type => worker, modules => [ores_middleware_rate_limiter]},
        #{id => ores_middleware_idempotency, start => {ores_middleware_idempotency, start_link, []}, restart => permanent, shutdown => 5000, type => worker, modules => [ores_middleware_idempotency]}
    ],
    {ok, {#{strategy => one_for_one, intensity => 5, period => 10}, Children}}.
