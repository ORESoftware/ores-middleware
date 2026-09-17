#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NGINX_FRAGMENT="$ROOT/adapters/nginx/ores-middleware.conf.example"
HAPROXY_CONFIG="$ROOT/adapters/haproxy/ores-middleware.cfg.example"

fail() {
  printf 'edge-proxy-adapters: %s\n' "$*" >&2
  exit 1
}

require_contains() {
  local file="$1"
  local needle="$2"
  grep -Fq -- "$needle" "$file" || fail "$file missing required text: $needle"
}

forbid_regex() {
  local file="$1"
  local pattern="$2"
  if grep -Eq -- "$pattern" "$file"; then
    fail "$file contains forbidden pattern: $pattern"
  fi
}

[[ -s "$NGINX_FRAGMENT" ]] || fail "missing NGINX adapter example"
[[ -s "$HAPROXY_CONFIG" ]] || fail "missing HAProxy adapter example"

# Trusted forwarding metadata must be reconstructed at the admitted boundary.
require_contains "$NGINX_FRAGMENT" 'include /etc/nginx/ores/trusted-proxies.conf;'
require_contains "$NGINX_FRAGMENT" 'proxy_set_header X-Forwarded-For $remote_addr;'
require_contains "$NGINX_FRAGMENT" 'proxy_set_header X-Forwarded-Proto $scheme;'
require_contains "$NGINX_FRAGMENT" 'proxy_set_header X-Request-ID $request_id;'
forbid_regex "$NGINX_FRAGMENT" 'set_real_ip_from[[:space:]]+(0\.0\.0\.0/0|::/0)'

# Route-specific login admission must not collapse into the general edge bucket.
require_contains "$NGINX_FRAGMENT" 'map $request_method $ores_auth_login_ip_key {'
require_contains "$NGINX_FRAGMENT" 'POST $binary_remote_addr;'
require_contains "$NGINX_FRAGMENT" 'limit_req_zone $binary_remote_addr zone=ores_edge_default_ip:'
require_contains "$NGINX_FRAGMENT" 'limit_req_zone $ores_auth_login_ip_key zone=ores_auth_login_ip:'
require_contains "$NGINX_FRAGMENT" 'location = /v1/auth/login {'
require_contains "$NGINX_FRAGMENT" 'limit_req zone=ores_auth_login_ip burst=5 nodelay;'
require_contains "$NGINX_FRAGMENT" 'limit_req_status 429;'
require_contains "$NGINX_FRAGMENT" 'add_header X-Content-Type-Options "nosniff" always;'
require_contains "$NGINX_FRAGMENT" 'add_header Referrer-Policy "strict-origin-when-cross-origin" always;'

# HAProxy must likewise overwrite forwarding metadata and keep the login budget
# in a separate table that is tracked only when both path and method match.
require_contains "$HAPROXY_CONFIG" 'http-request del-header X-Forwarded-For'
require_contains "$HAPROXY_CONFIG" 'http-request set-header X-Forwarded-For %[src]'
require_contains "$HAPROXY_CONFIG" 'http-request del-header X-Forwarded-Proto'
require_contains "$HAPROXY_CONFIG" 'http-request set-header X-Request-ID %[unique-id]'
require_contains "$HAPROXY_CONFIG" 'backend ores_rl_default'
require_contains "$HAPROXY_CONFIG" 'backend ores_rl_auth_login'
require_contains "$HAPROXY_CONFIG" 'acl ores_auth_login_path path -i /v1/auth/login'
require_contains "$HAPROXY_CONFIG" 'acl ores_auth_login_method method POST'
require_contains "$HAPROXY_CONFIG" 'http-request track-sc1 src table ores_rl_auth_login if ores_auth_login_path ores_auth_login_method'
require_contains "$HAPROXY_CONFIG" 'http-request deny deny_status 429 if ores_default_rate_exceeded'
require_contains "$HAPROXY_CONFIG" 'http-request deny deny_status 429 if ores_auth_login_path ores_auth_login_method ores_auth_login_rate_exceeded'
require_contains "$HAPROXY_CONFIG" 'http-response set-header X-Content-Type-Options nosniff'
require_contains "$HAPROXY_CONFIG" 'http-response set-header Referrer-Policy strict-origin-when-cross-origin'

if command -v nginx >/dev/null 2>&1; then
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  sed \
    -e 's|include /etc/nginx/ores/trusted-proxies.conf;|set_real_ip_from 127.0.0.1;|' \
    -e 's|access_log /var/log/nginx/ores-access.log ores_json;|access_log off;|' \
    "$NGINX_FRAGMENT" > "$tmp/fragment.conf"
  cat > "$tmp/nginx.conf" <<EOF
pid $tmp/nginx.pid;
events {}
http {
  include $tmp/fragment.conf;
}
EOF
  nginx -t -c "$tmp/nginx.conf" -p "$tmp/"
fi

if command -v haproxy >/dev/null 2>&1; then
  haproxy -c -f "$HAPROXY_CONFIG"
fi

printf 'edge-proxy-adapters: static invariants passed; available native syntax validators passed\n'
