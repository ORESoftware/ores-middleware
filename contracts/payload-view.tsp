import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace Ores.Middleware.Payload;

enum PayloadCodec {
  json,
  messagepack,
  cbor,
  protobuf,
  raw,
}

model PayloadViewV1 {
  schema: "ores.middleware.payload-view/v1";
  codec: PayloadCodec;
  opaque: boolean;
  @minValue(0)
  @maxValue(16777216)
  encoded_bytes: uint32;
  @minValue(0)
  @maxValue(67108864)
  decoded_bytes: uint32;
  transformed: boolean;
}
