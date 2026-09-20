# AGENTS

## Slik jobber vi

Følg **RAVEN-normene** i
[raven-platform/README.md](https://github.com/KODE15AS/raven-platform)
(seksjonen «RAVEN-normer»): handover, ssh, kontoer, tbd, container, lenker,
web-profil, bestilling, språk. Normene gjentas ikke her; fasiten er sjekket ut
på RAVEN under `~/dev/raven-platform`.

## Denne tjenesten

Elduro er et signallaboratorium for Polar H10: live puls, rå EKG (130 Hz) og
akselerometer (200 Hz) via Polars PMD-tjeneste, med tapsfri opptak og en
ESP32-S3-basert feltenhet. Se [README](./README.md) for arkitektur og
[handover/HANDOVER.md](./handover/HANDOVER.md) for status og backlog.

### Kjøre / teste / deploye

- Backend (dev): `cargo run -p elduro-backend` (:8080)
- Frontend (dev): `cd frontend && npm install && npm run dev`
- Capture-agent (dev): `cargo run -p elduro-capture -- --backend ws://127.0.0.1:8080/ws/agent`
- Web-deploy: `docker compose up -d --build web` (:8094)
- Ingress: `docker compose up -d caddy`
- Windows-agent: `docker build -f Dockerfile.agent-windows --target export -o bin .`
- Firmware (ESP32-S3): se `firmware/README.md` (Docker IDF v5.4.4, `/dev/ttyACM0`)

Alt git/bygg/deploy skjer på RAVEN (`~/dev/elduro`); ingen lokal git på
laptop (norm «container»).

### Repo-spesifikke avvik og advarsler

- **Unntak fra norm «container» (ett repo = én container):** compose-stacken
 har to tjenester, `web` og `caddy`. Caddy er den offentlige ingressen på
 raven og fronter **også studio15/erbium.no og wordpress-kode15**, ikke bare
 elduro.no. Endringer i `Caddyfile` eller caddy-tjenesten påvirker andre
 tjenester i produksjon – vær varsom, og se migreringsplanen Caddy → Traefik
 i [handover/HANDOVER.md](./handover/HANDOVER.md).
- **Unntak fra norm «container» nr. 2 (besluttet av Jørn 20.09.2026):**
 elduros kanoniske arkiv skal ligge i den **delte `mariadb:11.4`-containeren**
 på raven (egen database + egen bruker for elduro), ikke i en egen
 db-container. Begrunnelse: databasene er ikke på GitHub, og ett felles
 MariaDB-regime gir ett backup-regime. Konsekvens: omstart/oppgradering av
 den delte containeren påvirker flere tjenester – koordiner. Se
 [docs/format/arkivkoding-vurdering.md](./docs/format/arkivkoding-vurdering.md).
- **Rådata kun på disk:** `recordings/` (måledata) og `bin/` er gitignorert og
  finnes bare på RAVEN. De kan ikke gjenopprettes fra git – slett aldri disse
  som «opprydding».
- **Frosset datakontrakt:** [docs/format/frame-schema.md](./docs/format/frame-schema.md)
  (schema_version 2) er frosset; endringer krever eksplisitt beslutning.
- **Maskinvare i løkka:** capture-agenten trenger BlueZ og BT-600-adapteren på
  verten; firmware flashes mot fysisk enhet. Ikke alt kan verifiseres i
  container.
- **Web-mal:** Elduro bruker KODE15-profilen (`raven-platform/web-profil/`,
  `--k15-*`-tokens) for webflatene (norm «web-profil»).
- **Secrets:** ingen `.env` i bruk i dag; `firmware/main/wifi_creds.h` er
  gitignorert (se `.example`-filen). Aldri commit hemmeligheter (norm
  «kontoer»).
