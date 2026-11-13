#!/usr/bin/env bash
# Backup-and-restore drill against the real server binary (run from backend/).
# 1) record live alert count, 2) graceful stop, 3) copy wda.redb, 4) wipe data,
# 5) restore the copy, 6) restart, 7) the same alerts must be served again.
set -euo pipefail
BASE=http://127.0.0.1:8080
DATA=/tmp/wda-e2e/data
count() { curl -fsS "$BASE/api/v1/alerts?west=-180&south=-85&east=180&north=85&zoom=2" | python3 -c 'import sys,json;print(json.load(sys.stdin)["total_matching"])'; }
ids()   { curl -fsS "$BASE/api/v1/alerts?west=-180&south=-85&east=180&north=85&zoom=2" | python3 -c 'import sys,json;print(",".join(sorted(i["alert_id"] for i in json.load(sys.stdin)["items"])))'; }
before=$(count); before_ids=$(ids)
echo "before: $before alert(s)"
[ "$before" -gt 0 ] || { echo "nothing to back up"; exit 1; }
pkill -TERM -f "wda-server" ; for i in $(seq 1 20); do pgrep -f wda-server >/dev/null || break; sleep 0.5; done
cp "$DATA/wda.redb" /tmp/wda-backup.redb
rm -rf "$DATA"; mkdir -p "$DATA"
# Stop the fixture feed so alerts can only come from the database file.
pkill -f "http.server 9000" || true
# Start once with an empty store to prove the data really is gone.
WDA_CONFIG=e2e/app.e2e.toml WDA_BIND=127.0.0.1:8090 ./target/release/wda-server >/tmp/wda-e2e/empty.log 2>&1 &
for i in $(seq 1 30); do curl -fsS http://127.0.0.1:8090/health/ready >/dev/null 2>&1 && break; sleep 0.5; done
empty=$(curl -fsS "http://127.0.0.1:8090/api/v1/alerts?west=-180&south=-85&east=180&north=85&zoom=2" | python3 -c 'import sys,json;print(json.load(sys.stdin)["total_matching"])')
pkill -TERM -f "wda-server"; for i in $(seq 1 20); do pgrep -f wda-server >/dev/null || break; sleep 0.5; done
echo "after wipe: $empty"
[ "$empty" = "0" ] || { echo "FAIL: wiped store still served alerts"; exit 1; }
rm -rf "$DATA"; mkdir -p "$DATA"
cp /tmp/wda-backup.redb "$DATA/wda.redb"
WDA_CONFIG=e2e/app.e2e.toml ./target/release/wda-server >/tmp/wda-e2e/restored.log 2>&1 &
for i in $(seq 1 30); do curl -fsS $BASE/health/ready >/dev/null 2>&1 && break; sleep 0.5; done
after=$(count); after_ids=$(ids)
echo "after restore: $after alert(s)"
[ "$after" = "$before" ] && [ "$after_ids" = "$before_ids" ] || { echo "FAIL: restored data differs"; exit 1; }
echo "OK: backup restored with identical alerts"
