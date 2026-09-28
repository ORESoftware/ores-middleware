#![forbid(unsafe_code)]

use std::fs;

const CADDYFILE: &str = "adapters/caddy/Caddyfile.example";

fn require(source: &str, needle: &str) {
    if !source.contains(needle) {
        eprintln!("caddy-adapter: missing required text: {needle}");
        std::process::exit(1);
    }
}

fn forbid(source: &str, needle: &str) {
    if source.contains(needle) {
        eprintln!("caddy-adapter: forbidden text present: {needle}");
        std::process::exit(1);
    }
}

fn main() {
    let source = fs::read_to_string(CADDYFILE).unwrap_or_else(|error| {
        eprintln!("caddy-adapter: cannot read {CADDYFILE}: {error}");
        std::process::exit(1);
    });

    require(&source, "trusted_proxies static");
    require(&source, "@forbidden_method method TRACE CONNECT");
    require(&source, "respond @forbidden_method 405");
    require(&source, "@unknown_host not host");
    require(&source, "respond @unknown_host 421");
    require(&source, "max_size 2MB");
    require(&source, "header_up -Forwarded");
    require(&source, "header_up -X-Real-IP");
    require(&source, "header_up -Proxy");
    require(&source, "header_up -tracestate");
    require(&source, "header_up -baggage");
    require(&source, "header_up -Upgrade");
    require(&source, "header_up -X-Request-ID");
    require(&source, "header_up X-Real-IP {client_ip}");
    require(&source, "header_up X-Ores-Edge-Proxy caddy");
    require(&source, "dial_timeout 3s");
    require(&source, "response_header_timeout 30s");

    forbid(&source, "trusted_proxies static 0.0.0.0/0");
    forbid(&source, "trusted_proxies static ::/0");
    forbid(&source, "header_up Authorization");
    forbid(&source, "header_up Cookie");

    println!("caddy-adapter: static admission passed");
}
