#!/usr/bin/env bash
# E2E smoke for tryingopen2api (Rust axum gateway)
# Usage:
#   API_KEY=sk-xxx scripts/e2e_smoke.sh [BASE_URL]
#   scripts/e2e_smoke.sh http://127.0.0.1:47831
#   API_KEY=sk-xxx scripts/e2e_smoke.sh https://try.hwhcie.bond
# Defaults: BASE_URL=https://try.hwhcie.bond ; API_KEY from $API_KEY env.
# Phase 1 (2026-09-26): authored only, not executed yet.

set -u

BASE_URL="${1:-https://try.hwhcie.bond}"
BASE_URL="${BASE_URL%/}"
API_KEY="${API_KEY:-}"

UA="tryingopen2api-e2e-smoke/1.0"
TS_CMD=(curl -fsS --max-time 45 -A "$UA")
TS_COMMON=("${TS_CMD[@]}")

PASS=0
FAIL=0
SKIP=0
FAILED_NAMES=""

echo "== E2E smoke: tryingopen2api =="
echo "base_url : $BASE_URL"
echo "api_key  : $([ -n "$API_KEY" ] && echo '<provided>' || echo '<absent>')"
echo "start    : $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "----------------------------------------"

# ------------- helpers -------------

# sanitize for log filenames (no '/' or ':' inside)
slug() { printf '%s' "$1" | tr '/:' '__'; }

# result(): name, status, expected, detail
result() {
  local name="$1" status="$2" expected="$3" detail="$4"
  if [ "$status" = "PASS" ]; then
    PASS=$((PASS+1))
    printf 'PASS  %-32s expect=%s  %s\n' "$name" "$expected" "$detail"
  elif [ "$status" = "SKIP" ]; then
    SKIP=$((SKIP+1))
    printf 'SKIP  %-32s expect=%s  %s\n' "$name" "$expected" "$detail"
  else
    FAIL=$((FAIL+1))
    FAILED_NAMES="${FAILED_NAMES}${FAILED_NAMES:+,}$name"
    printf 'FAIL  %-32s expect=%s  %s\n' "$name" "$expected" "$detail"
  fi
}

log_body() {
  local name="$1" body="$2"
  if [ -n "${E2E_SAVE_DIR:-}" ]; then
    mkdir -p "$E2E_SAVE_DIR"
    printf '%s\n' "$body" > "$E2E_SAVE_DIR/$(slug "$name").txt"
  fi
}

# status_ok  : non-empty string in $STATUS.
# detail_str : summary to print (single line, no newlines).
run_req() {
  local name="$1"; shift
  local logf
  logf="$(mktemp 2>/dev/null || printf '%s' /tmp/e2e-smoke-body.$$)"
  STATUS=""
  set +e
  if ! curl -sS --max-time 60 -A "$UA" -w $'\n%{http_code}' "$@" -o "$logf" 2>/tmp/e2e-curl-err.$$; then
    STATUS="ERR"
  else
    STATUS="$(tail -n1 "$logf")"
    if [ "${#STATUS}" -ge 4 ]; then
      sed -i '$d' "$logf" 2>/dev/null
    fi
  fi
  set -e
  DETAIL="$(head -c 300 "$logf" | tr '\n' ' ')"
  if [ "$STATUS" = "ERR" ]; then
    DETAIL="curl error: $(head -c 200 /tmp/e2e-curl-err.$$ 2>/dev/null | tr '\n' ' ')"
  fi
  rm -f /tmp/e2e-curl-err.$$ "$logf"
}

expect_status() {
  local name="$1" expected="$2" actual="$3"
  if [ -z "$actual" ]; then
    result "$name" FAIL "$expected" "no HTTP status captured"
    return 1
  fi
  if [ "$expected" = "2xx" ]; then
    case "$actual" in
      2*) result "$name" PASS "2xx" "HTTP $actual"; return 0 ;;
    esac
  elif [ "$expected" = "4xx/5xx" ]; then
    case "$actual" in
      4*|5*) result "$name" PASS "4xx/5xx" "HTTP $actual"; return 0 ;;
    esac
  else
    [ "$actual" = "$expected" ] && { result "$name" PASS "$expected" "HTTP $actual"; return 0; }
  fi
  result "$name" FAIL "$expected" "HTTP $actual"
  return 1
}

jget() { python - "$1" "$2" 2>/dev/null <<'PY'
import json,sys
try:
    v=json.load(open(sys.argv[1]))
    p=sys.argv[2].split(".")
    for k in p:
        if isinstance(v,dict) and k in v: v=v[k]
        else: raise ValueError
    print(v)
except Exception:
    print("")
PY
}

# ------------- smoke (healthz + ui, no key required) -------------

hdr_file="$(mktemp 2>/dev/null || printf '%s' /tmp/e2e-hdr.$$)"
body_file="$(mktemp 2>/dev/null || printf '%s' /tmp/e2e-body.$$)"

# T1 healthz
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -D "$hdr_file" -o "$body_file" -w '%{http_code}' "$BASE_URL/healthz" 2>/tmp/e2e-err.$$)"
set -e
log_body T1-healthz "$(cat "$body_file")"
if expect_status T1-healthz 200 "$STATUS"; then
  okv="$(jget "$body_file" ok)"
  ver="$(jget "$body_file" version)"
  if [ "$okv" = "True" ] && [ -n "$ver" ]; then
    result T1-healthz-body PASS "ok:true+version" "ok=$okv version=$ver"
  else
    result T1-healthz-body FAIL "ok:true+version" "body ok=$okv version=$ver"
  fi
fi

# T2 UI
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -D "$hdr_file" -o "$body_file" -w '%{http_code}' "$BASE_URL/ui" 2>/tmp/e2e-err.$$)"
set -e
log_body T2-ui "$(cat "$body_file")"
if expect_status T2-ui 200 "$STATUS"; then
  if rg -q -i 'æ§å¶é¢æ¿|é¢æ¿|API' "$body_file" 2>/dev/null; then
    result T2-ui-body PASS "html-panel" "panel keywords found"
  else
    result T2-ui-body FAIL "html-panel" "panel keywords not found"
  fi
fi

# ------------- key-gated checks -------------

if [ -z "$API_KEY" ]; then
  echo "----------------------------------------"
  echo "API_KEY æªæä¾ï¼è·³è¿å¨é¨éè¦é´æçæ£æ¥ï¼T3..T8ï¼"
  result T3-models-no-key SKIP "401" "no API_KEY provided"
  result T4-chat-nonstream SKIP "200" "no API_KEY provided"
  result T5-chat-stream SKIP "200+text/event-stream" "no API_KEY provided"
  result T6-messages SKIP "200" "no API_KEY provided"
  result T7-responses SKIP "200" "no API_KEY provided"
  result T8-metrics SKIP "200/401" "no API_KEY provided"
  result T9-rate-limit SKIP "429 observed" "no API_KEY provided"
  result T10-breaker SKIP "4xx/5xx" "no API_KEY provided"
  rm -f "$hdr_file" "$body_file"
  echo "----------------------------------------"
  echo "SUMMARY: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
  echo "RESULT=$([ "$FAIL" -eq 0 ] && echo PASS || echo FAIL)"
  exit 0
fi

# T3a /v1/models without key -> 401
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/models" 2>/tmp/e2e-err.$$)"
set -e
log_body T3a-models-no-key "$(cat "$body_file")"
expect_status T3a-models-no-key 401 "$STATUS" || true

# T3b /v1/models with key -> 200 + data array
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -H "Authorization: Bearer $API_KEY" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/models" 2>/tmp/e2e-err.$$)"
set -e
log_body T3b-models-with-key "$(cat "$body_file")"
if expect_status T3b-models-with-key 200 "$STATUS"; then
  data="$(jget "$body_file" data)"
  if [ -n "$data" ] && printf '%s' "$data" | rg -q '^\['; then
    result T3b-models-data PASS "data-array" "data array non-empty"
  else
    result T3b-models-data FAIL "data-array" "data not an array / empty"
  fi
fi

chat_payload='{"model":"qwen/qwen3.8-27b","messages":[{"role":"user","content":"hi"}],"stream":false}'

# T4 chat non-streaming
set +e
STATUS="$(curl -sS --max-time 120 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$chat_payload" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/chat/completions" 2>/tmp/e2e-err.$$)"
set -e
log_body T4-chat-nonstream "$(cat "$body_file")"
expect_status T4-chat-nonstream 200 "$STATUS" || true

# T5 chat streaming -> 200 + text/event-stream
stream_payload='{"model":"qwen/qwen3.8-27b","messages":[{"role":"user","content":"hi"}],"stream":true}'
set +e
STATUS="$(curl -sS --max-time 120 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -D "$hdr_file" -d "$stream_payload" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/chat/completions" 2>/tmp/e2e-err.$$)"
set -e
log_body T5-chat-stream "$(cat "$body_file")"
if expect_status T5-chat-stream 200 "$STATUS"; then
  ctype="$(rg -i '^content-type:' "$hdr_file" | head -n1 | tr -d '\r')"
  if printf '%s' "$ctype" | rg -qi 'text/event-stream'; then
    result T5-chat-stream-ctype PASS "text/event-stream" "$ctype"
  else
    result T5-chat-stream-ctype FAIL "text/event-stream" "content-type=$ctype body_head=$(head -c 120 "$body_file" | tr '\n' ' ')"
  fi
fi

# T6 Anthropic /v1/messages
anthropic_payload='{"model":"qwen/qwen3.8-27b","messages":[{"role":"user","content":"hi"}],"max_tokens":64,"stream":false}'
set +e
STATUS="$(curl -sS --max-time 120 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$anthropic_payload" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/messages" 2>/tmp/e2e-err.$$)"
set -e
log_body T6-messages "$(cat "$body_file")"
expect_status T6-messages 200 "$STATUS" || true

# T7 Responses API /v1/responses
responses_payload='{"model":"qwen/qwen3.8-27b","input":"hi","stream":false}'
set +e
STATUS="$(curl -sS --max-time 120 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$responses_payload" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/responses" 2>/tmp/e2e-err.$$)"
set -e
log_body T7-responses "$(cat "$body_file")"
expect_status T7-responses 200 "$STATUS" || true

# T8 metrics: no key -> 401, with key -> 200
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -o "$body_file" -w '%{http_code}' "$BASE_URL/metrics" 2>/tmp/e2e-err.$$)"
set -e
log_body T8a-metrics-no-key "$(cat "$body_file")"
expect_status T8a-metrics-no-key 401 "$STATUS" || true
set +e
STATUS="$(curl -sS --max-time 45 -A "$UA" -H "Authorization: Bearer $API_KEY" -o "$body_file" -w '%{http_code}' "$BASE_URL/metrics" 2>/tmp/e2e-err.$$)"
set -e
log_body T8b-metrics-with-key "$(cat "$body_file")"
expect_status T8b-metrics-with-key 200 "$STATUS" || true

# T9 rate limit: burst 6 -> expect at least one 429
rl_seen=""
rl_statuses=""
i=0
while [ "$i" -lt 6 ]; do
  set +e
  s="$(curl -sS --max-time 90 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$chat_payload" -o /dev/null -w '%{http_code}' "$BASE_URL/v1/chat/completions" 2>/dev/null)"
  set -e
  rl_statuses="${rl_statuses}${rl_statuses:+,}$s"
  if [ "$s" = "429" ]; then rl_seen=1; fi
  i=$((i+1))
done
if [ -n "$rl_seen" ]; then
  result T9-rate-limit PASS "429 observed" "statuses=[$rl_statuses]"
else
  result T9-rate-limit FAIL "429 observed" "statuses=[$rl_statuses] (no 429; threshold per production config)"
fi

# T10 breaker: unknown model -> 4xx/5xx (not 200)
breakers=''
if [ "${E2E_RUN_BREAKER:-1}" = "1" ]; then
  breaker_payload='{"model":"does-not-exist-zzz-model","messages":[{"role":"user","content":"hi"}],"stream":false}'
  set +e
  STATUS="$(curl -sS --max-time 90 -A "$UA" -H "Authorization: Bearer $API_KEY" -H 'Content-Type: application/json' -d "$breaker_payload" -o "$body_file" -w '%{http_code}' "$BASE_URL/v1/chat/completions" 2>/tmp/e2e-err.$$)"
  set -e
  log_body T10-breaker "$(cat "$body_file")"
  breakers="$STATUS"
  expect_status T10-breaker 4xx/5xx "$STATUS" || true
else
  result T10-breaker SKIP "4xx/5xx" "disabled via E2E_RUN_BREAKER=0"
fi

rm -f "$hdr_file" "$body_file"

echo "----------------------------------------"
echo "SUMMARY: PASS=$PASS FAIL=$FAIL SKIP=$SKIP"
if [ -n "$FAILED_NAMES" ]; then
  echo "FAILED : $FAILED_NAMES"
fi
if [ "$FAIL" -eq 0 ]; then
  echo "RESULT=PASS"
  exit 0
else
  echo "RESULT=FAIL"
  exit 1
fi
