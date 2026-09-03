defmodule OresMiddleware.OTelAdversarialTest do
  use ExUnit.Case, async: false

  import Plug.Conn, only: [assign: 3, send_resp: 3]
  import Plug.Test, only: [conn: 2]

  alias ORESoftware.NextLoggers
  alias OresMiddleware.{Context, OTel, OTelPlug}

  setup do
    previous_context = Context.current()
    previous_metadata = Logger.metadata()

    Context.clear()
    Logger.reset_metadata([])
    log_restore = NextLoggers.clear_context()

    on_exit(fn ->
      case previous_context do
        nil -> Context.clear()
        value -> Context.put(value)
      end

      Logger.reset_metadata(previous_metadata)
      NextLoggers.restore_context(log_restore)
    end)

    :ok
  end

  test "64 concurrent request processes keep request, user, tenant, and baggage isolated" do
    logger = OTel.new_logger("middleware-adversarial-test", name: "orders")

    results =
      0..63
      |> Task.async_stream(
        fn slot ->
          slot = Integer.to_string(slot)
          context = context(slot)
          request_logger = OTel.request_logger(logger, context)

          result =
            Context.run(context, fn ->
              assert Context.current().request_id == "request-#{slot}"
              assert NextLoggers.current_context().fields["request.id"] == "request-#{slot}"
              assert NextLoggers.current_context().fields["user.id"] == "user-#{slot}"
              assert NextLoggers.current_context().fields["tenant.id"] == "tenant-#{slot}"

              file_record = NextLoggers.info(logger, "file:#{slot}")
              assert {:ok, request_record} = request_logger.warn.("request:#{slot}")
              {file_record, request_record}
            end)

          assert Context.current() == nil
          assert NextLoggers.current_context() == %{}
          {slot, result}
        end,
        max_concurrency: 64,
        ordered: false,
        timeout: 5_000
      )
      |> Enum.map(fn {:ok, value} -> value end)

    assert length(results) == 64

    Enum.each(results, fn {slot, {file_record, request_record}} ->
      verify_record(file_record, slot, "file:#{slot}")
      verify_record(request_record, slot, "request:#{slot}")
    end)

    assert Context.current() == nil
    assert NextLoggers.current_context() == %{}
  end

  test "nested request scopes restore exact portable, Logger, and ores-otel state after an exception" do
    outer_context = context("outer")
    outer_metadata = [outer_marker: true, request_id: "request-outer"]
    outer_log_context = %{trace_id: "outer-trace", fields: %{"scope" => "outer"}}

    Context.put(outer_context)
    Logger.reset_metadata(outer_metadata)
    restore_token = NextLoggers.put_context(outer_log_context)

    try do
      assert_raise RuntimeError, "handler exploded", fn ->
        Context.run(context("inner"), fn ->
          assert Context.current().request_id == "request-inner"
          assert NextLoggers.current_context().fields["request.id"] == "request-inner"
          raise "handler exploded"
        end)
      end

      assert Context.current() == outer_context
      assert Map.new(Logger.metadata()) == Map.new(outer_metadata)
      assert NextLoggers.current_context() == outer_log_context
    after
      NextLoggers.restore_context(restore_token)
    end
  end

  test "OTelPlug lifecycle transport failure is fail-open and restores the exact prior log context" do
    failing_logger =
      OTel.new_logger("middleware-adversarial-test",
        name: "failing-sink",
        transports: [fn _record -> raise "sink unavailable" end]
      )

    outer = %{trace_id: "outer", fields: %{"scope" => "outer"}}
    outer_restore = NextLoggers.put_context(outer)

    try do
      options = OTelPlug.init(logger: failing_logger, lifecycle: true)

      response =
        conn(:get, "/failing-sink")
        |> assign(:ores_middleware_context, context("transport"))
        |> OTelPlug.call(options)
        |> send_resp(204, "")

      assert response.status == 204
      assert response.resp_body == ""
      assert is_map(response.assigns.log)
      assert response.assigns.log == response.assigns.ores_log
      assert NextLoggers.current_context() == outer
    after
      NextLoggers.restore_context(outer_restore)
    end
  end

  test "strict OTelPlug ordering fails explicitly instead of silently creating an uncorrelated logger" do
    logger = OTel.new_logger("middleware-adversarial-test")
    options = OTelPlug.init(logger: logger, strict: true)

    assert_raise ArgumentError, ~r/must run after OresMiddleware.Plug/, fn ->
      conn(:get, "/wrong-order") |> OTelPlug.call(options)
    end

    assert NextLoggers.current_context() == %{}
  end

  test "request logger remains pinned after ambient context changes" do
    logger = OTel.new_logger("middleware-adversarial-test", name: "orders")
    pinned = OTel.request_logger(logger, context("pinned"))

    outer_restore =
      NextLoggers.put_context(%{
        trace_id: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        fields: %{
          "request.id" => "request-other",
          "user.id" => "user-other",
          "tenant.id" => "tenant-other"
        }
      })

    try do
      assert {:ok, record} = pinned.info.("pinned logger")
      verify_record(record, "pinned", "pinned logger")
    after
      NextLoggers.restore_context(outer_restore)
    end
  end

  defp context(slot) do
    %{
      request_id: "request-#{slot}",
      trace_id: String.pad_leading(slot, 32, "a") |> String.slice(-32, 32),
      span_id: "0123456789abcdef",
      tenant_id: "tenant-#{slot}",
      user_id: "user-#{slot}",
      locale: "en-US",
      started_at_unix_ms: 1,
      deadline_unix_ms: 2,
      baggage: %{
        "otel.slot" => slot,
        "authorization" => "must-not-propagate",
        "cookie" => "must-not-propagate"
      }
    }
  end

  defp verify_record(record, slot, message) do
    assert record["message"] == message
    assert record["fields"]["request.id"] == "request-#{slot}"
    assert record["fields"]["user.id"] == "user-#{slot}"
    assert record["fields"]["tenant.id"] == "tenant-#{slot}"
    assert record["fields"]["otel.baggage"] == %{"otel.slot" => slot}
    assert record["traceId"] == context(slot).trace_id
    refute inspect(record) =~ "must-not-propagate"
    refute inspect(record) =~ "authorization"
    refute inspect(record) =~ "cookie"
  end
end
