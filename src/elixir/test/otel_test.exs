defmodule OresMiddleware.OTelTest do
  use ExUnit.Case, async: false

  import Plug.Conn, only: [assign: 3, send_resp: 3]
  import Plug.Test, only: [conn: 2]

  alias ORESoftware.NextLoggers
  alias OresMiddleware.{Context, OTel, OTelPlug}

  setup do
    parent = self()

    logger =
      OTel.new_logger("middleware-test",
        name: "orders",
        id_factory: fn -> "record-1" end,
        clock: fn -> "2026-09-03T00:00:00Z" end,
        transports: [
          fn record ->
            send(parent, {:record, record})
            :ok
          end
        ]
      )

    context = %{
      request_id: "request-42",
      trace_id: "0123456789abcdef0123456789abcdef",
      span_id: "0123456789abcdef",
      tenant_id: "tenant-7",
      user_id: "user-42",
      locale: "en-US",
      started_at_unix_ms: 1,
      deadline_unix_ms: 2,
      baggage: %{
        "otel.vendor" => "allowed",
        "authorization" => "must-not-propagate"
      }
    }

    %{logger: logger, context: context}
  end

  test "request logger closures retain authenticated correlation", %{logger: logger, context: context} do
    request_logger = OTel.request_logger(logger, context)

    assert {:ok, record} = request_logger.warn.("slow dependency")
    assert record["level"] == "WARN"
    assert record["traceId"] == context.trace_id
    assert record["fields"]["request.id"] == "request-42"
    assert record["fields"]["user.id"] == "user-42"
    assert record["fields"]["tenant.id"] == "tenant-7"
    assert record["fields"]["otel.baggage"] == %{"otel.vendor" => "allowed"}
    refute inspect(record) =~ "must-not-propagate"
    assert_receive {:record, ^record}
    assert NextLoggers.current_context() == %{}
  end

  test "ordinary file logger inherits and restores middleware context", %{
    logger: logger,
    context: context
  } do
    assert NextLoggers.current_context() == %{}

    record =
      Context.run(context, fn ->
        NextLoggers.info(logger, "handler reached")
      end)

    assert record["fields"]["request.id"] == "request-42"
    assert record["fields"]["user.id"] == "user-42"
    assert record["fields"]["tenant.id"] == "tenant-7"
    assert_receive {:record, ^record}
    assert NextLoggers.current_context() == %{}
  end

  test "OTelPlug assigns conn.log and restores the exact prior context", %{
    logger: logger,
    context: context
  } do
    outer = %{trace_id: "outer", fields: %{"scope" => "outer"}}
    outer_restore = NextLoggers.put_context(outer)

    try do
      plug_options = OTelPlug.init(logger: logger, lifecycle: false)

      conn =
        conn(:get, "/orders/42")
        |> assign(:ores_middleware_context, context)
        |> OTelPlug.call(plug_options)

      assert conn.assigns.log == conn.assigns.ores_log
      assert is_function(conn.assigns.log.info, 1)
      assert NextLoggers.current_context().fields["request.id"] == "request-42"
      assert {:ok, record} = conn.assigns.log.info.("inside plug")
      assert record["fields"]["user.id"] == "user-42"
      assert_receive {:record, ^record}

      conn = send_resp(conn, 202, "ok")
      assert conn.status == 202
      assert NextLoggers.current_context() == outer
    after
      NextLoggers.restore_context(outer_restore)
    end

    assert NextLoggers.current_context() == %{}
  end
end
