use std::fs;

const PRODUCTION_SOURCES: &[&str] = &[
    "src/ts/src/index.ts",
    "src/ts/src/adapters.ts",
    "src/golang/middleware.go",
    "src/rust/src/pipeline.rs",
    "src/gleam/src/ores_middleware.gleam",
    "src/elixir/lib/ores_middleware/plug.ex",
    "src/erlang/src/ores_middleware.erl",
    "src/erlang/src/ores_middleware_cowboy.erl",
];

fn main() {
    let mut failures = Vec::new();
    for path in PRODUCTION_SOURCES {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        for forbidden in [
            "-0000000000000000-01",
            "inboundSpanId",
            "validSpanId(context.spanId) ??",
        ] {
            if content.contains(forbidden) {
                failures.push(format!(
                    "{path}: forbidden response trace-context pattern {forbidden:?}"
                ));
            }
        }
    }

    for (path, required) in [
        ("src/ts/src/index.ts", "function validTraceparent"),
        ("src/golang/middleware.go", "func validTraceparent"),
        (
            "src/gleam/src/ores_middleware.gleam",
            "fn normalize_traceparent",
        ),
        (
            "src/elixir/lib/ores_middleware/plug.ex",
            "defp normalize_traceparent",
        ),
        (
            "src/erlang/src/ores_middleware.erl",
            "normalize_traceparent(Value)",
        ),
    ] {
        let content = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("failed to read {path}: {error}"));
        if !content.contains(required) {
            failures.push(format!(
                "{path}: missing required safety marker {required:?}"
            ));
        }
    }

    if !failures.is_empty() {
        eprintln!("response trace-context safety audit failed:");
        for failure in failures {
            eprintln!("- {failure}");
        }
        std::process::exit(1);
    }

    println!("response trace-context safety audit passed");
}
