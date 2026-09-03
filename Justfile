set shell := ["bash", "-euo", "pipefail", "-c"]

contracts:
    npm install
    npm run contracts:compile
    npm run contracts:check

rust:
    mkdir -p target/rust target/descriptors
    cargo test --manifest-path src/rust/Cargo.toml
    cargo run --quiet --manifest-path src/rust/Cargo.toml --bin contractcheck > target/descriptors/rust.json
    cargo build --release --manifest-path src/rust/Cargo.toml --target-dir target/rust

ts:
    mkdir -p target/ts target/descriptors
    npm --prefix src/ts install
    npm --prefix src/ts run build
    npm --prefix src/ts test
    node src/ts/dist/contractcheck.js > target/descriptors/ts.json

golang:
    mkdir -p target/golang target/descriptors
    cd src/golang && go test ./...
    cd src/golang && go run ./cmd/contractcheck > ../../target/descriptors/golang.json
    cd src/golang && go build -o ../../target/golang/contractcheck ./cmd/contractcheck

gleam:
    mkdir -p target/gleam target/descriptors
    cd src/gleam && gleam format --check src test
    cd src/gleam && gleam test
    cd src/gleam && gleam run -m ores_middleware_contractcheck > ../../target/descriptors/gleam.json
    cp -R src/gleam/build/* target/gleam/

elixir:
    mkdir -p target/elixir target/descriptors
    cd src/elixir && mix deps.get
    cd src/elixir && mix test
    cd src/elixir && mix ores.contractcheck > ../../target/descriptors/elixir.json
    cp -R src/elixir/_build/* target/elixir/

erlang:
    mkdir -p target/erlang target/descriptors
    cd src/erlang && rebar3 eunit
    cd src/erlang && escript escript/contractcheck.escript > ../../target/descriptors/erlang.json
    cp -R src/erlang/_build/* target/erlang/

verify: contracts rust ts golang gleam elixir erlang
    npm run verify
    npm run descriptors:check
