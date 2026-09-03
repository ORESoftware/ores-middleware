set shell := ["bash", "-euo", "pipefail", "-c"]

contracts:
    npm install
    npm run contracts:compile
    npm run contracts:check
    python3 -m unittest scripts/test_contract_parity.py scripts/test_schema_convergence.py -v
    python3 scripts/check_zpkg.py

rust:
    cargo test --manifest-path src/rust/Cargo.toml --all-features
    cargo test --manifest-path src/rust/docs-serving/Cargo.toml
    python3 scripts/build_targets.py --languages rust
    mkdir -p target/descriptors
    cargo run --quiet --manifest-path src/rust/Cargo.toml --bin contractcheck > target/descriptors/rust.json

ts:
    npm --prefix src/ts install
    npm --prefix src/ts run build
    npm --prefix src/ts test
    npm --prefix src/ts/docs-serving test
    python3 scripts/build_targets.py --languages ts
    mkdir -p target/descriptors
    node src/ts/dist/contractcheck.js > target/descriptors/ts.json

golang:
    cd src/golang && go test ./...
    python3 scripts/build_targets.py --languages golang
    mkdir -p target/descriptors
    cd src/golang && go run ./cmd/contractcheck > ../../target/descriptors/golang.json

gleam:
    cd src/gleam && gleam format --check src test && gleam test
    python3 scripts/build_targets.py --languages gleam
    mkdir -p target/descriptors
    cd src/gleam && gleam run -m ores_middleware_contractcheck > ../../target/descriptors/gleam.json

elixir:
    cd src/elixir && mix deps.get && mix test
    python3 scripts/build_targets.py --languages elixir
    mkdir -p target/descriptors
    cd src/elixir && mix ores.contractcheck > ../../target/descriptors/elixir.json

erlang:
    cd src/erlang && rebar3 eunit
    python3 scripts/build_targets.py --languages erlang
    mkdir -p target/descriptors
    cd src/erlang && escript escript/contractcheck.escript > ../../target/descriptors/erlang.json

verify: contracts rust ts golang gleam elixir erlang
    npm run descriptors:check
    python3 scripts/audit.py --receipt target/audit/receipt.json
