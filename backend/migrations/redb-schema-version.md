# redb schema versions

| Version | Tables | Notes |
|---|---|---|
| 1 | alerts, messages, cap_refs, source_state, event_log, metadata | Phase 1–2. Values are JSON records. |

Startup refuses to open a database whose `metadata.schema_version` differs
from the binary's supported version. A migration must be an explicit,
tested step with a verified backup restore (spec §3.3, Phase 5).
Translation tables (`translations`, `translation_state`) are added with Phase 4.
