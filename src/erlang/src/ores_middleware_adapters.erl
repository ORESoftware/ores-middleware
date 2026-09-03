-module(ores_middleware_adapters).

-export([cowboy/3, ranch/3, elli/3, otp/3]).

cowboy(Middleware, Request, Next) -> Middleware(Request, Next).
ranch(Middleware, Request, Next) -> Middleware(Request, Next).
elli(Middleware, Request, Next) -> Middleware(Request, Next).
otp(Middleware, Request, Next) -> Middleware(Request, Next).
