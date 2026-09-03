-module(ores_middleware).

-export([
    capabilities/0,
    default_config/1,
    validate_config/1,
    default_hooks/0,
    create_middleware/2,
    run/4,
    prepare/3,
    finish/6,
    run_with_context/2,
    current_context/0,
    descriptor/0,
    decode_json/1,
    tls_options/2
]).

-define(CONTRACT_VERSION, <<"1.0.0">>).

capabilities() -> [
    <<"request-context">>, <<"panic-recovery">>, <<"request-id">>, <<"trace-context">>,
    <<"structured-logging">>, <<"metrics-red">>, <<"deadline-timeout">>, <<"payload-limit">>,
    <<"rate-limit">>, <<"auth">>, <<"sync-observer">>, <<"json">>, <<"headers">>,
    <<"compression">>, <<"tls-policy">>, <<"security-headers">>, <<"idempotency">>,
    <<"ip-policy">>, <<"cache-etag">>, <<"content-negotiation">>, <<"fault-injection">>,
    <<"test-auth-bypass">>, <<"schema-capture">>
].

default_config(ServiceName) when is_binary(ServiceName), byte_size(ServiceName) > 0 ->
    #{
        contract_version => ?CONTRACT_VERSION,
        environment => development,
        required_capabilities => capabilities(),
        settings => #{
            request_id_header => <<"x-request-id">>, trace_header => <<"traceparent">>, timeout_ms => 5000,
            max_body_bytes => 2 * 1024 * 1024, context_registry_max_entries => 10000, context_registry_ttl_ms => 30000,
            rate_limit => #{enabled => true, capacity => 100, refill_per_second => 20.0, key_by => [tenant, user, ip, route]},
            compression => #{enabled => true, minimum_bytes => 1024, algorithms => [<<"gzip">>]},
            tls => #{mode => trusted_proxy, require_https => true, strict_forwarded_headers => true, trusted_proxy_cidrs => [<<"127.0.0.1/32">>, <<"::1/128">>]},
            security_headers => #{enabled => true, hsts_max_age_seconds => 31536000, content_security_policy => <<"default-src 'self'; frame-ancestors 'none'">>, frame_options => <<"DENY">>},
            idempotency => #{enabled => true, header_name => <<"idempotency-key">>, ttl_seconds => 86400, required_methods => [<<"POST">>, <<"PUT">>, <<"PATCH">>]},
            fault_injection => #{enabled => false, latency_ms => 0, error_rate => 0.0, drop_rate => 0.0},
            test_auth_bypass => #{enabled => false, header_name => <<"x-test-auth-bypass">>, allowed_cidrs => [<<"127.0.0.1/32">>, <<"::1/128">>]},
            content_representations => [<<"application/json">>, <<"application/problem+json">>]
        },
        integrations => #{
            shared_auth => #{mode => disabled, issuer => undefined, audience => undefined, jwks_uri => undefined, introspection_url => undefined, fail_open => false},
            opto_sync => #{mode => disabled, endpoint => undefined, outbox_topic => undefined, fail_open => true},
            ores_otel => #{enabled => true, service_name => ServiceName, exporter_endpoint => undefined, propagators => [<<"tracecontext">>, <<"baggage">>]}
        }
    }.

validate_config(Config) when is_map(Config) ->
    Settings = maps:get(settings, Config, #{}),
    Rate = maps:get(rate_limit, Settings, #{}),
    Fault = maps:get(fault_injection, Settings, #{}),
    TestBypass = maps:get(test_auth_bypass, Settings, #{}),
    Tls = maps:get(tls, Settings, #{}),
    Integrations = maps:get(integrations, Config, #{}),
    SharedAuth = maps:get(shared_auth, Integrations, #{}),
    Issues0 = [],
    Issues1 = add_issue(maps:get(contract_version, Config, undefined) =/= ?CONTRACT_VERSION, <<"/contractVersion">>, <<"unsupported_version">>, <<"expected 1.0.0">>, Issues0),
    Issues2 = add_issue(not positive_integer(maps:get(timeout_ms, Settings, 0)), <<"/settings/timeoutMs">>, <<"range">>, <<"timeout must be positive">>, Issues1),
    Issues3 = add_issue(not positive_integer(maps:get(max_body_bytes, Settings, 0)), <<"/settings/maxBodyBytes">>, <<"range">>, <<"body limit must be positive">>, Issues2),
    InvalidRate = maps:get(enabled, Rate, false) andalso (not positive_integer(maps:get(capacity, Rate, 0)) orelse not positive_number(maps:get(refill_per_second, Rate, 0))),
    Issues4 = add_issue(InvalidRate, <<"/settings/rateLimit">>, <<"invalid_rate_limit">>, <<"enabled token bucket requires positive capacity and refill">>, Issues3),
    InvalidFault = not rate(maps:get(error_rate, Fault, -1)) orelse not rate(maps:get(drop_rate, Fault, -1)),
    Issues5 = add_issue(InvalidFault, <<"/settings/faultInjection">>, <<"range">>, <<"fault rates must be within 0..=1">>, Issues4),
    Production = maps:get(environment, Config, development) =:= production,
    Issues6 = add_issue(Production andalso maps:get(enabled, Fault, false), <<"/settings/faultInjection/enabled">>, <<"production_forbidden">>, <<"fault injection is forbidden in production">>, Issues5),
    Issues7 = add_issue(Production andalso maps:get(enabled, TestBypass, false), <<"/settings/testAuthBypass/enabled">>, <<"production_forbidden">>, <<"test auth bypass is forbidden in production">>, Issues6),
    Issues8 = add_issue(maps:get(fail_open, SharedAuth, false), <<"/integrations/sharedAuth/failOpen">>, <<"auth_fail_open">>, <<"shared-auth must fail closed">>, Issues7),
    Issues9 = add_issue(maps:get(mode, Tls, disabled) =:= trusted_proxy andalso maps:get(trusted_proxy_cidrs, Tls, []) =:= [], <<"/settings/tls/trustedProxyCidrs">>, <<"trusted_proxy_required">>, <<"trusted-proxy mode requires explicit CIDRs">>, Issues8),
    Required = maps:get(required_capabilities, Config, []),
    lists:reverse(lists:foldl(fun(Capability, Acc) -> add_issue(not lists:member(Capability, capabilities()), <<"/requiredCapabilities">>, <<"unknown_capability">>, Capability, Acc) end, Issues9, Required));
validate_config(_) -> [#{path => <<"/">>, code => <<"type">>, message => <<"configuration must be a map">>}].

positive_integer(Value) -> is_integer(Value) andalso Value > 0.
positive_number(Value) -> (is_integer(Value) orelse is_float(Value)) andalso Value > 0.
rate(Value) -> (is_integer(Value) orelse is_float(Value)) andalso Value >= 0 andalso Value =< 1.
add_issue(false, _Path, _Code, _Message, Issues) -> Issues;
add_issue(true, Path, Code, Message, Issues) -> [#{path => Path, code => Code, message => Message} | Issues].

default_hooks() -> #{
    authenticate => fun(_Request, _Context) -> {ok, #{user_id => undefined, tenant_id => undefined, baggage => #{}}} end,
    resolve_test_identity => fun(_Request, _Context) -> {error, not_configured} end,
    authorize_ip => fun(_Request, _Context) -> true end,
    rate_limit => fun(Key, Capacity, Refill) -> ores_middleware_rate_limiter:allow(Key, Capacity, Refill) end,
    idempotency_get => fun(Key) -> ores_middleware_idempotency:get(Key) end,
    idempotency_put => fun(Key, Response, Ttl) -> ores_middleware_idempotency:put(Key, Response, Ttl) end,
    trusted_proxy => fun(_Request, _Cidrs) -> false end,
    telemetry_started => fun(Context, Request) -> logger:info("request started", [], #{request_id => maps:get(request_id, Context), trace_id => maps:get(trace_id, Context), method => maps:get(method, Request), path => maps:get(path, Request)}) end,
    telemetry_finished => fun(Context, Request, Response, Duration) -> logger:info("request finished", [], #{request_id => maps:get(request_id, Context), trace_id => maps:get(trace_id, Context), method => maps:get(method, Request), path => maps:get(path, Request), status => maps:get(status, Response), duration_ms => Duration}) end,
    sync_observe => fun(_Context, _Request, _Response, _Duration) -> ok end,
    schema_capture => fun(_Request, _Response) -> ok end
}.

create_middleware(Config, Hooks0) ->
    case validate_config(Config) of
        [] ->
            Hooks = maps:merge(default_hooks(), Hooks0),
            {ok, fun(Request, Next) -> run(Config, Hooks, Request, Next) end};
        Issues -> {error, Issues}
    end.

run(Config, Hooks, Request, Next) when is_function(Next, 1) ->
    case prepare(Config, Hooks, Request) of
        {error, Response} -> Response;
        {cached, Response} -> Response;
        {ok, Context, IdempotencyKey} ->
            Started = erlang:monotonic_time(millisecond),
            (maps:get(telemetry_started, Hooks))(Context, Request),
            Response0 = run_handler(Next, Request, Context, maps:get(timeout_ms, maps:get(settings, Config))),
            finish(Config, Hooks, Context, Request, Response0, #{started => Started, idempotency_key => IdempotencyKey})
    end.

prepare(Config, Hooks, Request) ->
    Settings = maps:get(settings, Config),
    Headers = maps:get(headers, Request, #{}),
    BodySize = maps:get(body_size, Request, 0),
    case BodySize > maps:get(max_body_bytes, Settings) of
        true -> {error, problem(413, <<"payload_too_large">>, <<"request body exceeds configured limit">>)};
        false -> prepare_accept(Config, Hooks, Request, Headers)
    end.

prepare_accept(Config, Hooks, Request, Headers) ->
    Settings = maps:get(settings, Config),
    Accept = maps:get(<<"accept">>, Headers, <<"*/*">>),
    Supported = maps:get(content_representations, Settings),
    case accepts(Accept, Supported) of
        false -> {error, problem(406, <<"not_acceptable">>, <<"no supported representation was requested">>)};
        true -> prepare_transport(Config, Hooks, Request, Headers)
    end.

prepare_transport(Config, Hooks, Request, Headers) ->
    Settings = maps:get(settings, Config),
    Tls = maps:get(tls, Settings),
    Trusted = (maps:get(trusted_proxy, Hooks))(Request, maps:get(trusted_proxy_cidrs, Tls)),
    Forwarded = maps:get(<<"x-forwarded-proto">>, Headers, undefined),
    Scheme = maps:get(scheme, Request, <<"http">>),
    case maps:get(strict_forwarded_headers, Tls) andalso Forwarded =/= undefined andalso not Trusted of
        true -> {error, problem(400, <<"untrusted_forwarded_header">>, <<"forwarded transport headers came from an untrusted peer">>)};
        false ->
            EffectiveHttps = Scheme =:= <<"https">> orelse (Trusted andalso Forwarded =:= <<"https">>),
            case maps:get(require_https, Tls) andalso not EffectiveHttps of
                true -> {error, problem(426, <<"https_required">>, <<"HTTPS is required">>)};
                false -> prepare_context(Config, Hooks, Request, Headers)
            end
    end.

prepare_context(Config, Hooks, Request, Headers) ->
    Settings = maps:get(settings, Config),
    Now = erlang:system_time(millisecond),
    RequestId = token_or_new(maps:get(maps:get(request_id_header, Settings), Headers, undefined)),
    TraceId = trace_or_new(maps:get(maps:get(trace_header, Settings), Headers, undefined)),
    Context0 = #{request_id => RequestId, trace_id => TraceId, tenant_id => undefined, user_id => undefined, locale => maps:get(<<"accept-language">>, Headers, undefined), started_at_unix_ms => Now, deadline_unix_ms => Now + maps:get(timeout_ms, Settings), baggage => #{}},
    case (maps:get(authorize_ip, Hooks))(Request, Context0) of
        false -> {error, problem(403, <<"ip_policy_denied">>, <<"request source is not permitted">>)};
        true -> prepare_auth(Config, Hooks, Request, Context0, Headers)
    end.

prepare_auth(Config, Hooks, Request, Context0, Headers) ->
    Settings = maps:get(settings, Config),
    BypassPolicy = maps:get(test_auth_bypass, Settings),
    Bypass = maps:get(enabled, BypassPolicy) andalso maps:get(maps:get(header_name, BypassPolicy), Headers, undefined) =:= <<"true">>,
    Environment = maps:get(environment, Config),
    Decision = case Bypass of
        true when Environment =:= test; Environment =:= staging -> (maps:get(resolve_test_identity, Hooks))(Request, Context0);
        true -> {error, production_forbidden};
        false -> (maps:get(authenticate, Hooks))(Request, Context0)
    end,
    case Decision of
        {error, _Reason} -> {error, problem(401, <<"authentication_failed">>, <<"authentication failed">>)};
        {ok, Auth} ->
            Context = Context0#{user_id => maps:get(user_id, Auth, undefined), tenant_id => maps:get(tenant_id, Auth, undefined), baggage => maps:get(baggage, Auth, #{})},
            SharedAuth = maps:get(shared_auth, maps:get(integrations, Config)),
            case maps:get(mode, SharedAuth) =/= disabled andalso maps:get(user_id, Context) =:= undefined of
                true -> {error, problem(401, <<"authentication_required">>, <<"shared-auth did not establish a user">>)};
                false -> prepare_rate(Config, Hooks, Request, Context)
            end
    end.

prepare_rate(Config, Hooks, Request, Context) ->
    Settings = maps:get(settings, Config),
    Policy = maps:get(rate_limit, Settings),
    Key = iolist_to_binary(lists:join(<<":">>, [value(maps:get(tenant_id, Context)), value(maps:get(user_id, Context)), value(maps:get(remote_ip, Request, undefined)), maps:get(path, Request)])),
    case maps:get(enabled, Policy) andalso not (maps:get(rate_limit, Hooks))(Key, maps:get(capacity, Policy), maps:get(refill_per_second, Policy)) of
        true -> {error, problem(429, <<"rate_limited">>, <<"rate limit exceeded">>)};
        false -> prepare_fault(Config, Hooks, Request, Context)
    end.

prepare_fault(Config, Hooks, Request, Context) ->
    Settings = maps:get(settings, Config),
    Policy = maps:get(fault_injection, Settings),
    case maps:get(enabled, Policy) of
        true ->
            timer:sleep(maps:get(latency_ms, Policy)),
            case rand:uniform() < maps:get(drop_rate, Policy) of
                true -> {error, problem(503, <<"fault_drop">>, <<"injected transport drop">>)};
                false -> case rand:uniform() < maps:get(error_rate, Policy) of
                    true -> {error, problem(500, <<"fault_error">>, <<"injected middleware error">>)};
                    false -> prepare_idempotency(Config, Hooks, Request, Context)
                end
            end;
        false -> prepare_idempotency(Config, Hooks, Request, Context)
    end.

prepare_idempotency(Config, Hooks, Request, Context) ->
    Settings = maps:get(settings, Config),
    Policy = maps:get(idempotency, Settings),
    Headers = maps:get(headers, Request, #{}),
    Method = maps:get(method, Request),
    HeaderValue = maps:get(maps:get(header_name, Policy), Headers, undefined),
    case maps:get(enabled, Policy) andalso lists:member(Method, maps:get(required_methods, Policy)) andalso HeaderValue =/= undefined of
        false -> ores_middleware_context:put(Context), {ok, Context, undefined};
        true ->
            Key = iolist_to_binary([Method, <<":">>, maps:get(path, Request), <<":">>, HeaderValue]),
            case (maps:get(idempotency_get, Hooks))(Key) of
                {ok, Response} -> {cached, Response};
                _ -> ores_middleware_context:put(Context), {ok, Context, Key}
            end
    end.

run_handler(Next, Request, Context, Timeout) ->
    Parent = self(),
    Tag = make_ref(),
    {Pid, Monitor} = spawn_monitor(fun() ->
        Result = try {ok, ores_middleware_context:run(Context, fun() -> Next(Request) end)} catch _:_ -> {error, handler_failed} end,
        Parent ! {Tag, Result}
    end),
    receive
        {Tag, {ok, Response}} -> demonitor(Monitor, [flush]), Response;
        {Tag, {error, _}} -> demonitor(Monitor, [flush]), problem(500, <<"internal_error">>, <<"request handler failed">>);
        {'DOWN', Monitor, process, Pid, _Reason} -> problem(500, <<"internal_error">>, <<"request handler failed">>)
    after Timeout ->
        exit(Pid, kill),
        receive {'DOWN', Monitor, process, Pid, _} -> ok after 0 -> ok end,
        problem(504, <<"deadline_exceeded">>, <<"request deadline exceeded">>)
    end.

finish(Config, Hooks, Context, Request, Response0, Meta) ->
    Settings = maps:get(settings, Config),
    Response1 = attach_etag(Request, Response0),
    Response2 = attach_security(Settings, Context, Response1),
    Response3 = maybe_compress(Settings, Request, Response2),
    Duration = erlang:monotonic_time(millisecond) - maps:get(started, Meta),
    (maps:get(schema_capture, Hooks))(Request, Response3),
    OptoSync = maps:get(opto_sync, maps:get(integrations, Config)),
    Response4 = case (maps:get(sync_observe, Hooks))(Context, Request, Response3, Duration) of
        ok -> Response3;
        {error, _} -> case maps:get(fail_open, OptoSync) of true -> Response3; false -> problem(503, <<"sync_observer_failed">>, <<"opto-sync observation failed">>) end
    end,
    IdempotencyKey = maps:get(idempotency_key, Meta, undefined),
    Status = maps:get(status, Response4),
    case IdempotencyKey =/= undefined andalso Status >= 200 andalso Status < 300 of
        true -> Policy = maps:get(idempotency, Settings), (maps:get(idempotency_put, Hooks))(IdempotencyKey, Response4, maps:get(ttl_seconds, Policy));
        false -> ok
    end,
    (maps:get(telemetry_finished, Hooks))(Context, Request, Response4, Duration),
    Response4.

attach_etag(#{method := <<"GET">>, headers := RequestHeaders}, #{status := 200, body := Body, headers := Headers} = Response) when is_binary(Body) ->
    Etag = <<$\", (binary:encode_hex(crypto:hash(sha256, Body), lowercase))/binary, $\">>,
    case maps:get(<<"if-none-match">>, RequestHeaders, undefined) of
        Etag -> Response#{status => 304, body => <<>>, headers => Headers#{<<"etag">> => Etag}};
        _ -> Response#{headers => Headers#{<<"etag">> => Etag}}
    end;
attach_etag(_Request, Response) -> Response.

attach_security(Settings, Context, #{headers := Headers0} = Response) ->
    Security = maps:get(security_headers, Settings),
    Headers1 = Headers0#{maps:get(request_id_header, Settings) => maps:get(request_id, Context), <<"traceparent">> => <<"00-", (maps:get(trace_id, Context))/binary, "-0000000000000000-01">>},
    Headers2 = case maps:get(enabled, Security) of
        true -> Headers1#{
            <<"x-content-type-options">> => <<"nosniff">>,
            <<"x-frame-options">> => maps:get(frame_options, Security),
            <<"referrer-policy">> => <<"strict-origin-when-cross-origin">>,
            <<"strict-transport-security">> => iolist_to_binary(io_lib:format("max-age=~B; includeSubDomains", [maps:get(hsts_max_age_seconds, Security)])),
            <<"content-security-policy">> => maps:get(content_security_policy, Security)
        };
        false -> Headers1
    end,
    Response#{headers => Headers2}.

maybe_compress(Settings, #{headers := RequestHeaders}, #{body := Body, headers := Headers} = Response) when is_binary(Body) ->
    Policy = maps:get(compression, Settings),
    AcceptEncoding = maps:get(<<"accept-encoding">>, RequestHeaders, <<>>),
    case maps:get(enabled, Policy) andalso byte_size(Body) >= maps:get(minimum_bytes, Policy) andalso binary:match(AcceptEncoding, <<"gzip">>) =/= nomatch andalso not maps:is_key(<<"content-encoding">>, Headers) of
        true -> Response#{body => zlib:gzip(Body), headers => Headers#{<<"content-encoding">> => <<"gzip">>, <<"vary">> => <<"accept-encoding">>}};
        false -> Response
    end;
maybe_compress(_Settings, _Request, Response) -> Response.

problem(Status, Code, Detail) -> #{status => Status, headers => #{<<"content-type">> => <<"application/problem+json">>}, body => iolist_to_binary(json:encode(#{type => <<"urn:ores:middleware:", Code/binary>>, title => Code, status => Status, detail => Detail}))}.

accepts(<<>>, _Supported) -> true;
accepts(<<"*/*">>, _Supported) -> true;
accepts(Accept, Supported) -> lists:any(fun(Value) -> binary:match(Accept, Value) =/= nomatch end, Supported).

token_or_new(Value) when is_binary(Value), byte_size(Value) > 0, byte_size(Value) =< 128 -> Value;
token_or_new(_) -> new_id().
trace_or_new(Value) when is_binary(Value) -> case binary:split(Value, <<"-">>, [global]) of [_, Trace | _] when byte_size(Trace) =:= 32 -> string:lowercase(Trace); _ -> new_id() end;
trace_or_new(_) -> new_id().
new_id() -> binary:encode_hex(crypto:strong_rand_bytes(16), lowercase).
value(undefined) -> <<"_">>;
value(Value) when is_binary(Value) -> Value;
value(Value) -> iolist_to_binary(io_lib:format("~tp", [Value])).

run_with_context(Context, Fun) -> ores_middleware_context:run(Context, Fun).
current_context() -> ores_middleware_context:current().
decode_json(Binary) when is_binary(Binary) -> json:decode(Binary).
tls_options(CertFile, KeyFile) -> [{versions, ['tlsv1.3']}, {certfile, CertFile}, {keyfile, KeyFile}, {honor_cipher_order, true}, {secure_renegotiate, true}].

descriptor() -> #{
    <<"contractVersion">> => ?CONTRACT_VERSION,
    <<"language">> => <<"erlang">>,
    <<"runtime">> => <<"erlang-otp">>,
    <<"packageName">> => <<"ores_middleware">>,
    <<"frameworkAdapters">> => [<<"cowboy">>, <<"ranch">>, <<"elli">>, <<"otp">>],
    <<"capabilities">> => capabilities(),
    <<"operationSymbols">> => #{
        <<"descriptor">> => <<"ores_middleware:descriptor/0">>,
        <<"defaultConfig">> => <<"ores_middleware:default_config/1">>,
        <<"validateConfig">> => <<"ores_middleware:validate_config/1">>,
        <<"createMiddleware">> => <<"ores_middleware:create_middleware/2">>,
        <<"runWithContext">> => <<"ores_middleware:run_with_context/2">>,
        <<"currentContext">> => <<"ores_middleware:current_context/0">>,
        <<"capabilities">> => <<"ores_middleware:capabilities/0">>
    }
}.
