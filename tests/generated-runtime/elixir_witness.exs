defmodule OresGeneratedRuntimeWitness do
  @moduledoc false
  @witness_schema "ores.generated-runtime-witness/v1"
  @model OresMiddleware.Generated.IdempotencyRecord

  def main([fixture_path, generated_path, authority]) do
    Code.require_file(Path.expand(generated_path))
    fixture = fixture_path |> File.read!() |> Jason.decode!()

    cases =
      Enum.map(fixture["cases"], fn test_case ->
        case strict_decode(test_case["value"], fixture) do
          {:ok, record} ->
            %{
              "id" => test_case["id"],
              "accepted" => true,
              "normalized" => normalize(record)
            }

          :error ->
            %{
              "id" => test_case["id"],
              "accepted" => false,
              "normalized" => nil
            }
        end
      end)

    status_acceptance =
      fixture["statuses"]
      |> Enum.concat(["__unknown__"])
      |> Map.new(fn status -> {status, @model.valid_idempotency_status?(status)} end)

    witness = %{
      "schema" => @witness_schema,
      "authority" => authority,
      "language" => "elixir",
      "model" => fixture["model"],
      "wireFields" => reflected_wire_fields(),
      "requiredFields" => fixture["requiredFields"],
      "optionalFields" => fixture["optionalFields"],
      "statuses" => fixture["statuses"],
      "statusAcceptance" => status_acceptance,
      "cases" => cases
    }

    IO.puts(Jason.encode!(witness))
  end

  def main(_arguments) do
    raise ArgumentError,
          "usage: mix run tests/generated-runtime/elixir_witness.exs -- <fixture.json> <generated.ex> <authority>"
  end

  defp strict_decode(value, fixture) when is_map(value) do
    allowed = MapSet.new(fixture["wireFields"])

    with true <- Enum.all?(Map.keys(value), &MapSet.member?(allowed, &1)),
         true <- required_strings?(value, fixture["requiredFields"]),
         true <- optional_fields_valid?(value),
         true <- rfc3339?(value["createdAt"]),
         true <- rfc3339?(value["expiresAt"]),
         true <- @model.valid_idempotency_status?(value["status"]) do
      {:ok,
       struct!(@model,
         created_at: value["createdAt"],
         expires_at: value["expiresAt"],
         id: value["id"],
         idempotency_key: value["idempotencyKey"],
         request_hash: value["requestHash"],
         response_body: Map.get(value, "responseBody"),
         response_status: Map.get(value, "responseStatus"),
         status: value["status"],
         tenant_id: value["tenantId"]
       )}
    else
      _ -> :error
    end
  rescue
    KeyError -> :error
    ArgumentError -> :error
  end

  defp strict_decode(_value, _fixture), do: :error

  defp required_strings?(value, required_fields) do
    Enum.all?(required_fields, fn field ->
      Map.has_key?(value, field) and is_binary(value[field])
    end)
  end

  defp optional_fields_valid?(value) do
    optional_string_valid?(value, "responseBody") and
      optional_int32_valid?(value, "responseStatus")
  end

  defp optional_string_valid?(value, field) do
    not Map.has_key?(value, field) or is_binary(value[field])
  end

  defp optional_int32_valid?(value, field) do
    not Map.has_key?(value, field) or
      (is_integer(value[field]) and value[field] >= -2_147_483_648 and
         value[field] <= 2_147_483_647)
  end

  defp rfc3339?(value) when is_binary(value) do
    match?({:ok, _datetime, _offset}, DateTime.from_iso8601(value))
  end

  defp rfc3339?(_value), do: false

  defp normalize(record) do
    %{
      "createdAt" => record.created_at,
      "expiresAt" => record.expires_at,
      "id" => record.id,
      "idempotencyKey" => record.idempotency_key,
      "requestHash" => record.request_hash,
      "status" => record.status,
      "tenantId" => record.tenant_id
    }
    |> maybe_put("responseBody", record.response_body)
    |> maybe_put("responseStatus", record.response_status)
  end

  defp maybe_put(value, _key, nil), do: value
  defp maybe_put(value, key, item), do: Map.put(value, key, item)

  defp reflected_wire_fields do
    @model.__struct__()
    |> Map.keys()
    |> Enum.reject(&(&1 == :__struct__))
    |> Enum.map(&wire_name/1)
    |> Enum.sort()
  end

  defp wire_name(:created_at), do: "createdAt"
  defp wire_name(:expires_at), do: "expiresAt"
  defp wire_name(:id), do: "id"
  defp wire_name(:idempotency_key), do: "idempotencyKey"
  defp wire_name(:request_hash), do: "requestHash"
  defp wire_name(:response_body), do: "responseBody"
  defp wire_name(:response_status), do: "responseStatus"
  defp wire_name(:status), do: "status"
  defp wire_name(:tenant_id), do: "tenantId"
end

OresGeneratedRuntimeWitness.main(System.argv())
