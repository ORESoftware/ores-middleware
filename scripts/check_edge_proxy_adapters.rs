#![forbid(unsafe_code)]

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const NGINX_REL: &str = "adapters/nginx/ores-middleware.conf.example";
const HAPROXY_REL: &str = "adapters/haproxy/ores-middleware.cfg.example";
const REQUIRE_NATIVE_ENV: &str = "ORES_EDGE_PROXY_REQUIRE_NATIVE";

fn fail(message: impl AsRef<str>) -> ! {
    eprintln!("edge-proxy-adapters: {}", message.as_ref());
    std::process::exit(1);
}

fn read_required(root: &Path, relative: &str) -> String {
    let path = root.join(relative);
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| fail(format!("cannot read {}: {error}", path.display())));
    if source.trim().is_empty() {
        fail(format!("{} is empty", path.display()));
    }
    source
}

fn require_contains(name: &str, source: &str, needle: &str) {
    if !source.contains(needle) {
        fail(format!("{name} missing required text: {needle}"));
    }
}

fn forbid_contains(name: &str, source: &str, needle: &str) {
    if source.contains(needle) {
        fail(format!("{name} contains forbidden text: {needle}"));
    }
}

fn require_occurrences(name: &str, source: &str, needle: &str, minimum: usize) {
    let count = source.matches(needle).count();
    if count < minimum {
        fail(format!(
            "{name} requires at least {minimum} occurrences of {needle:?}; found {count}"
        ));
    }
}

fn normalized_whitespace(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn check_nginx(source: &str) {
    const NAME: &str = "NGINX adapter";
    require_contains(NAME, source, "include /etc/nginx/ores/trusted-proxies.conf;");
    require_contains(NAME, source, "real_ip_recursive on;");
    require_contains(NAME, source, "proxy_set_header Forwarded \"\";");
    require_contains(NAME, source, "proxy_set_header X-Forwarded-For $remote_addr;");
    require_contains(NAME, source, "proxy_set_header X-Real-IP $remote_addr;");
    require_contains(NAME, source, "proxy_set_header X-Forwarded-Proto $scheme;");
    require_contains(NAME, source, "proxy_set_header X-Forwarded-Host \"\";");
    require_contains(NAME, source, "proxy_set_header X-Forwarded-Port \"\";");
    require_contains(NAME, source, "proxy_set_header X-Request-ID $request_id;");
    require_contains(NAME, source, "proxy_set_header Connection \"\";");
    require_contains(NAME, source, "proxy_next_upstream off;");

    let normalized = normalized_whitespace(source);
    forbid_contains(NAME, &normalized, "set_real_ip_from 0.0.0.0/0;");
    forbid_contains(NAME, &normalized, "set_real_ip_from ::/0;");

    require_contains(NAME, source, "map $request_method $ores_auth_login_ip_key {");
    require_contains(NAME, source, "POST $binary_remote_addr;");
    require_contains(NAME, source, "limit_req_zone $binary_remote_addr zone=ores_edge_default_ip:");
    require_contains(NAME, source, "limit_req_zone $ores_auth_login_ip_key zone=ores_auth_login_ip:");
    require_contains(NAME, source, "location = /v1/auth/login {");
    require_occurrences(NAME, source, "limit_req zone=ores_edge_default_ip burst=40 nodelay;", 2);
    require_contains(NAME, source, "limit_req zone=ores_auth_login_ip burst=5 nodelay;");
    require_contains(NAME, source, "limit_req_status 429;");
    require_contains(NAME, source, "limit_req_log_level warn;");

    require_contains(NAME, source, "client_header_timeout 10s;");
    require_contains(NAME, source, "client_body_timeout 10s;");
    require_contains(NAME, source, "proxy_connect_timeout 3s;");
    require_contains(NAME, source, "proxy_read_timeout 30s;");
    require_contains(NAME, source, "client_max_body_size 2m;");
    require_contains(NAME, source, "client_max_body_size 64k;");

    require_contains(NAME, source, "'\"path\":\"$uri\",'");
    forbid_contains(NAME, source, "$request_uri");
    forbid_contains(NAME, source, "$args");
    forbid_contains(NAME, source, "$http_authorization");
    forbid_contains(NAME, source, "$http_cookie");
    require_contains(NAME, source, "add_header X-Content-Type-Options \"nosniff\" always;");
    require_contains(NAME, source, "add_header Referrer-Policy \"strict-origin-when-cross-origin\" always;");
}

fn check_haproxy(source: &str) {
    const NAME: &str = "HAProxy adapter";
    require_contains(NAME, source, "unique-id-format \"%[uuid]\"");
    require_contains(NAME, source, "http-request set-header X-Request-ID %[unique-id]");
    forbid_contains(NAME, source, "option httplog");
    require_contains(NAME, source, "log-format '{\"request_id\":\"%ID\",\"method\":\"%HM\",\"path\":\"%HP\"");
    forbid_contains(NAME, source, "%HQ");
    forbid_contains(NAME, source, "%HU");
    forbid_contains(NAME, source, "%ci");
    forbid_contains(NAME, source, "%cp");

    require_contains(NAME, source, "http-request del-header Forwarded");
    require_contains(NAME, source, "http-request del-header X-Forwarded-For");
    require_contains(NAME, source, "http-request del-header X-Forwarded-Proto");
    require_contains(NAME, source, "http-request del-header X-Forwarded-Host");
    require_contains(NAME, source, "http-request del-header X-Forwarded-Port");
    require_contains(NAME, source, "http-request del-header X-Real-IP");
    require_contains(NAME, source, "http-request set-header X-Forwarded-For %[src]");
    require_contains(NAME, source, "http-request set-header X-Real-IP %[src]");

    require_occurrences(NAME, source, "stick-table type ipv6", 2);
    forbid_contains(NAME, source, "stick-table type ip ");
    require_contains(NAME, source, "backend ores_rl_default");
    require_contains(NAME, source, "backend ores_rl_auth_login");
    require_contains(NAME, source, "acl ores_auth_login_path path /v1/auth/login");
    forbid_contains(NAME, source, "acl ores_auth_login_path path -i");
    require_contains(NAME, source, "acl ores_auth_login_method method POST");
    require_contains(NAME, source, "http-request track-sc1 src table ores_rl_auth_login if ores_auth_login_path ores_auth_login_method");
    require_contains(NAME, source, "http-request deny deny_status 429 if ores_default_rate_exceeded");
    require_contains(NAME, source, "http-request deny deny_status 429 if ores_auth_login_path ores_auth_login_method ores_auth_login_rate_exceeded");

    require_contains(NAME, source, "timeout http-request 10s");
    require_contains(NAME, source, "timeout http-keep-alive 10s");
    require_contains(NAME, source, "http-after-response set-header X-Content-Type-Options nosniff");
    require_contains(NAME, source, "http-after-response set-header Referrer-Policy strict-origin-when-cross-origin");
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    env::temp_dir().join(format!("ores-edge-proxy-{}-{nanos}", std::process::id()))
}

fn run_checked(program: &str, args: &[&str]) {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| fail(format!("failed to execute {program}: {error}")));
    if !output.status.success() {
        fail(format!(
            "{program} validation failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
}

fn command_available(program: &str, version_arg: &str) -> bool {
    Command::new(program)
        .arg(version_arg)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

fn native_required() -> bool {
    env::var(REQUIRE_NATIVE_ENV)
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn validate_nginx_native(source: &str) {
    let temp = unique_temp_dir();
    fs::create_dir_all(&temp)
        .unwrap_or_else(|error| fail(format!("cannot create {}: {error}", temp.display())));

    let fragment = source
        .replace("include /etc/nginx/ores/trusted-proxies.conf;", "set_real_ip_from 127.0.0.1;")
        .replace("access_log /var/log/nginx/ores-access.log ores_json;", "access_log off;");
    let fragment_path = temp.join("fragment.conf");
    fs::write(&fragment_path, fragment)
        .unwrap_or_else(|error| fail(format!("cannot write {}: {error}", fragment_path.display())));

    let config_path = temp.join("nginx.conf");
    let config = format!(
        "pid {}/nginx.pid;\nerror_log stderr notice;\nevents {{}}\nhttp {{\n  include {};\n}}\n",
        temp.display(),
        fragment_path.display()
    );
    fs::write(&config_path, config)
        .unwrap_or_else(|error| fail(format!("cannot write {}: {error}", config_path.display())));

    let config_arg = config_path.to_string_lossy().into_owned();
    let prefix_arg = format!("{}/", temp.display());
    run_checked("nginx", &["-t", "-c", &config_arg, "-p", &prefix_arg]);
    fs::remove_dir_all(&temp)
        .unwrap_or_else(|error| fail(format!("cannot remove {}: {error}", temp.display())));
}

fn validate_haproxy_native(root: &Path) {
    let config = root.join(HAPROXY_REL);
    let config_arg = config.to_string_lossy().into_owned();
    run_checked("haproxy", &["-c", "-f", &config_arg]);
}

fn main() {
    let root = env::current_dir().unwrap_or_else(|error| fail(format!("cannot get cwd: {error}")));
    let nginx = read_required(&root, NGINX_REL);
    let haproxy = read_required(&root, HAPROXY_REL);

    check_nginx(&nginx);
    check_haproxy(&haproxy);

    let nginx_available = command_available("nginx", "-v");
    let haproxy_available = command_available("haproxy", "-v");
    if native_required() && (!nginx_available || !haproxy_available) {
        fail(format!(
            "native validators required but unavailable: nginx={nginx_available}, haproxy={haproxy_available}"
        ));
    }
    if nginx_available {
        validate_nginx_native(&nginx);
    }
    if haproxy_available {
        validate_haproxy_native(&root);
    }

    println!(
        "edge-proxy-adapters: static invariants passed; native nginx={nginx_available}, haproxy={haproxy_available}"
    );
}
