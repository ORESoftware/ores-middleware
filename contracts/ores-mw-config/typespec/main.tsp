namespace Ores.Middleware.Config;

model Flags2EnvConfig extends Record<never> {
  contract: string;
  requireAudit: boolean;
  precedence: "argv-over-env";
}

model EnvBinding extends Record<never> {
  name: string;
  key: string;
  kind: "string" | "bool" | "integer" | "double" | "json" | "url";
  required: boolean;
  secret: boolean;
  defaultValue?: string;
  description?: string;
}

model MiddlewareTarget extends Record<never> {
  name: string;
  role: "server" | "client";
  roots: string[];
  enabled: boolean;
  middleware: "stack" | "propagation-only" | "disabled";
  stackConfig?: string;
  propagateHeaders?: string[];
}

model MiddlewareManifest extends Record<never> {
  schemaVersion: 1;
  repositoryMode: "server-only" | "client-only" | "hybrid";
  allowOverlappingRoots: boolean;
  defaultTarget?: string;
  flags2env?: Flags2EnvConfig;
  env?: EnvBinding[];
  targets: MiddlewareTarget[];
}
