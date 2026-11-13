#!/usr/bin/env python3
"""Builds backend/data/emergency_contacts.json.

Country/territory list: ISO 3166-1 (Debian iso-codes package, /usr/share/iso-codes).
Contacts: ONLY entries checked against an official source during review. Every other
country/territory is recorded with coverage "unavailable" and no numbers — never guessed.
Re-run after editing VERIFIED below. Review date is recorded per entry.
"""
import json, sys

REVIEWED = "2026-11-13"
ISO = json.load(open("/usr/share/iso-codes/json/iso_3166-1.json"))["3166-1"]

def c(service, display, dial, authority, url, verification, **kw):
    return {"service": service, "display": display, "dial": dial, "authority": authority,
            "source_url": url, "verification": verification, "last_verified": REVIEWED,
            "region": kw.get("region"), "network": kw.get("network"),
            "instructions": kw.get("instructions"), "language": kw.get("language", "en"),
            "notes": kw.get("notes"), "valid_from": None, "valid_until": None}

EC_112 = "https://digital-strategy.ec.europa.eu/en/library/2024-report-implementation-eu-emergency-number-112"
EU = ["AT","BE","BG","HR","CY","CZ","DK","EE","FI","FR","DE","GR","HU","IE","IT","LV","LT","LU","MT","NL","PL","PT","RO","SK","SI","ES","SE"]

VERIFIED = {
  "AU": [
    c("general","000","000","Australian Government — Department of Infrastructure, Transport, Regional Development, Communications and the Arts","https://www.infrastructure.gov.au/media-communications/phone/triple-zero/how-call-triple-zero-000","verified",
      notes="Police, fire or ambulance. If there is no mobile coverage on any network, a mobile phone cannot reach Triple Zero."),
    c("general","112","112","Australian Government — Department of Infrastructure, Transport, Regional Development, Communications and the Arts","https://www.infrastructure.gov.au/media-communications/phone/triple-zero/using-other-emergency-numbers","verified",
      network="Mobile phones only", notes="Connects to the same Triple Zero service; no priority over 000; does not work via satellite."),
    c("relay","106","106","Australian Government — Department of Infrastructure, Transport, Regional Development, Communications and the Arts","https://www.infrastructure.gov.au/media-communications/phone/triple-zero/using-other-emergency-numbers","verified",
      instructions="TTY (textphone) emergency service.", network="TTY / textphone"),
  ],
  "GB": [
    c("general","999","999","UK Government (GOV.UK)","https://www.gov.uk/guidance/999-and-112-the-uks-national-emergency-numbers","verified",
      notes="Police, fire and rescue, ambulance and coastguard."),
    c("general","112","112","UK Government (GOV.UK)","https://www.gov.uk/guidance/999-and-112-the-uks-national-emergency-numbers","verified",
      notes="Connects to the same service as 999."),
  ],
  "NZ": [
    c("general","111","111","New Zealand Police","https://www.police.govt.nz/call-111","verified",
      notes="Police, Fire or Ambulance; the operator asks which service you need. Free; works on a mobile even without credit."),
  ],
  "US": [
    c("general","911","911","Federal Communications Commission (FCC)","https://www.fcc.gov/node/25869","verified"),
  ],
  "CA": [
    c("general","911","911","Federal Communications Commission (FCC), United States","https://www.fcc.gov/node/25869","official_secondary",
      notes="The FCC states 911 is the official emergency number in the United States and Canada. Not yet confirmed against a Canadian authority."),
  ],
  "JP": [
    c("police","110","110","Chuo City (Tokyo) — official municipal guidance","https://www.city.chuo.lg.jp/en/introuble.html","verified", language="en",
      notes="Dial without an area code."),
    c("fire_ambulance","119","119","Chuo City (Tokyo) — official municipal guidance","https://www.city.chuo.lg.jp/en/introuble.html","verified", language="en",
      notes="Fire, sudden illness or serious injury."),
  ],
  "SG": [
    c("fire_ambulance","995","995","Singapore Civil Defence Force (SCDF)","https://www.scdf.gov.sg/home/about-us/information-on-ems","verified",
      notes="For life-threatening emergencies only. SCDF Emergency SMS: 70995."),
  ],
  "TH": [
    c("police","191","191","UK Government foreign travel advice (GOV.UK)","https://www.gov.uk/foreign-travel-advice/thailand/getting-help","official_secondary", language="th",
      notes="Listed by a foreign government; not yet confirmed against the Royal Thai Police."),
    c("ambulance","1669","1669","UK Government foreign travel advice (GOV.UK)","https://www.gov.uk/foreign-travel-advice/thailand/getting-help","official_secondary", language="th",
      notes="Listed by a foreign government; not yet confirmed against the National Institute for Emergency Medicine."),
    c("fire","199","199","UK Government foreign travel advice (GOV.UK)","https://www.gov.uk/foreign-travel-advice/thailand/getting-help","official_secondary", language="th"),
    c("tourist_police","1155","1155","UK Government foreign travel advice (GOV.UK)","https://www.gov.uk/foreign-travel-advice/thailand/getting-help","official_secondary", language="th"),
  ],
}
for code in EU:
    VERIFIED.setdefault(code, []).append(
        c("general","112","112","European Commission","" + EC_112,"verified",
          notes="Single European emergency number, available in all EU Member States. National service numbers not yet reviewed."))

def coverage(contacts, eu_only=False):
    if not contacts:
        return "unavailable"
    states = {x["verification"] for x in contacts}
    services = {x["service"] for x in contacts if x["verification"] == "verified"}
    core = "general" in services or ({"police"} <= services and bool(services & {"fire_ambulance", "ambulance"}))
    if states == {"verified"} and core and not eu_only:
        return "verified"
    if "verified" in states or "official_secondary" in states:
        return "partially_verified"
    return "unverified"

out = []
for e in ISO:
    code = e["alpha_2"]
    contacts = VERIFIED.get(code, [])
    eu_only = code in EU and all(x["authority"] == "European Commission" for x in contacts)
    out.append({
        "code": code, "alpha_3": e["alpha_3"], "numeric": e["numeric"],
        "name": e.get("common_name") or e["name"], "official_name": e.get("official_name"),
        "coverage": coverage(contacts, eu_only), "last_reviewed": REVIEWED if contacts else None,
        "contacts": contacts,
        "notes": None if contacts else "No contact has been verified against an official source yet. Do not assume 112 or 911 works here.",
    })

doc = {"schema_version": 1, "dataset_version": REVIEWED,
       "country_list_source": "ISO 3166-1 via Debian iso-codes",
       "verification_levels": {
         "verified": "Confirmed on the responsible authority's (or an EU institution's) official publication.",
         "official_secondary": "Published by another government's official site; not yet confirmed with the responsible authority.",
         "unverified": "Not confirmed against an official source."},
       "countries": out}
json.dump(doc, open(sys.argv[1], "w"), ensure_ascii=False, indent=1)
from collections import Counter
print(len(out), Counter(x["coverage"] for x in out))
