defmodule Mix.Tasks.Ores.Contractcheck do
  use Mix.Task

  @shortdoc "Prints the runtime adapter descriptor"
  @impl true
  def run(_args) do
    Mix.Task.run("app.start")
    IO.puts(OresMiddleware.descriptor_json())
  end
end
