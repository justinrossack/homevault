#!/usr/bin/env bash
# homevault installieren oder aktualisieren (Debian/Ubuntu).
#
#   curl -fsSL https://raw.githubusercontent.com/justinrossack/homevault/main/install.sh | sudo bash
#
# Erster Aufruf: fragt Datenordner und Festplatten-Spiegel ab, legt alles an
# und startet homevault. Jeder weitere Aufruf aktualisiert auf die neueste
# Version; Daten und Einstellungen bleiben unverändert.
set -euo pipefail

REPO="justinrossack/homevault"
BRANCH="${HOMEVAULT_BRANCH:-main}"
DIR="${HOMEVAULT_DIR:-/opt/homevault}"
RAW="https://raw.githubusercontent.com/$REPO/$BRANCH"

say() { printf '\n\033[1;34m==>\033[0m %s\n' "$*"; }
die() { printf '\n\033[1;31mFehler:\033[0m %s\n' "$*" >&2; exit 1; }
# Fragen auch beim Aufruf über "curl | bash" an das Terminal stellen
ask() {
  local question=$1 default=$2 answer
  if [ -r /dev/tty ]; then
    read -r -p "$question [$default]: " answer </dev/tty || true
  fi
  printf '%s' "${answer:-$default}"
}

[ "$(id -u)" -eq 0 ] || die "Bitte mit sudo ausführen."

# --- Docker ------------------------------------------------------------
# Compose v2 heißt je nach System anders: Debian 13 "docker-compose",
# Ubuntu "docker-compose-v2". Debian 12 hat nur das alte v1 – dann Dockers
# eigene Paketquelle.
compose_pkg() {
  local v
  if apt-cache show docker-compose-v2 >/dev/null 2>&1; then echo docker-compose-v2; return; fi
  v=$(apt-cache policy docker-compose 2>/dev/null | awk '/Candidate:/ {print $2}')
  case "$v" in 2.*) echo docker-compose ;; esac
}
install_docker() {
  apt-get update -qq
  apt-get install -y -qq curl openssl >/dev/null
  if dpkg -s docker-ce >/dev/null 2>&1; then
    # Docker aus Dockers Paketquelle ist schon da, nur Compose fehlt
    apt-get install -y -qq docker-compose-plugin >/dev/null
    return
  fi
  local pkg
  pkg=$(compose_pkg)
  if [ -n "$pkg" ]; then
    apt-get install -y -qq docker.io "$pkg" >/dev/null
  else
    say "Docker wird über Dockers eigene Paketquelle installiert (get.docker.com)"
    curl -fsSL https://get.docker.com | sh
  fi
}
if ! command -v docker >/dev/null 2>&1 || ! docker compose version >/dev/null 2>&1; then
  say "Docker mit Compose fehlt"
  [ "$(ask 'Docker jetzt installieren? (j/n)' j)" = j ] || die "Docker wird benötigt."
  install_docker
  systemctl enable --now docker >/dev/null 2>&1 || true
  docker compose version >/dev/null 2>&1 \
    || die "Docker Compose v2 ist nach der Installation nicht verfügbar. Bitte nach https://docs.docker.com/engine/install/ installieren und das Skript erneut starten."
fi

mkdir -p "$DIR"
cd "$DIR"

download() { curl -fsSL "$RAW/$1" -o "$1.neu" && mv "$1.neu" "$1"; }

# --- Aktualisieren -----------------------------------------------------
if [ -f .env ]; then
  say "Bestehende Installation in $DIR wird aktualisiert"
  cp docker-compose.yml "docker-compose.yml.vorher" 2>/dev/null || true
  download docker-compose.yml
  download .env.example
  docker compose pull
  docker compose up -d --remove-orphans
  docker image prune -f >/dev/null
  say "Fertig. homevault läuft in der neuesten Version."
  docker compose ps --format '  {{.Service}}: {{.Status}}'
  exit 0
fi

# --- Neu installieren --------------------------------------------------
say "homevault wird in $DIR installiert"
download docker-compose.yml
download .env.example
# Die .env erst am Ende an ihren Platz legen: bricht etwas ab, startet der
# nächste Aufruf wieder als Neuinstallation (statt als Aktualisierung)
ENV_NEW=.env.neu
cp .env.example "$ENV_NEW"
chmod 600 "$ENV_NEW"

MEDIA=$(ask "Ordner für Fotos und Dateien" "/srv/homevault")
case "$MEDIA" in
  /*) ;;
  *) die "Bitte einen absoluten Pfad angeben (beginnt mit /), z. B. /srv/homevault" ;;
esac
mkdir -p "$MEDIA"
# Besitzer des Datenordners (Web-App und Netzlaufwerk laufen als dieser Benutzer)
PUID=$(stat -c %u "$MEDIA"); PGID=$(stat -c %g "$MEDIA")
if [ "$PUID" -eq 0 ]; then
  PUID=1000; PGID=1000
  chown "$PUID:$PGID" "$MEDIA"
fi
sed -i "s|^MEDIA_DIR=.*|MEDIA_DIR=$MEDIA|; s|^PUID=.*|PUID=$PUID|; s|^PGID=.*|PGID=$PGID|" "$ENV_NEW"

LISTEN_445=$(ss -Hltn 'sport = :445' 2>/dev/null || true)
if [ -n "$LISTEN_445" ]; then
  echo "  Hinweis: Port 445 ist schon belegt (anderer Samba?). Das Netzlaufwerk nutzt deshalb Port 1445."
  sed -i "s|^SMB_PORT=.*|SMB_PORT=1445|" "$ENV_NEW"
fi

if [ "$(ask 'Festplatten-Verwaltung (Spiegel/RAID, Hot-Swap) einschalten? (j/n)' n)" = j ]; then
  sed -i 's|^#COMPOSE_PROFILES=|COMPOSE_PROFILES=|; s|^#MEDIA_PROPAGATION=|MEDIA_PROPAGATION=|; s|^#DISK_AGENT_URL=|DISK_AGENT_URL=|' "$ENV_NEW"
  sed -i "s|^#DISK_AGENT_TOKEN=.*|DISK_AGENT_TOKEN=$(openssl rand -hex 24)|" "$ENV_NEW"
  # Der Speicher wird über MEDIA_DIR eingehängt und muss für die Container sichtbar sein
  mount --make-rshared / 2>/dev/null || true
fi

if [ "$(ask 'Lokale KI für den Eingangs-Ordner einschalten (Dokumente benennen/sortieren, braucht ca. 4 GB RAM)? (j/n)' n)" = j ]; then
  if grep -q '^COMPOSE_PROFILES=' "$ENV_NEW"; then
    sed -i 's|^COMPOSE_PROFILES=\(.*\)|COMPOSE_PROFILES=\1,ai|' "$ENV_NEW"
  else
    echo "COMPOSE_PROFILES=ai" >> "$ENV_NEW"
  fi
  sed -i 's|^#AI_URL=|AI_URL=|' "$ENV_NEW"
fi

say "Images laden"
# mit der neuen Konfiguration laden, bevor sie gilt
docker compose --env-file "$ENV_NEW" pull
mv "$ENV_NEW" .env

say "homevault starten"
docker compose up -d

IP=$(hostname -I 2>/dev/null | awk '{print $1}')
say "Fertig!"
cat <<EOF

  Öffne im Browser:   http://${IP:-<server-ip>}:$(grep -E '^WEB_PORT=' .env | cut -d= -f2)
  Dort startet der Einrichtungsassistent (Admin-Konto, Festplatten, Benutzer).

  Aktualisieren:      denselben Befehl noch einmal ausführen
  Einstellungen:      $DIR/.env
  Daten:              $MEDIA

EOF
