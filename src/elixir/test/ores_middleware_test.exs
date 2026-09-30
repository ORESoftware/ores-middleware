defmodule OresMiddlewareTest do
  use ExUnit.Case, async: false

  import Plug.Conn, only: [put_req_header: 3]
  import Plug.Test, only: [conn: 2]

  test "descriptor exports the standard semantic operations" do
    descriptor = OresMiddleware.descriptor()
    assert length(descriptor.capabilities) == 23
    assert map_size(descriptor.operationSymbols) == 7
  end

  test "production rejects test-only middleware" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:environment], :production)
    config = put_in(config, [:settings, :faultInjection, :enabled], true)
    config = put_in(config, [:settings, :testAuthBypass, :enabled], true)
    issues = OresMiddleware.validate_config(config)
    assert Enum.any?(issues, &String.contains?(&1.path, "faultInjection"))
    assert Enum.any?(issues, &String.contains?(&1.path, "testAuthBypass"))
  end

  test "Plug adapter establishes context and correlation headers" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    config = put_in(config, [:settings, :rateLimit, :enabled], false)
    stack = OresMiddleware.Stack.new!(config)
    conn = conn(:get, "/v1") |> put_req_header("accept", "application/json")

    conn =
      OresMiddleware.Plug.wrap(stack, conn, fn conn ->
        Plug.Conn.resp(conn, 200, Jason.encode!(%{ok: true}))
      end)

    assert conn.status == 200
    assert Plug.Conn.get_resp_header(conn, "x-request-id") != []
    assert Plug.Conn.get_resp_header(conn, "traceparent") == []
    assert Plug.Conn.get_resp_header(conn, "x-content-type-options") == ["nosniff"]
  end

  test "request context is process scoped and restored" do
    context = %{
      request_id: "r1",
      trace_id: String.duplicate("a", 32),
      tenant_id: nil,
      user_id: nil
    }

    assert OresMiddleware.current_context() == nil

    assert OresMiddleware.run_with_context(context, fn ->
             OresMiddleware.current_context().request_id
           end) == "r1"

    assert OresMiddleware.current_context() == nil
  end

  test "strict forwarding rejects client identity from an untrusted peer" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    config = put_in(config, [:settings, :rateLimit, :enabled], false)
    stack = OresMiddleware.Stack.new!(config)

    conn =
      conn(:get, "/v1")
      |> Map.put(:remote_ip, {198, 51, 100, 10})
      |> put_req_header("x-forwarded-for", "203.0.113.9")

    conn =
      OresMiddleware.Plug.wrap(stack, conn, fn conn ->
        Plug.Conn.resp(conn, 200, "ok")
      end)

    assert conn.status == 400
  end

  test "trusted proxy rate key uses validated forwarded client identity" do
    config = OresMiddleware.default_config("test")
    config = put_in(config, [:settings, :tls, :requireHttps], false)
    parent = self()

    stack =
      OresMiddleware.Stack.new!(config, %{
        rate_limit: fn key, capacity, refill ->
          send(parent, {:rate_key, key, capacity, refill})
          true
        end
      })

    conn =
      conn(:get, "/v1")
      |> Map.put(:remote_ip, {127, 0, 0, 1})
      |> put_req_header("cf-connecting-ip", "not-an-ip")
      |> put_req_header("x-forwarded-for", "203.0.113.55, 10.0.0.4")

    conn =
      OresMiddleware.Plug.wrap(stack, conn, fn conn ->
        Plug.Conn.resp(conn, 200, "ok")
      end)

    assert conn.status == 200
    assert_receive {:rate_key, "203.0.113.55", 5, 5.0}
  end

  test "local token bucket bounds source IP cardinality" do
    {:ok, pid} = OresMiddleware.TokenBucket.start_link([])

    Enum.each(0..10_000, fn index ->
      assert OresMiddleware.TokenBucket.allow(pid, "ip-#{index}", 1, 0.000001)
    end)

    %{buckets: buckets, order: order} = :sys.get_state(pid)
    assert map_size(buckets) == 10_000
    refute Map.has_key?(buckets, "ip-0")
    assert :queue.len(order) == 10_000
  end
end
