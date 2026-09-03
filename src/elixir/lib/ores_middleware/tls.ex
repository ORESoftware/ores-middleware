defmodule OresMiddleware.TLS do
  @moduledoc "Secure TLS termination defaults for Bandit, Cowboy, or custom OTP listeners."
  def options(certfile, keyfile) do
    [versions: [:'tlsv1.3'], certfile: certfile, keyfile: keyfile, honor_cipher_order: true, secure_renegotiate: true]
  end
end
