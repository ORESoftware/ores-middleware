defmodule OresMiddleware.IP do
  @moduledoc false
  use Bitwise

  def in_cidrs?(address, cidrs) when is_tuple(address) and is_list(cidrs) do
    Enum.any?(cidrs, &contains?(&1, address))
  end

  defp contains?(cidr, address) do
    with [network_text, prefix_text] <- String.split(cidr, "/", parts: 2),
         {:ok, network} <- :inet.parse_address(String.to_charlist(network_text)),
         {prefix, ""} <- Integer.parse(prefix_text),
         {address_bits, width} <- tuple_to_integer(address),
         {network_bits, ^width} <- tuple_to_integer(network),
         true <- prefix >= 0 and prefix <= width do
      shift = width - prefix
      address_bits >>> shift == network_bits >>> shift
    else
      _ -> false
    end
  end

  defp tuple_to_integer({a, b, c, d}), do: {a <<< 24 ||| b <<< 16 ||| c <<< 8 ||| d, 32}
  defp tuple_to_integer({a, b, c, d, e, f, g, h}) do
    {[a, b, c, d, e, f, g, h] |> Enum.reduce(0, fn part, acc -> acc <<< 16 ||| part end), 128}
  end
  defp tuple_to_integer(_), do: {0, 0}
end
