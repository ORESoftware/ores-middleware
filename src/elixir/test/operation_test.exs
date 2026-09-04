defmodule OresMiddleware.OperationTest do
  use ExUnit.Case, async: true

  alias OresMiddleware.{Context, Operation}

  defp context(id) do
    %{
      request_id: "request-#{id}",
      trace_id: String.pad_leading(Integer.to_string(id, 16), 32, "0"),
      tenant_id: "tenant-#{id}",
      user_id: "user-#{id}",
      locale: nil,
      started_at_unix_ms: 0,
      deadline_unix_ms: nil,
      baggage: %{}
    }
  end

  test "reporter failure cannot replace a redacted operation failure" do
    outer = context(1)
    inner = context(2)
    Context.put(outer)
    on_exit(fn -> Context.clear() end)

    assert {:error, failure} =
             Operation.run(
               inner,
               %{transport: :http, scope: :request, name: "orders.read"},
               fn -> raise "private body token" end,
               fn _failure -> raise "reporter unavailable" end
             )

    assert failure.kind == :error
    assert failure.code == "operation_failed"
    assert failure.operation == "orders.read"
    assert failure.request_id == inner.request_id
    assert failure.trace_id == inner.trace_id
    refute Map.has_key?(failure, :message)
    refute Map.has_key?(failure, :stack)
    refute Map.has_key?(failure, :cause)
    refute inspect(failure) =~ "private body token"
    assert Context.current() == outer
  end

  test "expired deadline prevents operation invocation and restores context" do
    parent = self()

    assert {:error, failure} =
             Operation.run(
               context(3),
               %{
                 transport: :tcp,
                 scope: :callback,
                 name: "queue.consume",
                 deadline_unix_ms: System.system_time(:millisecond) - 1
               },
               fn -> send(parent, :operation_invoked) end,
               fn _failure -> :ok end
             )

    assert failure.kind == :deadline_exceeded
    assert failure.code == "operation_deadline_exceeded"
    refute_received :operation_invoked
    assert Context.current() == nil
  end

  test "malformed operation names normalize without changing success" do
    assert {:ok, :accepted} =
             Operation.run(
               context(4),
               %{transport: :websocket, scope: :message, name: "customer/secret"},
               fn -> :accepted end
             )

    assert Context.current() == nil
  end
end
