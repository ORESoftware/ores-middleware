-module(operation_boundary_test_ffi).

-export([raise/0]).

raise() -> erlang:error(handler_failed).
