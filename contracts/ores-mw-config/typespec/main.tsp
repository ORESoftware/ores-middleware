/**
 * Normalized .ores-mw.toml orchestration contract.
 *
 * This TypeSpec file and the sibling Draft 2020-12 schema are independent,
 * human-authored peer authorities. The TOML manifest is normalized into this
 * JSON shape; neither authority is generated from the other.
 */
import "@typespec/json-schema";

namespace Ores.Middleware.Config;

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
  targets: MiddlewareTarget[];
}
