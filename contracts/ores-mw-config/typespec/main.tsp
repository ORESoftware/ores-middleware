namespace Ores.Middleware.Config;

model Flags2EnvConfig {
  contract: string;
  requireAudit: boolean;
  precedence: "argv-over-env";
}

model EnvBinding {
  name: string;
  key: string;
  kind: "string" | "bool" | "integer" | "double" | "json" | "url";
  required: boolean;
  secret: boolean;
  defaultValue?: string;
  description?: string;
}

model MiddlewareTarget {
  name: string;
  role: "server" | "client";
  roots: string[];
  enabled: boolean;
  middleware: "stack" | "propagation-only" | "disabled";
  stackConfig?: string;
  propagateHeaders?: string[];
}

model MiddlewareManifest {
  schemaVersion: 1;
  repositoryMode: "server-only" | "client-only" | "hybrid";
  allowOverlappingRoots: boolean;
  defaultTarget?: string;
  flags2env?: Flags2EnvConfig;
  env?: EnvBinding[];
  targets: MiddlewareTarget[];
}
