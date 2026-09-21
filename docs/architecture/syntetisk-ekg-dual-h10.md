# Syntetisk EKG fra to Polar H10 – planlegging (20.09.2026)

Tidlig tankearbeid foran H10 nr. 2, bestilt av Jørn. **Oppdatert 21.09.2026:
beslutningene i seksjonen nederst er låst (Jørn, chat 5); resten av dokumentet
er planleggingsgrunnlaget.**
Se også [field-unit-and-dual-h10-sync.md](./field-unit-and-dual-h10-sync.md)
for synk-avgjørelsen (hjerteslag-forankret, i etterbehandling).

## Mål og retning (fra Jørn 20.09)

- To H10 gir to separate rå-EKG som vi vil se **hver for seg** (to strimler i
  RAW ECG, over hverandre – UI-en er nå forberedt for dette).
- RHYTHM/HRV skal IKKE lenger vise individuelt per belte; der vil vi vise **det
  best mulige aggregerte «syntetiske» EKG** satt sammen fra de to.
- RR-intervall og RMSSD utsettes til vi ser hvor godt det syntetiske EKG-et
  faktisk blir.

## Viktig premiss: to belter = to avledninger, ikke to kopier

To H10 på ulike plasseringer på brystet måler **forskjellige projeksjoner av
hjertets elektriske vektor** (ulike «avledninger»), ikke samme signal med litt
støy. Det betyr:

- «Syntetisk EKG» er **avlednings-fusjon/rekonstruksjon**, ikke enkel midling.
  Å bare snitte to avledninger kan svekke morfologi (P/QRS/T kan delvis
  kansellere).
- Verdien er nettopp at to avledninger sammen bærer mer informasjon enn én –
  grunnlaget for full-EKG-rekonstruksjon, AV-blokk og invertert T (jf.
  README «Research goals»). Plasseringsplanen fra chat 3 (øvre belte rotert
  under høyre bryst, nedre under venstre) er valgt for å gi to nyttige,
  ~ortogonale avledninger.

## Forutsetninger som må på plass først

1. **Felles tidslinje.** Kanalene må ligge på samme klokke før noen fusjon.
   - SNTP på ESP32 (levert 20.09) gir ekte veggklokke → `ts_host_ns` i Unix-tid
     og `clock: ntp-synced`. Nødvendig, men ikke tilstrekkelig alene.
   - Sub-sample-justering gjøres **hjerteslag-forankret** (R-topp mot R-topp),
     som besluttet – NTP/host-tid grovjusterer, R-toppene finjusterer.
2. **To samtidige kilder i pipelinen.** Backend/arkiv håndterer allerede N
   kilder (dedup-nøkkel `(device_id, stream, ts_device_ns)` per belte). To
   H10 = to `device_id`, to sesjoner, felles subject.
3. **Én sentral per belte.** H10 tar én BLE-sentral; to belter trenger to
   sentraler (to ESP32-broer, eller ESP32 + raven BT-600 på benk). Arbitrerings-
   regelen «nyeste start vinner» må da bli **per enhet**, ikke global (notert i
   backend-koden allerede).

## Kandidatmetoder for syntetisk EKG (til vurdering, ikke valgt)

Fra enklest til mest ambisiøs:

1. **Justert overlegg (baseline).** R-topp-forankret tidsjustering, vis begge
   avledninger + en enkel kombinasjon (f.eks. RMS eller vektorstørrelse
   `sqrt(a² + b²)`). Rask, robust, gir en «alltid noe»-strimmel. God
   førsteversjon for å se om synk holder.
2. **Vektor-/pseudo-ortogonal kombinasjon.** Hvis de to avledningene er
   ~ortogonale, kan de behandles som to komponenter av hjertevektoren og gi en
   avlednings-uavhengig størrelse (retning mot en «beste» projeksjon).
   Mer meningsfull morfologi enn ren midling.
3. **Kvalitetsvektet fusjon.** Vekt hver kanal per øyeblikk etter SNR/
   ACC-bevegelse (vi har ACC per belte) – la det roligste/reneste beltet
   dominere når det andre er støyende. Bygger på ACC-gating vi allerede planla
   for RMSSD.
4. **Full 12-avlednings-rekonstruksjon (forskningsmål).** Lin.transform/ML fra
   to avledninger mot rekonstruerte standardavledninger – dette er det
   langsiktige forskningsmålet, ikke live-visning. Gjøres offline i
   `analysis/` mot korpuset, ikke i `ecgScope.ts`.

## Foreslått rekkefølge når nr. 2 er her

1. Få to samtidige rå-EKG stabilt inn (to broer/sentraler), vist som to
   strimler i RAW ECG. **Verifiser synk** (R-topp-forankret) på benk først.
2. Implementer metode 1 (justert overlegg + enkel kombinasjon) som den
   «syntetiske» strimmelen i RHYTHM/HRV. Vurder kvaliteten visuelt.
3. Hvis lovende: metode 2/3. Behold rå-strimlene alltid (aldri kast rådata).
4. Først når det syntetiske EKG-et er godt nok: revurder RR/RMSSD fra det (og
   om nativ-RR fortsatt skal være primær).

## UI-status nå

- RAW ECG: én strimmel, fast klinisk høyde, forberedt for å stable en strimmel
  nr. 2 (én per belte) uten omskriving.
- RAW ACC: egen fane (20.09).
- RHYTHM/HRV: uendret inntil videre. ~~Blir «syntetisk EKG»-visning~~ →
  besluttet 21.09: syntetisk EKG får **egen ny fane** (se under).

## Beslutninger 21.09.2026 (Jørn, chat 5)

Etter faktainnhenting (repo + litteratur) og gjennomgang punkt for punkt:

1. **Mål presisert: 2D, ikke 3D.** To belter gir to projeksjoner av
   hjertevektoren → en **plan vektorsløyfe (2D-VCG)**, ikke et fullt
   3D-bilde (det krever tre ~ortogonale avledninger, jf. Frank XYZ/EASI).
   Leveransen er «best mulig 2-avlednings-syntese + plan vektorsløyfe».
   Litteratur: rekonstruksjon fra to avledninger tenderer mot
   populasjonsgjennomsnitt (Frontiers-benchmark 2026, doi:10.3389/fcvm.2026.1856211;
   M2Eformer, doi:10.3390/bioengineering11030293).
2. **Oppstart = benk-deling, ett skritt om gangen:** belte A på ESP32-broen
   (iPhone-hotspot), belte B på BT-600/capture-agenten på raven.
   Innkjøringssekvens etter UI-finpussen: (a) kun A på ESP32, (b) kun B på
   BT-600, (c) begge samtidig. **Dual-på-én-ESP32 utsettes** til eget trinn
   når nytt Sense-kort er montert (coex-risiko + manglende SD-sikkerhetsnett
   på reservekortet).
3. **Latens akseptert** (sekund-nivå; R-topp-regresjonen trenger
   oppvarmingsvindu). Visualisering: EKG-grid/kurve tegnes «washed out» mot
   høyre kant og skarp der representasjonen anses god.
4. **Kalibreringsreferanse finnes:** nytt 12-avlednings-EKG av subjektet
   (2026-09-11, Østlandske Hjertesenter Moss; viser bl.a. T-inversjon og
   blokk). Overføres fra Jørns laptop til raven som
   `docs/private/2026-09-11-ekg-12-pkt.pdf` (**gitignorert – helsedata
   committes aldri**). Brukes etter beste evne til personlig
   transform-kalibrering.
5. **Synk-presisjonstak ~7,7 ms akseptert** (130 Hz sampletid; bekreftet av
   Polar i polar-ble-sdk issue #580 – SetLocalTime har kun 1 s oppløsning,
   enhetsklokker + R-topp-regresjon er riktig metode).
6. **Støytolerant HRV/RMSSD bygges på konsensus-slag** fra begge belter
   (toleransevindu + ACC/SQI-kvalitetsvekting, bSQI-stil), IKKE derivert fra
   den syntetiske strimmelen. Verktøykasse: WFDB XQRS (R-topp),
   NeuroKit2 (SQI/`ecg_quality`, Kubios-artefaktkorreksjon).
7. **Syntetisk strøm genereres kun** (visningslag) i utviklingsfasen;
   eventuell arkivering i MariaDB vurderes senere. Frame-skjemaet (frosset,
   v2) røres ikke.
8. **Belteplassering logges som øktmetadata fra dag én** (eksperimentering
   med plasseringer forventes). Gjeldende plan: **A** øvre belte, sensor
   vertikalt, senter ~5 cm under høyre brystvorte; **B** nedre belte rotert,
   senter ~8 cm under venstre brystvorte.
9. **Arbitrering per enhet er porten:** global «nyeste start vinner» i
   backend erstattes av per-enhet-arbitrering som beholder beskyttelsen
   per belte; regresjonstest med ett belte før to kilder slippes på.
10. **Ny egen fane** i elduro.no for syntetisk EKG / vektorsløyfe.
    *Revidert samme kveld (Jørns finpuss-dokument):* RHYTHM/HRV-fanen **er**
    den nye fanen - omdøpt til SYNTETISK EKG, tachogram/RMSSD består i den.
11. **Metode 1 levert 21.09 natt** (`backend/src/synth.rs` + replay-
    validering mot ekte dual-data: 100 % R-topp-match, ~3,2 ms residual,
    korrelasjon +0,85-0,94, drift ~-10 ppm). Fusjonen er 50/50-snitt med
    polaritetsvakt (EMA-korrelasjon); kvalitetsvekting (metode 3) og
    vektorsløyfe gjenstår.
