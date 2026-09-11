/**
 * Normalized .ores-mw.toml orchestration contract.
 *
 * This TypeSpec file and the sibling Draft 2020-12 schema are independent,
 * human-authored peer authorities. The TOML manifest is normalized into this
 * JSON shape; neither authority is generated from the other.
 *
 * Runtime environment bindings are names/metadata only. `.cli-flags.toml`
 * remains the sole argv contract and is interpreted by flags-2-env.
 */
import "@typespec/json-schema";

namespace Ores.Middleware.Config;

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model Flags2EnvConfig {
  contract: string;
  requireAudit: boolean;
  precedence: "argv-over-env";
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model EnvBinding {
  name: string;
  key: string;
  kind: "string" | "bool" | "integer" | "double" | "json" | "url";
  required: boolean;
  secret: boolean;
  defaultValue?: string;
  description?: string;
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareTarget {
  name: string;
  role: "server" | "client";
  roots: string[];
  enabled: boolean;
  middleware: "stack" | "propagation-only" | "disabled";
  stackConfig?: string;
  propagateHeaders?: string[];
}

@TypeSpec.JsonSchema.extension("additionalProperties", false)
model MiddlewareManifest {
  schemaVersion: 1;
  repositoryMode: "server-only" | "client-only" | "hybrid";
  allowOverlappingRoots: boolean;
  defaultTarget?: string;
  flags2env?: Flags2EnvConfig;
  env?: EnvBinding[];
  targets: MiddlewareTarget[];
}
