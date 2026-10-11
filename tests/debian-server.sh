#!/usr/bin/env bash
# Baut einen frischen Debian-13-„Server“ (Docker in Docker) mit drei
# Test-Festplatten, installiert homevault genau nach README und führt den
# Ende-zu-Ende-Test aus.  Läuft innerhalb des Debian-Containers.
set -eu
log() { printf '\n==> %s\n' "$*"; }

log "Pakete installieren (Debian-Paketquellen)"
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq docker.io docker-compose docker-buildx curl jq smbclient sqlite3 ffmpeg util-linux openssl >/dev/null
docker --version; docker compose version

log "Docker-Dienst starten"
docker info >/dev/null 2>&1 || (dockerd >/var/log/dockerd.log 2>&1 &)
for _ in $(seq 1 30); do docker info >/dev/null 2>&1 && break; sleep 1; done

log "Aufräumen (falls hier schon einmal getestet wurde)"
for d in /opt/homevault /opt/nas2 /opt/restoretest; do
  [ -d "$d" ] && (cd "$d" && docker compose down -v --remove-orphans >/dev/null 2>&1)
done
umount /srv/homevault 2>/dev/null || true
for l in $(losetup -a | grep /srv/disks | cut -d: -f1); do losetup -d "$l"; done
rm -rf /srv/homevault /srv/nas2 /srv/restoretest /srv/disks
docker info --format 'Docker {{.ServerVersion}}, Speicher-Treiber {{.Driver}}'

log "Server vorbereiten: Ordner, Rechte, drei Test-Festplatten (je 2 GB)"
mount --make-rshared /
mkdir -p /srv/homevault /srv/nas2 /srv/restoretest /srv/disks
chown 1000:1000 /srv/homevault /srv/nas2 /srv/restoretest
for i in 1 2 3; do truncate -s 2G /srv/disks/d$i.img; losetup -f /srv/disks/d$i.img; done
losetup -a | grep /srv/disks

install_instance() { # Ordner, MEDIA_DIR, Webport, SMB-Port, Präfix, Platten?
  rm -rf "$1"; mkdir -p "$1"
  cp -a /opt/src/. "$1/"
  cd "$1"
  cp .env.example .env                                   # README-Schritt 1
  sed -i "s|^MEDIA_DIR=.*|MEDIA_DIR=$2|; s|^WEB_PORT=.*|WEB_PORT=$3|; s|^SMB_PORT=.*|SMB_PORT=$4|" .env
  echo "CONTAINER_PREFIX=$5" >> .env
  if [ "$6" = ja ]; then                                 # README: Festplatten-Verwaltung einschalten
    sed -i 's|^#COMPOSE_PROFILES=|COMPOSE_PROFILES=|; s|^#MEDIA_PROPAGATION=|MEDIA_PROPAGATION=|; s|^#DISK_AGENT_URL=|DISK_AGENT_URL=|' .env
    sed -i "s|^#DISK_AGENT_TOKEN=.*|DISK_AGENT_TOKEN=$(openssl rand -hex 24)|" .env
    echo "DISK_TEST_LOOP_DIR=/srv/disks" >> .env         # nur im Test: Loop-Geräte gelten als Platten
  fi
  # aus dem Quellcode bauen (statt fertige Images von GitHub zu laden)
  docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build 2>&1 | tail -3
}

log "homevault installieren (mit Festplatten-Verwaltung)"
install_instance /opt/homevault /srv/homevault 8080 445 homevault ja
log "Zweiter homevault als Backup-Ziel (Papas NAS)"
install_instance /opt/nas2 /srv/nas2 8090 1445 nas2 nein
log "Dritter, leerer Server für die Komplett-Wiederherstellung"
install_instance /opt/restoretest /srv/restoretest 8095 1447 restoretest nein

for p in 8080 8090 8095; do for _ in $(seq 1 90); do curl -sf localhost:$p/api/health >/dev/null && break; sleep 2; done; done
curl -s -H 'content-type: application/json' -d '{"server_name":"Papas NAS","name":"papa","password":"papa1234"}' localhost:8090/api/setup >/dev/null
docker ps --format '{{.Names}}: {{.Status}}'
docker images --format '{{.Repository}}: {{.Size}}' | grep -E '^homevault-'

GW=$(docker exec homevault sh -c "ip route | awk '/default/ {print \$3}'")
log "Ende-zu-Ende-Test (Backup-Ziel: $GW:1445)"
cd /opt/homevault
B=http://127.0.0.1:8080 ASSETS=/opt/homevault/tests/assets SRC=/opt/homevault NAS2_HOST=$GW NAS2_PORT=1445 \
  RESTORE_URL=http://127.0.0.1:8095 bash tests/e2e.sh 2>&1 | tee /root/e2e.log
