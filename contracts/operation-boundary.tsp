import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace Ores.Middleware.OperationBoundary;

enum OperationTransport {
  http,
  lambda,
  tcp,
  websocket,
}

enum OperationScope {
  request,
  connection,
  message,
  callback,
}

@minLength(1)
@maxLength(128)
@pattern("^[A-Za-z0-9_.:-]+$")
scalar OperationName extends string;

model OperationDescriptor {
  transport: OperationTransport;
  scope: OperationScope;
  name: OperationName;
}
