"""End-to-end test: real Rust server + real frontend in Chromium (desktop and mobile).

The server ingests a CAP fixture from a local HTTP server, so every alert shown
comes through the production ingestion path. Run from backend/:

    python3 -m http.server 9000 -d tests/fixtures/cap &          # CAP feed
    WDA_CONFIG=e2e/app.e2e.toml ./target/release/wda-server &     # server
    python3 e2e/test_e2e.py http://127.0.0.1:8080
"""

import json
import re
import sys
import time
import urllib.request

from playwright.sync_api import expect, sync_playwright

BASE = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:8080"
FAILURES = []


def check(name, fn):
    try:
        fn()
        print(f"PASS {name}")
    except Exception as e:  # noqa: BLE001 - report every failure, then exit non-zero
        print(f"FAIL {name}: {e}")
        FAILURES.append(name)


def api(path):
    with urllib.request.urlopen(BASE + path, timeout=10) as r:
        return r.status, dict(r.headers), json.loads(r.read() or b"null")


def wait_for_alert(timeout=90):
    end = time.time() + timeout
    while time.time() < end:
        _, _, b = api("/api/v1/alerts?west=-180&south=-85&east=180&north=85&zoom=2")
        if b["total_matching"] > 0:
            return b["items"][0]
        time.sleep(2)
    raise AssertionError("fixture alert was not ingested")


def test_api_contract():
    s, h, _ = api("/api/v1/meta")
    h = {k.lower(): v for k, v in h.items()}
    assert s == 200
    assert "frame-ancestors 'none'" in h.get("content-security-policy", ""), h
    assert h.get("x-content-type-options") == "nosniff"
    s, _, c = api("/api/v1/contacts/AU")
    assert s == 200 and c["country"]["contacts"][0]["tel_uri"] == "tel:000"
    s, _, g = api("/api/v1/guides/drought")
    assert g["status"] == "unavailable"


def run_desktop(p, alert):
    browser = p.chromium.launch()
    ctx = browser.new_context(viewport={"width": 1280, "height": 800}, locale="en-US")
    page = ctx.new_page()
    errors = []
    page.on("pageerror", lambda e: errors.append(str(e)))
    page.goto(BASE + "/")

    def status_ready():
        expect(page.locator("#statusText")).not_to_have_text(re.compile("Loading"), timeout=20000)

    check("desktop: status leaves loading state", status_ready)

    def map_loaded():
        expect(page.locator(".maplibregl-canvas")).to_be_visible(timeout=30000)
        expect(page.locator(".maplibregl-ctrl-attrib")).to_contain_text("OpenStreetMap", timeout=15000)
        expect(page.locator(".pin, .cluster").first).to_be_visible(timeout=20000)

    check("desktop: MapLibre map, OSM attribution and disaster marker", map_loaded)

    def list_and_detail():
        page.click("#listBtn")
        item = page.locator(f'.item[data-id="{alert["alert_id"]}"]')
        expect(item).to_be_visible(timeout=20000)
        item.click()
        detail = page.locator("#detail")
        expect(detail).to_be_visible()
        expect(page.locator("#detailInner h1")).not_to_have_text("Loading…", timeout=10000)
        close = page.locator("#dclose")
        expect(close).to_be_visible()
        page.locator("#detailInner").evaluate("el => el.scrollTop = el.scrollHeight")
        expect(close).to_be_in_viewport()  # sticky header keeps close visible
        expect(page.locator(".sec.todo")).to_be_visible()
        expect(page.locator("details.tech")).not_to_have_attribute("open", "")
        expect(page.locator("#facSec")).to_contain_text("No official source has linked", timeout=10000)
        expect(page.locator("#guideSec")).to_contain_text("Floods", timeout=10000)
        close.click()
        expect(detail).to_be_hidden()

    check("desktop: list → detail panel order, sticky close, facilities honesty", list_and_detail)

    def help_screen():
        page.click("#helpBtn")
        help_ = page.locator("#help")
        expect(help_).to_be_visible()
        assert page.evaluate("document.getElementById('app').inert") is True
        page.fill("#ctySearch", "Australia (AU)")
        page.dispatch_event("#ctySearch", "change")
        call = page.locator('.callbtn[href="tel:000"]')
        expect(call).to_be_visible()
        assert page.url.startswith(BASE), "no automatic navigation/call"
        page.fill("#ctySearch", "Aruba (AW)")
        page.dispatch_event("#ctySearch", "change")
        expect(page.locator(".ccard")).to_contain_text("Do not assume 112 or 911")
        page.click("#tab-guides")
        expect(page.locator(".guide").first).to_be_visible()
        expect(page.locator("#hs-body")).to_contain_text("No reviewed official guidance")
        expect(page.locator("#hclose")).to_be_in_viewport()
        page.keyboard.press("Escape")
        expect(help_).to_be_hidden()
        assert page.evaluate("document.getElementById('app').inert") is False
        assert page.evaluate("document.activeElement.id") == "helpBtn", "focus restored"

    check("desktop: full-screen help, directory, tel links, guides, Escape", help_screen)

    def search():
        page.fill("#q", "13.75, 100.5")
        expect(page.locator("#sugg")).to_contain_text("Coordinates", timeout=5000)
        page.fill("#q", "japan")
        expect(page.locator("#sugg")).to_contain_text("Japan", timeout=5000)
        page.fill("#q", "Some Street 12")
        page.wait_for_timeout(500)
        page.keyboard.press("End")
        page.locator('#sugg [role=option]').last.dispatch_event("mousedown")
        expect(page.locator("#sugg")).to_contain_text("not enabled", timeout=5000)
        page.keyboard.press("Escape")

    check("desktop: search coordinates, countries, honest address search", search)

    def location_menu():
        page.click("#locBtn")
        expect(page.locator("#gpsOnce")).to_be_visible()
        expect(page.locator("#gpsLive")).to_be_visible()
        page.keyboard.press("Escape")
        expect(page.locator("#locMenu")).to_be_hidden()

    check("desktop: location menu once/live", location_menu)

    def gps_denied():
        page.click("#locBtn")
        page.click("#gpsOnce")  # no permission granted in this context → denied message
        expect(page.locator("#toast")).to_contain_text(re.compile("denied|unavailable|too long", re.I), timeout=20000)

    check("desktop: geolocation denied is reported", gps_denied)

    def offline():
        page.click("#helpBtn")  # warm the directory cache
        expect(page.locator(".ccard, #ctySearch").first).to_be_visible()
        page.keyboard.press("Escape")
        ctx.set_offline(True)
        page.evaluate("window.dispatchEvent(new Event('offline'))")
        expect(page.locator("#status")).to_have_class(re.compile("offline"), timeout=5000)
        page.click("#helpBtn")
        expect(page.locator("#hs-body")).to_contain_text("Saved copy from", timeout=10000)
        page.keyboard.press("Escape")
        ctx.set_offline(False)
        page.evaluate("window.dispatchEvent(new Event('online'))")

    check("desktop: offline state and saved directory", offline)
    check("desktop: no uncaught page errors", lambda: (_ for _ in ()).throw(AssertionError(errors)) if errors else None)
    browser.close()


def run_mobile(p, alert):
    browser = p.chromium.launch()
    ctx = browser.new_context(**p.devices["Pixel 5"], locale="th-TH")
    page = ctx.new_page()
    page.goto(BASE + "/")

    def sheet():
        expect(page.locator("html")).to_have_attribute("lang", "th")
        page.goto(BASE + "/#" + alert["alert_id"])
        detail = page.locator("#detail")
        expect(detail).to_be_visible(timeout=15000)
        box = detail.bounding_box()
        vh = page.viewport_size["height"]
        assert box["y"] > vh * 0.3 and abs(box["y"] + box["height"] - vh) < 4, box  # bottom sheet
        expect(page.locator("#dclose")).to_be_visible()
        page.click("#dclose")
        expect(detail).to_be_hidden()

    check("mobile: Thai auto-detected, detail as bottom sheet", sheet)

    def help_mobile():
        page.click("#helpBtn")
        expect(page.locator("#help")).to_be_visible()
        box = page.locator("#help").bounding_box()
        assert box["width"] >= page.viewport_size["width"] - 1, box
        page.click("#hclose")
        expect(page.locator("#help")).to_be_hidden()

    check("mobile: help covers the screen and closes", help_mobile)
    browser.close()


def main():
    check("api: contract and security headers", test_api_contract)
    alert = wait_for_alert()
    with sync_playwright() as p:
        run_desktop(p, alert)
        run_mobile(p, alert)
    print(f"\n{len(FAILURES)} failure(s)")
    sys.exit(1 if FAILURES else 0)


if __name__ == "__main__":
    main()
