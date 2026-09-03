#!/usr/bin/env escript
%%! -noshell
-mode(compile).

main(_Args) ->
    Capabilities = [
        <<"request-context">>, <<"panic-recovery">>, <<"request-id">>, <<"trace-context">>, <<"structured-logging">>, <<"metrics-red">>, <<"deadline-timeout">>, <<"payload-limit">>, <<"rate-limit">>, <<"auth">>, <<"sync-observer">>, <<"json">>, <<"headers">>, <<"compression">>, <<"tls-policy">>, <<"security-headers">>, <<"idempotency">>, <<"ip-policy">>, <<"cache-etag">>, <<"content-negotiation">>, <<"fault-injection">>, <<"test-auth-bypass">>, <<"schema-capture">>
    ],
    Descriptor = #{
        <<"contractVersion">> => <<"1.0.0">>, <<"language">> => <<"erlang">>, <<"runtime">> => <<"erlang-otp">>, <<"packageName">> => <<"ores_middleware">>,
        <<"frameworkAdapters">> => [<<"cowboy">>, <<"ranch">>, <<"elli">>, <<"otp">>], <<"capabilities">> => Capabilities,
        <<"operationSymbols">> => #{<<"descriptor">> => <<"ores_middleware:descriptor/0">>, <<"defaultConfig">> => <<"ores_middleware:default_config/1">>, <<"validateConfig">> => <<"ores_middleware:validate_config/1">>, <<"createMiddleware">> => <<"ores_middleware:create_middleware/2">>, <<"runWithContext">> => <<"ores_middleware:run_with_context/2">>, <<"currentContext">> => <<"ores_middleware:current_context/0">>, <<"capabilities">> => <<"ores_middleware:capabilities/0">>}
    },
    io:put_chars(json:encode(Descriptor)),
    io:nl().
