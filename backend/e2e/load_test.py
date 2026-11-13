"""Small load test against a running server (standard library only).

    python3 e2e/load_test.py http://127.0.0.1:8080 [requests] [concurrency]

Prints latency percentiles and error counts per endpoint and exits non-zero if
any request failed or p95 exceeds the budget. Results depend on the machine;
the CI numbers are a regression signal, not a capacity guarantee.
"""

import statistics
import sys
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080"
N = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
C = int(sys.argv[3]) if len(sys.argv) > 3 else 32
P95_BUDGET_MS = 250.0
PATHS = [
    "/api/v1/alerts?west=-180&south=-85&east=180&north=85&zoom=2",
    "/api/v1/alerts?west=95&south=5&east=106&north=21&zoom=6",
    "/api/v1/meta",
    "/api/v1/search?q=bang",
    "/api/v1/contacts/TH",
    "/api/v1/guides/flood?lang=th",
    "/health/ready",
]


def hit(path):
    t = time.perf_counter()
    try:
        with urllib.request.urlopen(BASE + path, timeout=10) as r:
            r.read()
            ok = r.status == 200
    except Exception:  # noqa: BLE001
        ok = False
    return path, (time.perf_counter() - t) * 1000.0, ok


def pct(xs, p):
    xs = sorted(xs)
    return xs[min(len(xs) - 1, int(round(p / 100.0 * (len(xs) - 1))))]


def main():
    jobs = [PATHS[i % len(PATHS)] for i in range(N)]
    start = time.perf_counter()
    with ThreadPoolExecutor(max_workers=C) as ex:
        results = list(ex.map(hit, jobs))
    wall = time.perf_counter() - start
    fails = sum(1 for _, _, ok in results if not ok)
    lat = [ms for _, ms, ok in results if ok]
    print(f"requests={N} concurrency={C} wall={wall:.2f}s throughput={N / wall:.0f} req/s failures={fails}")
    print(f"all: p50={pct(lat, 50):.1f}ms p95={pct(lat, 95):.1f}ms p99={pct(lat, 99):.1f}ms mean={statistics.mean(lat):.1f}ms")
    worst = 0.0
    for p in PATHS:
        xs = [ms for path, ms, ok in results if ok and path == p]
        if xs:
            p95 = pct(xs, 95)
            worst = max(worst, p95)
            print(f"  {p[:60]:60s} n={len(xs):4d} p50={pct(xs, 50):6.1f}ms p95={p95:6.1f}ms")
    if fails or worst > P95_BUDGET_MS:
        print(f"FAIL: failures={fails}, worst p95={worst:.1f}ms (budget {P95_BUDGET_MS}ms)")
        sys.exit(1)
    print("OK")


if __name__ == "__main__":
    main()
