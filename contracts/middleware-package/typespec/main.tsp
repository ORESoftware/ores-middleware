import "@typespec/json-schema";

namespace Ores.Middleware.Package;

@minLength(1)
scalar NonEmptyString extends string;

@pattern("^[0-9a-f]{64}$")
scalar Sha256Hex extends string;

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareArtifactDescriptor {
  id: NonEmptyString;
  language: NonEmptyString;
  kind: "javascript_module" | "web_assembly_module" | "native_library" | "beam_module" | "process_rpc";
  profile: "portable_javascript" | "portable_wasm" | "native" | "isolated_process";
  provider: "generic" | "cloudflare" | "caddy" | "nginx" | "haproxy";
  path: NonEmptyString;
  semantic_contract_sha256: Sha256Hex;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewarePackageManifest {
  spec_version: 1;
  name: NonEmptyString;
  version: NonEmptyString;
  semantic_contract_sha256: Sha256Hex;
  phases: NonEmptyString[];
  capabilities: NonEmptyString[];
  @minItems(1)
  artifacts: MiddlewareArtifactDescriptor[];
}
