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
    c("general","911","911","Parks Canada (Government of Canada)","https://parks.canada.ca/voyage-travel/securite-safety/urgence-emergency","verified",
      notes="Police, fire or ambulance. Mobile coverage is not reliable in the backcountry or remote areas."),
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
    c("police","191","191","Royal Thai Police (สำนักงานตำรวจแห่งชาติ)","https://old.royalthaipolice.go.th/faq.php","verified", language="th",
      notes="เหตุด่วน เหตุร้าย / urgent incidents and crime."),
    c("ambulance","1669","1669","National Institute for Emergency Medicine (สถาบันการแพทย์ฉุกเฉินแห่งชาติ)","https://www.niems.go.th/1/News/Detail/396?group=2","verified", language="th",
      notes="เจ็บป่วยฉุกเฉิน อุบัติเหตุ 24 ชั่วโมง ฟรี / medical emergencies and accidents, 24 h, free."),
    c("fire","199","199","Royal Thai Police — Phueng Daet station (official police unit page)","https://phungdaed.mukdahan.police.go.th/page11.html","verified", language="th",
      notes="แจ้งอัคคีภัย / fire."),
    c("disaster","1784","1784","Department of Disaster Prevention and Mitigation (กรมป้องกันและบรรเทาสาธารณภัย)","https://www.disaster.go.th/contact/central","verified", language="th",
      notes="สายด่วน ปภ. แจ้งเหตุสาธารณภัย 24 ชม. / disaster reporting hotline, 24 h."),
    c("tourist_police","1155","1155","Royal Thai Police (สำนักงานตำรวจแห่งชาติ)","https://old.royalthaipolice.go.th/faq.php","verified", language="th"),
  ],
  "IN": [
    c("general","112","112","Ministry of Home Affairs — Emergency Response Support System (ERSS)","https://112.gov.in/","verified",
      notes="Pan-India single number for police, fire & rescue, health. Panic call: press the power button 3 times quickly (smartphones) or long-press 5 or 9 (feature phones)."),
  ],
  "PH": [
    c("general","911","911","Government of the Philippines (GOV.PH)","https://www.gov.ph/the-government/hotlines/","verified",
      notes="National Emergency Hotline (Executive Order No. 56). Unified 911 call centres were being expanded region by region from September 2025."),
  ],
  "MY": [
    c("general","999","999","Government of Malaysia (MyGOV) — MERS 999","https://www.malaysia.gov.my/en/categories/safety-and-community/public-safety/mers-999-emergency-line","verified",
      notes="Police, Fire and Rescue, Ministry of Health, Civil Defence and Maritime Enforcement. Genuine emergencies only."),
  ],
  "ID": [
    c("general","112","112","Ministry of Communication and Digital Affairs (Komdigi) — Layanan 112","https://layanan112.komdigi.go.id/tentang","verified",
      region="Only regencies/cities that have implemented Call Center 112",
      notes="Free, works with a locked phone. Not yet available in every regency/city."),
    c("police","110","110","Ministry of Communication and Digital Affairs (Komdigi) — Layanan 112","https://layanan112.komdigi.go.id/tentang","verified", language="id"),
    c("fire","113","113","Ministry of Communication and Digital Affairs (Komdigi) — Layanan 112","https://layanan112.komdigi.go.id/tentang","verified", language="id"),
    c("ambulance","119","119","Ministry of Communication and Digital Affairs (Komdigi) — Layanan 112","https://layanan112.komdigi.go.id/tentang","verified", language="id"),
  ],
  "KR": [
    c("police","112","112","Seoul Metropolitan Government","https://world.seoul.go.kr/service/living/medical-facilities/","verified",
      notes="Crime reports; interpretation available."),
    c("fire_ambulance","119","119","Seoul Metropolitan Government","https://world.seoul.go.kr/service/living/medical-facilities/","verified",
      notes="Fire and emergency, including ambulance (free). Interpreters available."),
  ],
  "DE": [
    c("police","110","110","Federal Office of Civil Protection and Disaster Assistance (BBK)","https://www.bbk.bund.de/DE/Themen/Krisenmanagement/Mensch-und-Gesellschaft/Spontanhelfende/Infos-Helfende/Waehrend-des-Einsatzes/waehrend-des-einsatzes.html","verified", language="de"),
    c("fire_ambulance","112","112","Federal Office of Civil Protection and Disaster Assistance (BBK)","https://www.bbk.bund.de/DE/Warnung-Vorsorge/Vorsorge/Gesundheit-und-Hygiene/Erste-Hilfe-und-Notruf/erste-hilfe-und-notruf_node.html","verified", language="de",
      notes="Fire and ambulance, free, also from mobiles with a SIM card. Emergency fax to 110/112 and the nora app for people with speech or hearing impairments."),
  ],
  "FR": [
    c("ambulance","15","15","Ministère de l'Intérieur","https://www.interieur.gouv.fr/actualites/actualites-du-ministere/0-800-112-112-services-durgences-vous-rappellent-desormais-avec","verified", language="fr", notes="SAMU (medical emergencies)."),
    c("police","17","17","Ministère de l'Intérieur","https://www.interieur.gouv.fr/actualites/actualites-du-ministere/0-800-112-112-services-durgences-vous-rappellent-desormais-avec","verified", language="fr"),
    c("fire","18","18","Ministère de l'Intérieur","https://www.interieur.gouv.fr/actualites/actualites-du-ministere/0-800-112-112-services-durgences-vous-rappellent-desormais-avec","verified", language="fr"),
    c("general","112","112","Ministère de l'Intérieur","https://www.interieur.gouv.fr/actualites/actualites-du-ministere/0-800-112-112-services-durgences-vous-rappellent-desormais-avec","verified", language="fr",
      notes="European emergency number, reaches the three services above. Emergency services call back from 0 800 112 112."),
    c("relay","114","114","Ministère de l'Intérieur — Formation civique","https://formation-civique.interieur.gouv.fr/fiches-par-thematiques/vivre-dans-la-societe-fran%C3%A7aise/demarches-administratives/annexe-les-numeros-durgence/","verified", language="fr",
      network="App, internet and SMS", notes="For deaf, deafblind, hard-of-hearing and aphasic people."),
  ],
  "BR": [
    c("police","190","190","Anatel (National Telecommunications Agency)","https://www.anatel.gov.br/consumidor/cartilhas/40-numeros-de-unidade-publica/37-telefones-de-utilidade-publica","verified", language="pt", notes="Polícia Militar."),
    c("ambulance","192","192","Anatel (National Telecommunications Agency)","https://www.anatel.gov.br/consumidor/cartilhas/40-numeros-de-unidade-publica/37-telefones-de-utilidade-publica","verified", language="pt", notes="SAMU."),
    c("fire","193","193","Anatel (National Telecommunications Agency)","https://www.anatel.gov.br/consumidor/cartilhas/40-numeros-de-unidade-publica/37-telefones-de-utilidade-publica","verified", language="pt", notes="Corpo de Bombeiros."),
    c("disaster","199","199","Anatel (National Telecommunications Agency)","https://www.anatel.gov.br/consumidor/cartilhas/40-numeros-de-unidade-publica/37-telefones-de-utilidade-publica","verified", language="pt", notes="Defesa Civil."),
  ],
  "MX": [
    c("general","911","911","Secretariado Ejecutivo del Sistema Nacional de Seguridad Pública (gob.mx/911)","https://www.gob.mx/911/articulos/que-es-el-911emergencias?idiom=es","verified", language="es",
      notes="Single number for medical, security and civil-protection emergencies; free."),
  ],
  "HK": [
    c("general","999","999","Hong Kong SAR Government (GovHK)","https://www.gov.hk/en/nonresidents/visitinghk/usefulinfo/emergency.htm","verified",
      notes="Police, Fire Services Department or ambulance."),
    c("relay","992","992","Hong Kong SAR Government (GovHK)","https://www.gov.hk/en/nonresidents/visitinghk/usefulinfo/emergency.htm","verified",
      network="SMS", instructions="Emergency SMS for people with speech or hearing impairments."),
  ],
  "ZA": [
    c("police","10111","10111","Western Cape Government","https://www.westerncape.gov.za/health-wellness/know-who-you-can-call-emergency","verified", notes="SAPS / Police Flying Squad."),
    c("ambulance","10177","10177","Western Cape Government","https://www.westerncape.gov.za/health-wellness/know-who-you-can-call-emergency","verified"),
    c("general","112","112","Western Cape Government","https://www.westerncape.gov.za/health-wellness/know-who-you-can-call-emergency","verified",
      network="Mobile (MTN, Vodacom, Cell C, Telkom)", notes="There is no single emergency number in South Africa; fire and rescue numbers are set by each district municipality."),
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
