defmodule OresMiddleware.Generic do
  @moduledoc """
  Runtime-agnostic provider and middleware composition primitives.

  The consumer owns request/response/context shapes, provider dependencies,
  failure values, and middleware ordering. This module does not import or select
  an HTTP framework or identity provider.
  """

  @type provider(input, output) :: (input -> output)
  @type contextual_input(request, context) :: %{request: request, context: context}
  @type handler(request, response) :: (request -> response)
  @type middleware(request, response) :: (handler(request, response) -> handler(request, response))
  @type contextual_handler(request, context, response) :: (request, context -> response)
  @type contextual_middleware(request, context, response) ::
          (contextual_handler(request, context, response) ->
             contextual_handler(request, context, response))
  @type named_middleware(request, response) :: %{
          required(:name) => term(),
          required(:middleware) => middleware(request, response)
        }

  @spec provider_from(provider(input, output)) :: provider(input, output) when input: var, output: var
  def provider_from(provider) when is_function(provider, 1), do: provider

  @spec contextual_provider_from((request, context -> output)) ::
          provider(contextual_input(request, context), output)
        when request: var, context: var, output: var
  def contextual_provider_from(provider) when is_function(provider, 2) do
    fn %{request: request, context: context} -> provider.(request, context) end
  end

  @spec compose(handler(request, response), [middleware(request, response)]) ::
          handler(request, response)
        when request: var, response: var
  def compose(handler, middleware) when is_function(handler, 1) and is_list(middleware) do
    Enum.reduce(Enum.reverse(middleware), handler, fn stage, next ->
      unless is_function(stage, 1),
        do: raise(ArgumentError, "middleware stage must be a unary function")

      stage.(next)
    end)
  end

  @spec compose_named(handler(request, response), [named_middleware(request, response)]) ::
          handler(request, response)
        when request: var, response: var
  def compose_named(handler, stages) when is_list(stages) do
    middleware =
      Enum.map(stages, fn
        %{name: _, middleware: stage} when is_function(stage, 1) -> stage
        _ -> raise ArgumentError, "named middleware stage must contain :name and unary :middleware"
      end)

    compose(handler, middleware)
  end

  @spec compose_contextual(
          contextual_handler(request, context, response),
          [contextual_middleware(request, context, response)]
        ) :: contextual_handler(request, context, response)
        when request: var, context: var, response: var
  def compose_contextual(handler, middleware) when is_function(handler, 2) and is_list(middleware) do
    Enum.reduce(Enum.reverse(middleware), handler, fn stage, next ->
      unless is_function(stage, 1),
        do: raise(ArgumentError, "contextual middleware stage must be unary")

      stage.(next)
    end)
  end

  @spec compose_named_contextual(
          contextual_handler(request, context, response),
          [map()]
        ) :: contextual_handler(request, context, response)
        when request: var, context: var, response: var
  def compose_named_contextual(handler, stages) when is_list(stages) do
    middleware =
      Enum.map(stages, fn
        %{name: _, middleware: stage} when is_function(stage, 1) -> stage
        _ -> raise ArgumentError, "named contextual stage must contain :name and unary :middleware"
      end)

    compose_contextual(handler, middleware)
  end
end
