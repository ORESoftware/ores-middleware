import "@typespec/json-schema";

namespace Ores.Middleware.Package;

alias Sha256Hex = string;

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareArtifactDescriptor {
  id: string;
  language: string;
  kind: "javascript_module" | "web_assembly_module" | "native_library" | "beam_module" | "process_rpc";
  profile: "portable_javascript" | "portable_wasm" | "native" | "isolated_process";
  provider: "generic" | "cloudflare" | "caddy" | "nginx" | "haproxy";
  path: string;
  semantic_contract_sha256: Sha256Hex;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewarePackageManifest {
  spec_version: 1;
  name: string;
  version: string;
  semantic_contract_sha256: Sha256Hex;
  phases: string[];
  capabilities: string[];
  artifacts: MiddlewareArtifactDescriptor[];
}
