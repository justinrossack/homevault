# homevault

Schnelle, einfache Foto-, Video- und Datei-Verwaltung für den Heimserver – eine Art Mini-Synology. Ein Rust-Backend, eine Material-UI-Weboberfläche, Docker.

## Funktionen

**Fotos & Videos**
- Ordner durchsuchen, per Drag & Drop hochladen, Rahmenauswahl mit der Maus, ⌘A/Strg+A, Esc
- **Zeitleiste** nach Aufnahmedatum (EXIF/Video-Metadaten), **Favoriten**, **Alben** (ohne Kopien)
- **iPhone**: HEIC wird angezeigt, **Live Photos** (`IMG_1234.HEIC` + `.MOV`) spielen beim Öffnen kurz ab
- **Videos für alle Browser**: HEVC (auch HDR) wird im Hintergrund nach H.264 umgewandelt
- **Teilen-Links** für Leute ohne Konto, optional mit Passwort, Ablaufdatum, Download-Sperre

**Dateien & Dokumente**
- Vorschau für **PDF**, **Word/Excel/PowerPoint/OpenDocument** (per LibreOffice), Text, CSV (als Tabelle), Audio
- **ZIP/TAR-Inhalt** ansehen und einzelne Dateien daraus laden
- Nicht unterstützte Typen: klarer Hinweis mit Download
- **Papierkorb** (30 Tage, einstellbar), auch für Ordner; Wiederherstellen inkl. Alben/Favoriten

**Benutzer & Zugriff**
- **Einrichtungsassistent** beim ersten Start (Servername, Admin, Festplatten, Benutzer)
- Jeder Benutzer hat **Privat** (nur für ihn), alle teilen **Gemeinsam**; Speicherlimits pro Benutzer
- **Netzlaufwerk (SMB)** mit denselben Logins: Windows-Explorer, macOS Finder, iPhone-Dateien-App

**Automatisch sortieren & synchronisieren**
- **Eingangs-Ordner**: Was in `Eingang` landet (Browser, Netzlaufwerk, Scanner), wird einsortiert und lässt sich mit einem Klick rückgängig machen
  - Fotos und Videos nach Aufnahmedatum und Ort: `Fotos/2025/2025-08 Lissabon/` (Orte offline erkannt, ohne KI)
  - Dokumente benennt eine **lokale KI**: `Dokumente/Rechnungen/2026-03-12 Stadtwerke München Stromrechnung.pdf`. Gescannte Briefe werden per Texterkennung gelesen, nichts verlässt das Haus
- **Live-Sync** auf andere NAS im Haus: Regeln wie „Gemeinsam/Familie → Papas NAS“ oder „Privat von anna → Keller-NAS“, etwa eine Minute nach jeder Änderung, alte Stände bleiben unter `.versionen/`

**Datensicherheit**
- **Festplatten-Spiegel (RAID) mit Btrfs**: Platten im Browser auswählen, Spiegel anlegen, Zustand und SMART-Werte sehen, defekte Platte **im laufenden Betrieb tauschen** (Hot-Swap), erweitern, monatliche Datenprüfung (repariert stille Fehler)
- **Backup auf andere NAS** im Haus per SMB, mit Zeitplan und Versionen (`.versionen/`)
- **Wiederherstellen**: einzelne Dateien/Ordner (auch ältere Versionen) oder **komplett auf einem neuen Server** direkt aus dem Einrichtungsassistenten

> RAID schützt vor einem Plattendefekt, **nicht** vor Löschen, Erpressungstrojanern, Feuer oder Diebstahl. Dafür ist das Backup da. Beides zusammen ist die empfohlene Kombination.

## Installation (Debian/Ubuntu)

```bash
curl -fsSL https://raw.githubusercontent.com/justinrossack/homevault/main/install.sh | sudo bash
```

Für Debian 12/13 und Ubuntu 24.04. Das Skript installiert bei Bedarf Docker mit Compose v2, fragt nach dem Datenordner und ob der Festplatten-Spiegel eingeschaltet werden soll, lädt die fertigen Images (x86_64 und ARM64) und startet alles. Dann im Browser `http://<server-ip>:8080` öffnen – der Einrichtungsassistent startet.

**Aktualisieren:** denselben Befehl noch einmal ausführen (oder in `/opt/homevault`: `docker compose pull && docker compose up -d`). Daten und Einstellungen bleiben erhalten. Eine bestimmte Version festhalten: `HOMEVAULT_VERSION=1.2.3` in der `.env`.

<details>
<summary>Manuell installieren</summary>

```bash
sudo mkdir -p /opt/homevault /srv/homevault && cd /opt/homevault
sudo curl -fsSLO https://raw.githubusercontent.com/justinrossack/homevault/main/docker-compose.yml
sudo curl -fsSL https://raw.githubusercontent.com/justinrossack/homevault/main/.env.example -o .env
sudo nano .env                            # MEDIA_DIR, PUID/PGID (siehe unten)
sudo chown 1000:1000 /srv/homevault
sudo docker compose up -d
```
</details>

- `MEDIA_DIR`: Ordner für alle Daten, z. B. `/srv/homevault`
- `PUID`/`PGID`: Besitzer dieses Ordners (`id -u`, `id -g`). Stimmt das nicht, meldet homevault beim Start, wie es zu beheben ist.
- Port 445 (SMB) darf nicht von einem anderen Samba belegt sein.
- **Nicht** direkt ins Internet stellen. Für unterwegs: WireGuard/Tailscale oder Reverse-Proxy mit HTTPS.

### Festplatten-Spiegel (RAID) einschalten

In der `.env` die Zeilen unter „Festplatten-Verwaltung“ einkommentieren und ein eigenes Token setzen:

```bash
COMPOSE_PROFILES=disks
MEDIA_PROPAGATION=rslave
DISK_AGENT_URL=http://disks:8091
DISK_AGENT_TOKEN=<openssl rand -hex 24>
```

`docker compose up -d` – danach in den Einstellungen unter **Festplatten** (oder im Assistenten) die Platten auswählen. Dabei gilt:
- Systemplatten werden erkannt und sind gesperrt; vor dem Formatieren muss der Plattenname eingetippt werden.
- Vorhandene Daten aus `MEDIA_DIR` werden auf den Spiegel kopiert, homevault startet danach einmal neu.
- Der Platten-Dienst läuft als eigener Container mit Zugriff auf `/dev`; die Weboberfläche selbst hat keine Sonderrechte.
- Fehlt beim Start eine Platte, startet der Server im **Notbetrieb**; die Oberfläche warnt und bietet „Ersetzen“ an.
- Lässt sich der Spiegel gar nicht einhängen (zu viele Platten fehlen), startet die Oberfläche **schreibgeschützt** mit Warnung, das Netzlaufwerk bleibt aus. Sobald die Platten wieder da sind, hängt der Platten-Dienst den Spiegel ein und homevault startet normal neu.
- Während der Spiegel eingerichtet wird, ist das Netzlaufwerk kurz pausiert.
- `MEDIA_DIR` muss auf einem „shared mount“ liegen (bei Debian/Ubuntu mit systemd ist `/` das automatisch).

### Ohne LibreOffice (kleineres Image)

Die Office-Vorschau braucht ca. 1 GB im Image. Wer darauf verzichtet, baut selbst: `docker compose -f docker-compose.yml -f docker-compose.build.yml build --build-arg OFFICE=false`.

Selbst gebaute Images nicht mit `install.sh` oder `docker compose pull` aktualisieren (das lädt wieder das offizielle Image), sondern: `git pull` und denselben `build`-Befehl erneut, dann `docker compose up -d`.

### KI für den Eingangs-Ordner

In der `.env` bei `COMPOSE_PROFILES` „ai“ ergänzen (z. B. `COMPOSE_PROFILES=disks,ai`) und `AI_URL=http://ai:11434` einkommentieren, dann `docker compose up -d`. Beim ersten Start lädt homevault das Sprachmodell (Standard `qwen2.5:3b`, ca. 2 GB). Den Fortschritt zeigt die Seite **Eingang**.
- Läuft komplett lokal (Ollama), ohne Grafikkarte. Auf einem kleinen Server dauert ein Dokument etwa 30–90 Sekunden. Das passiert im Hintergrund mit niedriger Priorität.
- Ohne KI werden Fotos trotzdem sortiert, Dokumente bleiben im Eingang liegen.
- Scanner am besten als PDF in `Gemeinsam/Eingang` oder `Privat/<name>/Eingang` speichern lassen (SMB).

## Ordnerstruktur

```
MEDIA_DIR/
├─ Gemeinsam/          für alle Benutzer
├─ Privat/<name>/      nur für diesen Benutzer
├─ .papierkorb/        gelöschte Dateien, nicht freigegeben
└─ .homevault/         homevault.db (SQLite) und smbpasswd, nur für den Besitzer lesbar
```

Fotos bleiben normale Dateien. Die Datenbank enthält Benutzer, Sitzungen, Einstellungen, den Medien-Index, Favoriten, Alben, Teilen-Links, Papierkorb- und Backup-Einträge.

## Backup & Wiederherstellen

Einstellungen → **Backup**: Ziel anlegen (Server, Freigabe, Ordner, Benutzer, Passwort), „Verbindung testen“, Zeitplan wählen. Auf dem Ziel entsteht:

```
<freigabe>/<ordner>/
├─ Gemeinsam/, Privat/<name>/
├─ Datenbank/homevault.db               Schnappschuss
└─ .versionen/2026-10-11_030000/        gelöschte/überschriebene Dateien dieses Laufs
```

- **Einzelne Dateien zurückholen:** beim Ziel auf „Wiederherstellen“ – Stand wählen (aktuell oder älter), Dateien/Ordner auswählen, an den Originalort (nichts wird überschrieben) oder in einen neuen Ordner „Wiederhergestellt …“.
- **Neuer Server nach Totalausfall:** homevault frisch installieren, im Assistenten „Aus Backup wiederherstellen“ wählen und das Backup-NAS angeben. Benutzer, Alben, Einstellungen und Dateien kommen zurück.

## Live-Sync auf andere NAS

1. Einstellungen → **Backup**: das NAS als Ziel anlegen (für reinen Sync ohne Bereiche und mit Zeitplan „manuell“). Optional „Benutzer dürfen ihren Privat-Ordner hierher synchronisieren“.
2. Einstellungen → **Synchronisieren**: Regel „Quelle → Ziel“ anlegen. Admins wählen Quelle (Gemeinsam oder Privat eines Benutzers, optional Unterordner) und Zielordner. Benutzer wählen nur für ihren eigenen Privat-Ordner ein freigegebenes NAS, das Ziel ist dann fest `<ordner>/Benutzer/<name>`.

Nur homevault → NAS. Auf dem NAS Gelöschtes oder Überschriebenes wandert nach `<ziel>/.versionen/<Zeitpunkt>/`. Zwei Regeln (oder Regel und Backup) können nie in denselben Ordner schreiben, und liegen im Zielordner schon fremde Dateien, fragt homevault vorher nach.

### Backup/Sync über einen zweiten Netzwerkadapter

Pro NAS lässt sich unter Einstellungen → Backup ein **Netzwerkadapter** wählen, z. B. ein zweiter (USB-)LAN-Anschluss. Dann laufen Backup, Wiederherstellen und Live-Sync darüber, Uploads und Videos weiter über den ersten Anschluss. Die Web-App läuft dafür im Netzwerk des Servers (`network_mode: host`).

Hängen beide Adapter im **selben Netz**, braucht der Server eine feste Adresse für den zweiten Adapter und eine Routing-Regel, sonst nimmt Linux immer nur einen Adapter. Beispiel für Debian (`/etc/network/interfaces.d/zweiter-adapter`, Adapter `enx…`, Adresse außerhalb des DHCP-Bereichs der FritzBox):

```
allow-hotplug enx0000cd2316bf
iface enx0000cd2316bf inet manual
    up ip link set dev $IFACE up
    up ip addr add 192.168.178.250/24 dev $IFACE noprefixroute
    up ip route replace 192.168.178.0/24 dev $IFACE src 192.168.178.250 table 100
    up ip route replace default via 192.168.178.1 dev $IFACE onlink table 100
    up ip rule add from 192.168.178.250 lookup 100 priority 100
    down ip rule del from 192.168.178.250 lookup 100 priority 100 || true
    down ip addr flush dev $IFACE
```

Dazu `net.ipv4.conf.all.arp_ignore=1` und `net.ipv4.conf.all.arp_announce=2` (z. B. in `/etc/sysctl.d/`). Ohne Kabel am gewählten Adapter läuft das Backup über den normalen Weg weiter.

## Live Photos vom iPhone

Browser-Uploads vom iPhone enthalten nur das Foto. Für komplette Live Photos über das Netzlaufwerk speichern (Dateien-App) oder am Mac „Unbearbeitetes Original exportieren“.

## Veröffentlichen

GitHub Actions baut bei jedem Push auf `main` die Images für x86_64 und ARM64 und lädt sie nach `ghcr.io/justinrossack/homevault`, `…/homevault-disks` und `…/homevault-samba` (Tag `latest`). Eine Version veröffentlichen: `git tag v1.0.0 && git push --tags` (Tags `1.0.0`, `1.0`, `1`).

Einmalig nach dem ersten Lauf: auf GitHub unter **Packages** die drei Pakete öffnen → *Package settings* → *Change visibility* → **Public**. Neue Pakete sind sonst privat und `docker compose pull` schlägt bei allen anderen fehl.

## Tests

- `cargo test` – Unit-Tests (Pfadschutz, Index, Backup-Zeitplan, Archive …)
- `tests/e2e.sh` – Ende-zu-Ende-Test aller Funktionen auf einem laufenden Server
- `tests/debian-server.sh` – baut einen frischen Debian-Server mit Test-Platten, installiert nach dieser README und führt `e2e.sh` aus. Ergebnis: [docs/TESTBERICHT.md](docs/TESTBERICHT.md)

## Entwicklung

Aus dem Quellcode bauen statt fertige Images zu laden:

```bash
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

Ohne Docker:

```bash
DATA_DIR=./testdata CACHE_DIR=./testcache cargo run      # Backend
cd web && npm install && npm run dev                     # Frontend mit Hot Reload
```

Lokal für HEIC, Videos, Dokumente und Backup: `brew install libheif ffmpeg poppler rclone`.

## Konfiguration

| Variable | Standard | Bedeutung |
|---|---|---|
| `MEDIA_DIR` | – | Datenordner auf dem Server (Compose) |
| `PUID`, `PGID` | `1000` | Benutzer, unter dem Web-App und SMB auf Dateien zugreifen |
| `WEB_PORT`, `SMB_PORT` | `8080`, `445` | Ports auf dem Server |
| `TZ` | `Europe/Berlin` | Zeitzone für Backup-Zeitpläne |
| `INDEX_INTERVAL_MINUTES` | `10` | Wie oft der Medien-Index nach Änderungen sucht |
| `COMPOSE_PROFILES`, `MEDIA_PROPAGATION`, `DISK_AGENT_URL`, `DISK_AGENT_TOKEN` | – | Festplatten-Verwaltung (siehe oben) |
| `AI_URL`, `AI_MODEL` | –, `qwen2.5:3b` | Lokale KI für den Eingangs-Ordner (siehe oben) |
| `ADMIN_USER`, `ADMIN_PASSWORD` | `admin`, – | Optional: ersten Admin ohne Assistent anlegen (nur wenn noch kein Benutzer existiert) |

Ortsdaten: [GeoNames](https://www.geonames.org/), CC BY 4.0 (siehe `data/README.md`).
