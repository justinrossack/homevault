# --- 1. Frontend bauen ---
FROM node:22-alpine AS web
WORKDIR /web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

# --- 2. Rust-Binary bauen (statisch, musl) ---
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /app
# Abhängigkeiten separat bauen, damit Docker sie cachen kann
COPY Cargo.toml Cargo.lock build.rs ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs && cargo build --release && rm -rf src
# Ortsliste für Foto-Ordner (wird ins Programm eingebettet)
COPY data ./data
COPY src ./src
COPY --from=web /web/dist ./web/dist
RUN touch src/main.rs && cargo build --release

# --- 3. Platten-Dienst (optional, für RAID/Spiegel; braucht Zugriff auf /dev) ---
FROM alpine:3.22 AS disks
RUN apk add --no-cache btrfs-progs smartmontools util-linux lsblk blkid tini
COPY --from=build /app/target/release/homevault /usr/local/bin/homevault
ENV DATA_DIR=/data STATE_DIR=/state PORT=8091
HEALTHCHECK --interval=10s --timeout=3s --start-period=5m CMD wget -qO- http://127.0.0.1:8091/health || exit 1
# -s: mit `pid: host` ist tini nicht PID 1, räumt aber trotzdem Zombies ab
ENTRYPOINT ["/sbin/tini", "-s", "--", "homevault", "disk-agent"]

# --- 4. Laufzeit-Image (Web-App) ---
FROM alpine:3.22
# Vorschau für Word/Excel/PowerPoint per LibreOffice (+ ca. 450 MB).
# Abschalten mit: docker compose build --build-arg OFFICE=false
ARG OFFICE=true
# tesseract: Texterkennung für gescannte Dokumente im Eingangs-Ordner
RUN apk add --no-cache ffmpeg libheif-tools rclone tzdata poppler-utils tini \
       tesseract-ocr tesseract-ocr-data-deu tesseract-ocr-data-eng \
    && if [ "$OFFICE" = "true" ]; then \
         apk add --no-cache libreoffice-writer libreoffice-calc libreoffice-impress font-liberation ttf-dejavu; \
       fi \
    && adduser -D -H -u 1000 homevault \
    && mkdir -p /data /cache && chmod 1777 /cache
COPY --from=build /app/target/release/homevault /usr/local/bin/homevault
ENV DATA_DIR=/data CACHE_DIR=/cache PORT=8080 HOME=/tmp
USER 1000:1000
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s CMD wget -qO- "http://127.0.0.1:${PORT}/api/health" || exit 1
ENTRYPOINT ["/sbin/tini", "--", "homevault"]
