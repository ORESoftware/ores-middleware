import "@typespec/json-schema";
using TypeSpec.JsonSchema;

namespace Ores.Middleware.ConfigFingerprint;

@minLength(64)
@maxLength(64)
@pattern("^[0-9a-f]{64}$")
scalar Sha256 extends string;

model ConfigFingerprintV1 {
  schema: "ores.middleware.config-fingerprint/v1";
  fingerprint_sha256: Sha256;
  @maxValue(100000)
  public_field_count: uint32;
  @maxValue(100000)
  secret_binding_count: uint32;
  raw_secret_material_included: false;
}
