#!/usr/bin/env bash
# Ende-zu-Ende-Test für homevault auf einem frisch installierten Server.
#
# Voraussetzungen auf dem Server: homevault läuft per `docker compose up`
# (mit Festplatten-Verwaltung), drei freie Test-Platten, ein zweiter
# homevault als Backup-Ziel, sowie curl, jq, smbclient, sqlite3.
#
# Aufruf:  tests/e2e.sh            (Ausgabe: eine Zeile pro Test)
set -u

B=${B:-http://127.0.0.1:8080}
A=${ASSETS:-$(cd "$(dirname "$0")" && pwd)/assets}
MEDIA=${MEDIA:-/srv/homevault}
SRC=${SRC:-/opt/homevault}
NAS2_HOST=${NAS2_HOST:-}
NAS2_PORT=${NAS2_PORT:-1445}
T=$(mktemp -d)
ADM=$T/admin.cj USR=$T/lisa.cj GST=$T/gast.cj
J='content-type: application/json'
PASSED=0 FAILED=0

ok() { PASSED=$((PASSED + 1)); printf 'BESTANDEN | %-4s | %s\n' "$1" "$2"; }
nok() { FAILED=$((FAILED + 1)); printf 'FEHLER    | %-4s | %s | %s\n' "$1" "$2" "$3"; }
# t ID "Beschreibung" befehl…  -> Test bestanden, wenn der Befehl erfolgreich ist
t() {
  local id=$1 desc=$2 out
  shift 2
  if out=$("$@" 2>&1); then ok "$id" "$desc"; else nok "$id" "$desc" "$(echo "$out" | tail -1 | tr -d '\r' | cut -c1-200)"; fi
}
section() { printf '\n## %s\n' "$1"; }

# Hilfen
get() { curl -s -b "$1" -c "$1" "$B$2"; }
post() { curl -s -b "$1" -c "$1" -H "$J" -d "$3" "$B$2"; }
code() { curl -s -o /dev/null -w '%{http_code}' "$@"; }
is() { [ "$1" = "$2" ] || { echo "erwartet '$2', bekommen '$1'"; return 1; }; }
has() { printf '%s' "$1" | grep -q -- "$2" || { echo "'$2' fehlt in: $(printf '%s' "$1" | head -c 200)"; return 1; }; }
jqt() { printf '%s' "$1" | jq -e "$2" >/dev/null || { echo "Bedingung $2 nicht erfüllt: $(printf '%s' "$1" | head -c 300)"; return 1; }; }
until_ok() { local n=$1; shift; for _ in $(seq 1 "$n"); do "$@" >/dev/null 2>&1 && return 0; sleep 2; done; echo "Zeitüberschreitung"; return 1; }
upload() { curl -s -b "$1" -F "f=@$2;filename=${4:-$(basename "$2")}" "$B/api/upload?path=$3"; }
smb() { smbclient "//127.0.0.1/$1" -p "${SMB_PORT:-445}" -U "$2" -c "$3" 2>&1; }
compose() { (cd "$SRC" && docker compose "$@"); }
disks() { get "$ADM" /api/disks; }
job_done() { disks | jq -e '.job == null and (.pool.replace.running | not) and (.pool.balance.running | not)' >/dev/null; }
health() { curl -sf "$B/api/health" >/dev/null; }

# Hilfsfunktionen und Variablen auch in Unter-Shells (bash -c) verfügbar machen
export -f get post code is has jqt until_ok upload smb compose disks job_done health
export B A MEDIA SRC J T ADM USR GST

section "A Installation"
t A1 "Alle Container laufen" bash -c "compose ps --format '{{.Name}} {{.State}}' | grep -v running | wc -l | grep -qx 0"
t A2 "Web-App antwortet (Gesundheitscheck)" until_ok 30 health
t A3 "Weboberfläche wird ausgeliefert" bash -c "curl -s $B/ | grep -q '<div id=\"root\">'"
t A4 "Platten-Dienst ist gesund" bash -c "compose ps disks --format '{{.Status}}' | grep -q healthy"

section "B Ersteinrichtung"
ME=$(get "$GST" /api/me)
t B1 "Server meldet: Einrichtung nötig" jqt "$ME" '.setup_needed == true'
t B2 "Zu kurzes Passwort wird abgelehnt" has "$(post "$ADM" /api/setup '{"server_name":"Testserver","name":"admin","password":"x"}')" "mindestens"
t B3 "Admin-Konto wird angelegt und angemeldet" has "$(post "$ADM" /api/setup '{"server_name":"Testserver","name":"Admin","password":"admin1234"}')" '"ok":true'
t B4 "Benutzername wird kleingeschrieben" jqt "$(get "$ADM" /api/me)" '.user.name == "admin" and .user.admin == true'
t B5 "Einrichtung ist danach gesperrt" is "$(code -H "$J" -d '{"name":"hacker","password":"12345678"}' $B/api/setup)" 403
t B6 "Servername gespeichert" jqt "$(get "$GST" /api/me)" '.server_name == "Testserver"'

section "N Festplatten (Spiegel/RAID)"
D=$(disks)
t N1 "Platten werden erkannt (3 freie Test-Platten)" jqt "$D" '[.disks[] | select(.usable)] | length >= 3'
t N2 "Systemplatte ist gesperrt" jqt "$D" '[.disks[] | select(.usable | not) | select(.reason != null)] | length >= 1'
L1=$(echo "$D" | jq -r '[.disks[] | select(.usable)][0].name')
L2=$(echo "$D" | jq -r '[.disks[] | select(.usable)][1].name')
L3=$(echo "$D" | jq -r '[.disks[] | select(.usable)][2].name')
t N3 "Ohne Namensbestätigung wird nichts formatiert" has "$(post "$ADM" /api/disks/create "{\"devices\":[\"/dev/$L1\",\"/dev/$L2\"],\"profile\":\"raid1\"}")" "Bestätigung"
upload "$ADM" "$A/foto.jpg" Gemeinsam vor-dem-spiegel.jpg >/dev/null
t N4 "Spiegel aus 2 Platten wird angelegt" has "$(post "$ADM" /api/disks/create "{\"devices\":[\"/dev/$L1\",\"/dev/$L2\"],\"profile\":\"raid1\",\"confirm\":[\"$L1\",\"$L2\"]}")" '"ok":true'
t N5 "Schreiben während des Umzugs gesperrt (Wartung)" has "$(post "$ADM" /api/folder '{"path":"Gemeinsam/x"}')" "eingerichtet"
sleep 5
t N6 "Web-App startet auf dem Spiegel neu" until_ok 60 bash -c "curl -sf $B/api/health >/dev/null && curl -s -b $ADM $B/api/disks | jq -e '.pool.mounted == true' >/dev/null"
t N7 "Anmeldung bleibt erhalten (Daten mit umgezogen)" jqt "$(get "$ADM" /api/me)" '.authenticated == true'
t N8 "Vorhandene Dateien mit umgezogen" has "$(get "$ADM" '/api/list?path=Gemeinsam')" vor-dem-spiegel.jpg
t N9 "Server sieht Btrfs-Spiegel unter MEDIA_DIR" bash -c "findmnt -no FSTYPE $MEDIA | grep -qx btrfs"
t N10 "Profil RAID1, 2 Platten, kein Notbetrieb" jqt "$(disks)" '.pool.data_profile == "raid1" and (.pool.devices | length) == 2 and .pool.degraded == false'
t N11 "Zustand für Seitenleiste: ok" jqt "$(get "$ADM" /api/disks/summary)" '.state == "ok"'

section "C Anmeldung & Sitzungen"
t C1 "Falsches Passwort wird abgelehnt" is "$(code -H "$J" -d '{"username":"admin","password":"falsch"}' $B/api/login)" 400
t C2 "Ohne Anmeldung kein Zugriff auf Dateien" is "$(code "$B/api/list?path=Gemeinsam")" 401
t C3 "Session-Cookie ist HttpOnly + SameSite" bash -c "curl -s -D - -o /dev/null -H '$J' -d '{\"username\":\"admin\",\"password\":\"admin1234\"}' $B/api/login | grep -i set-cookie | grep -q 'HttpOnly; SameSite=Strict'"

section "D Benutzerverwaltung"
t D1 "Benutzer anlegen" has "$(post "$ADM" /api/users '{"name":"lisa","password":"lisa1234"}')" '"ok":true'
t D2 "Ungültiger Name wird abgelehnt" has "$(post "$ADM" /api/users '{"name":"../root","password":"x12345"}')" "Benutzername"
t D3 "Doppelter Name wird abgelehnt" has "$(post "$ADM" /api/users '{"name":"lisa","password":"lisa1234"}')" "existiert"
t D4 "Neuer Benutzer kann sich anmelden" has "$(post "$USR" /api/login '{"username":"lisa","password":"lisa1234"}')" '"ok":true'
t D5 "Normaler Benutzer darf keine Benutzer verwalten" is "$(code -b $USR $B/api/users)" 403
t D6 "Privater Ordner wurde angelegt" bash -c "test -d $MEDIA/Privat/lisa"
t D7 "Speicherlimit setzen (1 MB)" has "$(curl -s -b $ADM -X PATCH -H "$J" -d '{"quota_bytes":1048576}' $B/api/users/lisa)" '"ok":true'
t D8 "Upload über dem Limit wird abgelehnt" has "$(upload "$USR" "$A/iphone-video.mov" Privat)" "Speicherlimit"
curl -s -b $ADM -X PATCH -H "$J" -d '{"quota_bytes":0}' $B/api/users/lisa >/dev/null
t D9 "Eigenes Passwort ändern" has "$(post "$USR" /api/me/password '{"current":"lisa1234","new":"lisa5678"}')" '"ok":true'
t D10 "Altes Passwort gilt nicht mehr" is "$(code -H "$J" -d '{"username":"lisa","password":"lisa1234"}' $B/api/login)" 400

section "E Dateien"
t E1 "Foto hochladen (Gemeinsam)" has "$(upload "$ADM" "$A/foto.jpg" Gemeinsam)" "Gemeinsam/foto.jpg"
t E2 "Doppelter Name bekommt (1)" has "$(upload "$ADM" "$A/foto.jpg" Gemeinsam)" "foto (1).jpg"
t E3 "Ordner anlegen" has "$(post "$ADM" /api/folder '{"path":"Gemeinsam/Urlaub"}')" '"ok":true'
t E4 "Umbenennen" has "$(curl -s -b $ADM -X PATCH -H "$J" -d '{"from":"Gemeinsam/foto (1).jpg","to":"Gemeinsam/Urlaub/strand.jpg"}' $B/api/rename)" '"ok":true'
t E5 "Ordnerinhalt auflisten" has "$(get "$ADM" '/api/list?path=Gemeinsam/Urlaub')" strand.jpg
t E6 "Teilweises Laden (Range, für Videos)" is "$(code -b $ADM -H 'Range: bytes=0-99' "$B/api/file?path=Gemeinsam/foto.jpg")" 206
t E7 "Download mit Dateinamen" bash -c "curl -s -D - -o /dev/null -b $ADM '$B/api/file?path=Gemeinsam/foto.jpg&download=true' | grep -qi 'content-disposition: attachment'"
t E8 "Ausbruch mit ../ wird blockiert" has "$(get "$ADM" '/api/list?path=Gemeinsam/../../etc')" "Ungültiger Pfad"
upload "$USR" "$A/notizen.md" Privat >/dev/null
t E9 "Fremder privater Ordner nicht sichtbar" bash -c "! curl -s -b $ADM '$B/api/list?path=Privat' | grep -q notizen.md"
t E10 "Hochgeladene HTML-Datei läuft in Sandbox" bash -c "upload $ADM $A/boese.html Gemeinsam >/dev/null; curl -s -D - -o /dev/null -b $ADM '$B/api/file?path=Gemeinsam/boese.html' | grep -qi 'content-security-policy: sandbox'"

post "$ADM" /api/folder '{"path":"Gemeinsam/Gleichzeitig"}' >/dev/null
for _ in 1 2 3 4; do upload "$ADM" "$A/foto.jpg" Gemeinsam/Gleichzeitig >/dev/null & done; wait
t E11 "Gleichzeitige Uploads gleichen Namens: keiner überschreibt" jqt "$(get "$ADM" '/api/list?path=Gemeinsam/Gleichzeitig')" '[.entries[] | select(.kind == "image")] | length == 4'

section "F Fotos & Videos"
t F1 "Vorschaubild (JPEG)" bash -c "curl -s -b $ADM -o $T/t.jpg -w '%{content_type}' '$B/api/thumb?path=Gemeinsam/foto.jpg' | grep -q image/jpeg && test \$(stat -c%s $T/t.jpg) -gt 1000"
upload "$ADM" "$A/IMG_0001.HEIC" Gemeinsam/Urlaub >/dev/null
upload "$ADM" "$A/IMG_0001.MOV" Gemeinsam/Urlaub >/dev/null
L=$(get "$ADM" '/api/list?path=Gemeinsam/Urlaub')
t F2 "HEIC wird erkannt (Vorschau nötig)" jqt "$L" '.entries[] | select(.name == "IMG_0001.HEIC") | .preview == true'
t F3 "HEIC-Vorschau als JPEG" bash -c "curl -s -b $ADM -o /dev/null -w '%{http_code} %{content_type}' '$B/api/preview?path=Gemeinsam/Urlaub/IMG_0001.HEIC' | grep -q '200 image/jpeg'"
t F4 "Live Photo: Foto + MOV werden verknüpft" jqt "$L" '.entries[] | select(.name == "IMG_0001.HEIC") | .live == "Gemeinsam/Urlaub/IMG_0001.MOV"'
t F5 "Live Photo: MOV wird nicht einzeln angezeigt" bash -c "! echo '$L' | jq -e '.entries[] | select(.name == \"IMG_0001.MOV\")' >/dev/null"
t F6 "Live-Video wird als H.264 geliefert" bash -c "curl -s -b $ADM -o $T/live.mp4 '$B/api/live?path=Gemeinsam/Urlaub/IMG_0001.HEIC' && ffprobe -v error -show_entries stream=codec_name -of csv=p=0 $T/live.mp4 2>/dev/null | grep -q h264 || docker run --rm -v $T:/t --entrypoint ffprobe homevault-homevault -v error -show_entries stream=codec_name -of csv=p=0 /t/live.mp4 | grep -q h264"
upload "$ADM" "$A/video.mp4" Gemeinsam >/dev/null
upload "$ADM" "$A/iphone-video.mov" Gemeinsam >/dev/null
t F11 "Umbenennen auf den Namen des Live-Videos wird abgelehnt" bash -c "curl -s -b $ADM -X PATCH -H '$J' -d '{\"from\":\"Gemeinsam/Urlaub/IMG_0001.HEIC\",\"to\":\"Gemeinsam/Urlaub/neu.MOV\"}' $B/api/rename | grep -q 'Live-Video' && curl -s -b $ADM '$B/api/list?path=Gemeinsam/Urlaub' | grep -q IMG_0001.HEIC"
t F7 "H.264-Video wird direkt abgespielt" jqt "$(get "$ADM" '/api/video?path=Gemeinsam/video.mp4')" '.mode == "original"'
t F8 "HEVC-Video wird für alle Browser umgewandelt" until_ok 60 bash -c "curl -s -b $ADM '$B/api/video?path=Gemeinsam/iphone-video.mov' | jq -e '.mode == \"web\"'"
t F9 "Umgewandeltes Video ist abspielbar" is "$(code -b $ADM "$B/api/video/web?path=Gemeinsam/iphone-video.mov")" 200
t F10 "Video-Vorschaubild" is "$(code -b $ADM "$B/api/thumb?path=Gemeinsam/video.mp4")" 200

section "G Zeitleiste, Favoriten, Alben"
curl -s -b $ADM -X POST $B/api/index/rescan >/dev/null; sleep 4
TL=$(get "$ADM" /api/timeline)
t G1 "Zeitleiste enthält die Medien" jqt "$TL" '(.entries | length) >= 4'
t G2 "Aufnahmedatum aus EXIF (Foto)" jqt "$TL" '.entries[] | select(.name == "foto.jpg") | .taken < 1790000000'
t G3 "Aufnahmedatum aus Video-Metadaten" jqt "$TL" '.entries[] | select(.name == "iphone-video.mov") | .taken == 1717236000'
t G4 "Favorit setzen" has "$(post "$ADM" /api/favorites '{"paths":["Gemeinsam/foto.jpg"],"favorite":true}')" '"ok":true'
t G5 "Favoriten-Liste" has "$(get "$ADM" /api/favorites)" "Gemeinsam/foto.jpg"
AL=$(post "$ADM" /api/albums '{"name":"Testalbum"}' | jq -r .id)
t G6 "Album anlegen" bash -c "[ '$AL' -gt 0 ]"
t G7 "Fotos zum Album hinzufügen" jqt "$(post "$ADM" /api/albums/$AL/add '{"paths":["Gemeinsam/foto.jpg","Gemeinsam/Urlaub/strand.jpg"]}')" '.added == 2'
curl -s -b $ADM -X PATCH -H "$J" -d '{"from":"Gemeinsam/foto.jpg","to":"Gemeinsam/foto-neu.jpg"}' $B/api/rename >/dev/null
t G8 "Umbenennen behält Album und Favorit" bash -c "curl -s -b $ADM $B/api/albums/$AL | grep -q foto-neu.jpg && curl -s -b $ADM $B/api/favorites | grep -q foto-neu.jpg"
t G9 "Anderer Benutzer sieht fremdes Album nicht" is "$(code -b $USR $B/api/albums/$AL)" 404

section "H Papierkorb"
t H1 "Löschen verschiebt in den Papierkorb" has "$(curl -s -b $ADM -X DELETE "$B/api/file?path=Gemeinsam/foto-neu.jpg")" '"ok":true'
TR=$(get "$ADM" /api/trash)
t H2 "Eintrag im Papierkorb" jqt "$TR" '.items[] | select(.name == "foto-neu.jpg")'
t H3 "Wiederherstellen" has "$(post "$ADM" /api/trash/restore "{\"ids\":[$(echo "$TR" | jq '.items[0].id')]}")" foto-neu.jpg
t H4 "Album/Favorit nach Wiederherstellen erhalten" bash -c "sleep 3; curl -s -b $ADM $B/api/albums/$AL | grep -q foto-neu.jpg"
t H5 "Ordner mit Inhalt löschen" has "$(curl -s -b $ADM -X DELETE "$B/api/file?path=Gemeinsam/Urlaub")" '"ok":true'
t H6 "Privater Papierkorb für andere unsichtbar" bash -c "curl -s -b $USR -X DELETE '$B/api/file?path=Privat/notizen.md' >/dev/null; ! curl -s -b $ADM $B/api/trash | grep -q notizen.md"
t H7 "Papierkorb leeren" jqt "$(curl -s -b $ADM -X POST $B/api/trash/empty)" '.deleted >= 1'

section "I Teilen-Links"
SH=$(post "$ADM" /api/shares "{\"album_id\":$AL,\"password\":\"oma\",\"expires_days\":7,\"allow_download\":false}")
TK=$(echo "$SH" | jq -r .token)
t I1 "Link mit Passwort erstellen" is "${#TK}" 32
t I2 "Ohne Passwort nur Passwortabfrage" jqt "$(get "$GST" /api/s/$TK)" '.needs_password == true and (.entries == null)'
t I3 "Falsches Passwort abgelehnt" has "$(post "$GST" /api/s/$TK/unlock '{"password":"x"}')" "Falsches"
t I4 "Richtiges Passwort öffnet" has "$(post "$GST" /api/s/$TK/unlock '{"password":"oma"}')" '"ok":true'
SI=$(get "$GST" /api/s/$TK)
t I5 "Gast sieht Album" jqt "$SI" '.title == "Testalbum" and (.entries | length) >= 1'
IT=$(echo "$SI" | jq '.entries[0].id')
t I6 "Gast lädt Vorschaubild" is "$(code -b $GST $B/api/s/$TK/$IT/thumb)" 200
t I7 "Download verboten, wenn nicht erlaubt" is "$(code -b $GST "$B/api/s/$TK/$IT/file?download=true")" 403
t I8 "Fremdes Bild über Link nicht erreichbar" is "$(code -b $GST $B/api/s/$TK/999999/thumb)" 404
t I9 "Öffentliche Seite /s/<token> wird ausgeliefert" is "$(code $B/s/$TK)" 200
SID=$(get "$ADM" /api/shares | jq '.shares[0].id')
curl -s -b $ADM -X DELETE $B/api/shares/$SID >/dev/null
t I10 "Gelöschter Link funktioniert nicht mehr" is "$(code -b $GST $B/api/s/$TK)" 404

section "J Dokument-Vorschau"
for f in vertrag.pdf vertrag.docx notizen.md archiv.zip; do upload "$ADM" "$A/$f" Gemeinsam >/dev/null; done
DL=$(get "$ADM" '/api/list?path=Gemeinsam')
t J1 "Dateitypen erkannt" jqt "$DL" '[.entries[] | select(.doc != null) | .doc] | (index("pdf") and index("office") and index("text") and index("archive"))'
t J2 "PDF-Vorschaubild (erste Seite)" is "$(code -b $ADM "$B/api/thumb?path=Gemeinsam/vertrag.pdf")" 200
t J3 "Word wird in PDF umgewandelt" bash -c "curl -s -b $ADM -o $T/w.pdf -w '%{content_type}' '$B/api/doc?path=Gemeinsam/vertrag.docx' | grep -q application/pdf && head -c 4 $T/w.pdf | grep -q '%PDF'"
t J4 "Word-Vorschaubild" is "$(code -b $ADM "$B/api/thumb?path=Gemeinsam/vertrag.docx")" 200
t J5 "ZIP-Inhalt auflisten" jqt "$(get "$ADM" '/api/archive?path=Gemeinsam/archiv.zip')" '.files == 2'
t J6 "Einzelne Datei aus ZIP laden" bash -c "curl -s -b $ADM '$B/api/archive/file?path=Gemeinsam/archiv.zip&entry=notizen.md' | cmp -s - $A/notizen.md"
t J7 "Nicht unterstützt wird klar gemeldet" is "$(code -b $ADM "$B/api/doc?path=Gemeinsam/archiv.zip")" 415

section "K Netzlaufwerk (SMB)"
t K1 "Anmeldung mit Web-Zugangsdaten" has "$(smb Gemeinsam admin%admin1234 ls)" video.mp4
t K2 "Falsches Passwort abgelehnt" has "$(smb Gemeinsam admin%falsch ls)" LOGON_FAILURE
t K3 "Neues Passwort gilt auch für SMB" has "$(smb Privat lisa%lisa5678 ls)" "blocks"
echo "per SMB" > $T/smb.txt
t K4 "Upload per SMB" has "$(smb Gemeinsam admin%admin1234 "put $T/smb.txt von-smb.txt")" "putting file"
t K5 "SMB-Datei erscheint im Web" has "$(get "$ADM" '/api/list?path=Gemeinsam')" von-smb.txt
t K6 "Privat zeigt nur eigenen Ordner" bash -c "! smbclient //127.0.0.1/Privat -p ${SMB_PORT:-445} -U admin%admin1234 -c ls 2>&1 | grep -q notizen"
t K7 "Gelöschter Benutzer kommt nicht mehr rein" bash -c "curl -s -b $ADM -H '$J' -d '{\"name\":\"tmpuser\",\"password\":\"tmp12345\"}' $B/api/users >/dev/null; sleep 4; smbclient //127.0.0.1/Gemeinsam -p ${SMB_PORT:-445} -U tmpuser%tmp12345 -c ls 2>&1 | grep -q blocks && curl -s -b $ADM -X DELETE $B/api/users/tmpuser >/dev/null && sleep 1 && smbclient //127.0.0.1/Gemeinsam -p ${SMB_PORT:-445} -U tmpuser%tmp12345 -c ls 2>&1 | grep -q LOGON_FAILURE"

section "L Backup auf anderes NAS"
if [ -n "$NAS2_HOST" ]; then
  TGT="\"name\":\"Papas NAS\",\"host\":\"$NAS2_HOST\",\"port\":$NAS2_PORT,\"share\":\"Gemeinsam\",\"folder\":\"backup\",\"username\":\"papa\",\"schedule\":\"daily\",\"time\":\"03:00\",\"keep_days\":30"
  t L1 "Verbindungstest mit falschem Passwort scheitert" jqt "$(post "$ADM" /api/backups/test "{$TGT,\"password\":\"falsch\"}")" '.ok == false'
  t L2 "Verbindungstest erfolgreich" jqt "$(post "$ADM" /api/backups/test "{$TGT,\"password\":\"papa1234\"}")" '.ok == true'
  BID=$(post "$ADM" /api/backups "{$TGT,\"password\":\"papa1234\"}" | jq .id)
  t L3 "Backup-Ziel anlegen (Passwort wird nicht zurückgegeben)" bash -c "curl -s -b $ADM $B/api/backups | jq -e '.targets[0].has_password == true and (.targets[0].password == null)' >/dev/null"
  post "$ADM" /api/backups/$BID/run '{}' >/dev/null
  t L4 "Backup läuft erfolgreich durch" until_ok 60 bash -c "curl -s -b $ADM $B/api/backups | jq -e '.running == null and .targets[0].last_run.status == \"ok\"'"
  t L5 "Dateien und Datenbank liegen auf dem Ziel" bash -c "test -f /srv/nas2/Gemeinsam/backup/Gemeinsam/video.mp4 && test -f /srv/nas2/Gemeinsam/backup/Datenbank/homevault.db && sqlite3 /srv/nas2/Gemeinsam/backup/Datenbank/homevault.db 'select count(*) from users' | grep -qx 2"
  curl -s -b $ADM -X DELETE "$B/api/file?path=Gemeinsam/video.mp4" >/dev/null
  post "$ADM" /api/backups/$BID/run '{}' >/dev/null
  t L6 "Gelöschtes bleibt im Backup unter .versionen" until_ok 60 bash -c "ls /srv/nas2/Gemeinsam/backup/.versionen/*/Gemeinsam/video.mp4"

  # Wiederherstellen
  VER=$(get "$ADM" /api/backups/$BID/versions)
  t L7 "Backup-Versionen werden aufgelistet" jqt "$VER" '(.versions | length) >= 2 and .versions[0].id == ""'
  V1=$(echo "$VER" | jq -r '.versions[1].id')
  t L8 "Im Backup blättern" jqt "$(get "$ADM" "/api/backups/$BID/browse?version=&path=")" '[.entries[].name] | index("Gemeinsam")'
  t L9 "Gelöschte Datei in älterer Version gefunden" has "$(get "$ADM" "/api/backups/$BID/browse?version=$V1&path=Gemeinsam")" video.mp4
  t L10 "Datei an den Originalort zurückholen" has "$(post "$ADM" /api/backups/$BID/restore "{\"version\":\"$V1\",\"paths\":[\"Gemeinsam/video.mp4\"],\"mode\":\"original\"}")" '"ok":true'
  t L11 "Datei ist wieder da (identisch)" until_ok 30 bash -c "curl -s -b $ADM $B/api/backups | jq -e '.restore.ok == true' >/dev/null && cmp -s $MEDIA/Gemeinsam/video.mp4 $A/video.mp4"
  t L12 "Ordner in neuen Ordner „Wiederhergestellt …“ zurückholen" bash -c "curl -s -b $ADM -H '$J' -d '{\"version\":\"\",\"paths\":[\"Gemeinsam\"],\"mode\":\"folder\"}' $B/api/backups/$BID/restore | grep -q ok; sleep 6; ls -d $MEDIA/Gemeinsam/Wiederhergestellt* >/dev/null"
  if [ -n "${RESTORE_URL:-}" ]; then
    R=$RESTORE_URL
    t L13 "Neuer Server: Komplett-Wiederherstellung startet" has "$(curl -s -H "$J" -d "{\"host\":\"$NAS2_HOST\",\"port\":$NAS2_PORT,\"share\":\"Gemeinsam\",\"folder\":\"backup\",\"username\":\"papa\",\"password\":\"papa1234\"}" $R/api/setup/restore)" '"ok":true'
    sleep 8
    t L14 "Neuer Server übernimmt Benutzer aus dem Backup" until_ok 60 bash -c "curl -s $R/api/me | jq -e '.setup_needed == false and .server_name == \"Testserver\"'"
    t L15 "Anmeldung mit altem Konto funktioniert" has "$(curl -s -c $T/r.cj -H "$J" -d '{"username":"admin","password":"admin1234"}' $R/api/login)" '"ok":true'
    t L16 "Dateien und Alben sind wieder da" bash -c "curl -s -b $T/r.cj '$R/api/list?path=Gemeinsam' | grep -q vertrag.pdf && curl -s -b $T/r.cj $R/api/albums | jq -e '.albums[0].name == \"Testalbum\"' >/dev/null"
  fi
else
  nok L0 "Backup" "kein Backup-Ziel (NAS2_HOST) angegeben"
fi

section "S Live-Sync zu anderen NAS"
if [ -n "$NAS2_HOST" ]; then
  N2=/srv/nas2/Gemeinsam
  post "$ADM" /api/folder '{"path":"Gemeinsam/Familie"}' >/dev/null
  upload "$ADM" "$A/foto.jpg" Gemeinsam/Familie >/dev/null
  # zweites Ziel auf demselben NAS, nur als Verbindung, Benutzer dürfen hierher
  KID=$(post "$ADM" /api/backups "{\"name\":\"Keller-NAS\",\"host\":\"$NAS2_HOST\",\"port\":$NAS2_PORT,\"share\":\"Gemeinsam\",\"folder\":\"keller\",\"username\":\"papa\",\"password\":\"papa1234\",\"include_shared\":false,\"include_private\":false,\"include_db\":false,\"user_sync\":true}" | jq .id)
  t S1 "Regel anlegen: Gemeinsam/Familie → Papas NAS" jqt "$(post "$ADM" /api/sync "{\"target_id\":$BID,\"source\":\"shared\",\"sub\":\"Familie\",\"dest\":\"sync/familie\"}")" '.id > 0'
  t S2 "Erster Abgleich überträgt vorhandene Dateien" until_ok 30 test -f $N2/sync/familie/foto.jpg
  t S3 "Zweite Regel in denselben Ordner wird abgelehnt" has "$(post "$ADM" /api/sync "{\"target_id\":$KID,\"source\":\"shared\",\"dest\":\"sync/familie/2024\"}")" "andere Regel"
  t S4 "Regel in einen Backup-Ordner wird abgelehnt" has "$(post "$ADM" /api/sync "{\"target_id\":$BID,\"source\":\"shared\",\"dest\":\"backup/Gemeinsam\"}")" "Backup"
  upload "$ADM" "$A/notizen.md" Gemeinsam/Familie >/dev/null
  echo "per Netzlaufwerk" > $T/smb2.txt
  smb Gemeinsam admin%admin1234 "put $T/smb2.txt Familie/von-smb.txt" >/dev/null
  t S5 "Neue Dateien (Browser und Netzlaufwerk) kommen von selbst an" until_ok 75 bash -c "test -f $N2/sync/familie/notizen.md && test -f $N2/sync/familie/von-smb.txt"
  curl -s -b $ADM -X DELETE "$B/api/file?path=Gemeinsam/Familie/notizen.md" >/dev/null
  t S6 "Gelöschtes verschwindet auf dem NAS, alte Version bleibt" until_ok 75 bash -c "! test -e $N2/sync/familie/notizen.md && ls $N2/sync/familie/.versionen/*/notizen.md"
  t S7 "Status der Regel: aktuell, ohne Fehler" jqt "$(get "$ADM" /api/sync)" '.rules[] | select(.dest == "sync/familie") | .status == "ok" and .last_error == null'
  t S8 "Benutzer darf nur freigegebene NAS wählen" is "$(code -b $USR -H "$J" -d "{\"target_id\":$BID,\"source\":\"private\"}" $B/api/sync)" 403
  upload "$USR" "$A/notizen.md" Privat sync-test.md >/dev/null
  t S9 "Benutzer synchronisiert seinen Privat-Ordner" jqt "$(post "$USR" /api/sync "{\"target_id\":$KID,\"source\":\"private\"}")" '.id > 0'
  t S10 "…in seinen eigenen Ordner auf dem NAS" until_ok 30 test -f $N2/keller/Benutzer/lisa/sync-test.md
  t S11 "Benutzer sieht nur seine eigenen Regeln" jqt "$(get "$USR" /api/sync)" '(.rules | length) == 1 and .rules[0].can_edit == true and (.users | length) == 0 and (.targets | length) == 1'
  t S12 "Admin sieht alle Regeln" jqt "$(get "$ADM" /api/sync)" '(.rules | length) == 2'
  mkdir -p $N2/vorhanden && echo alt > $N2/vorhanden/alt.txt
  t S13 "Zielordner mit fremden Dateien: erst nachfragen" has "$(post "$ADM" /api/sync "{\"target_id\":$BID,\"source\":\"shared\",\"sub\":\"Familie\",\"dest\":\"vorhanden\"}")" "DEST_NOT_EMPTY"
  t S14 "Fremde Dateien bleiben unangetastet" test -f $N2/vorhanden/alt.txt
  SID=$(get "$ADM" /api/sync | jq '.rules[] | select(.dest == "sync/familie") | .id')
  curl -s -b $ADM -X PATCH -H "$J" -d '{"enabled":false}' $B/api/sync/$SID >/dev/null
  upload "$ADM" "$A/vertrag.pdf" Gemeinsam/Familie >/dev/null
  t S15 "Pausierte Regel überträgt nichts" bash -c "sleep 70; ! test -e $N2/sync/familie/vertrag.pdf && curl -s -b $ADM $B/api/sync | jq -e '.rules[] | select(.id == $SID) | .status == \"paused\"' >/dev/null"
fi

section "W Eingang: automatisch einsortieren"
t W1 "Eingangs-Ordner werden angelegt (Gemeinsam und pro Benutzer)" until_ok 30 bash -c "test -d $MEDIA/Gemeinsam/Eingang && test -d $MEDIA/Privat/lisa/Eingang"
upload "$ADM" "$A/foto.jpg" Gemeinsam/Eingang >/dev/null
upload "$ADM" "$A/IMG_0001.HEIC" Gemeinsam/Eingang >/dev/null
upload "$ADM" "$A/IMG_0001.MOV" Gemeinsam/Eingang >/dev/null
upload "$USR" "$A/iphone-video.mov" Privat/Eingang >/dev/null
# Video mit GPS (Lissabon), über das Netzlaufwerk abgelegt wie von einem Handy
ffmpeg -v error -y -f lavfi -i testsrc=duration=1:size=320x240:rate=10 -metadata location="+38.7223-009.1393/" \
  -metadata creation_time="2025-08-03T10:00:00Z" -c:v libx264 -pix_fmt yuv420p $T/urlaub.mp4
smb Gemeinsam admin%admin1234 "put $T/urlaub.mp4 Eingang/urlaub.mp4" >/dev/null
t W2 "Foto landet nach Aufnahmedatum in Fotos/2024/2024-07" until_ok 45 test -f $MEDIA/Gemeinsam/Fotos/2024/2024-07/foto.jpg
t W3 "Video mit GPS: Ordner mit Ortsname (offline erkannt)" until_ok 30 test -f "$MEDIA/Gemeinsam/Fotos/2025/2025-08 Lissabon/urlaub.mp4"
t W4 "Live Photo: Foto und Video ziehen zusammen um" until_ok 30 bash -c "f=\$(find $MEDIA/Gemeinsam/Fotos -name IMG_0001.HEIC); test -n \"\$f\" && test -f \"\${f%.HEIC}.MOV\""
t W5 "Privater Eingang sortiert in den eigenen Privat-Ordner" until_ok 30 test -f $MEDIA/Privat/lisa/Fotos/2024/2024-06/iphone-video.mov
t W6 "Eingang ist danach leer" bash -c "test -z \"\$(find $MEDIA/Gemeinsam/Eingang -maxdepth 1 -type f)\""
IB=$(get "$ADM" /api/inbox)
t W7 "Verlauf zeigt, was wohin kam" jqt "$IB" '[.items[] | select(.original_name == "foto.jpg" and .status == "done" and .new_path == "Gemeinsam/Fotos/2024/2024-07/foto.jpg")] | length == 1'
t W8 "Fremder privater Verlauf bleibt unsichtbar" jqt "$(get "$USR" /api/inbox)" 'all(.items[]; .area == "Gemeinsam" or .area == "Privat/lisa") and any(.items[]; .area == "Privat/lisa")'
WID=$(echo "$IB" | jq '[.items[] | select(.original_name == "foto.jpg")][0].id')
t W9 "Rückgängig: Datei zurück nach Eingang/Zurückgelegt" bash -c "curl -s -b $ADM -X POST $B/api/inbox/$WID/undo | grep -q ok && test -f $MEDIA/Gemeinsam/Eingang/Zurückgelegt/foto.jpg && ! test -e $MEDIA/Gemeinsam/Fotos/2024/2024-07/foto.jpg"
t W10 "Zurückgelegtes wird nicht erneut sortiert" bash -c "sleep 35; test -f $MEDIA/Gemeinsam/Eingang/Zurückgelegt/foto.jpg"
t W11 "Texterkennung (OCR) für Scans im Image vorhanden" bash -c "docker exec -i ${CONTAINER_PREFIX:-homevault} sh -c 'cat > /tmp/v.pdf && pdftoppm -r 150 -l 1 -png /tmp/v.pdf /tmp/s && tesseract /tmp/s-1.png stdout -l deu 2>/dev/null' < $A/vertrag.pdf | grep -qi vertrag"
if [ "$(get "$ADM" /api/inbox | jq '.ai != null')" = true ]; then
  t W12 "KI-Modell wird geladen und ist bereit" until_ok 150 bash -c "curl -s -b $ADM $B/api/inbox | jq -e '.ai.status.state == \"ready\"'"
  printf 'Stadtwerke München GmbH\nRechnung Nr. 2026-031-88412   München, 12.03.2026\nIhre Stromrechnung für Februar 2026\nVerbrauch 312 kWh, Gesamtbetrag 116,09 EUR\n' > $T/scan_0042.txt
  upload "$ADM" "$T/scan_0042.txt" Gemeinsam/Eingang >/dev/null
  t W13 "Dokument wird von der KI benannt und in Dokumente/ einsortiert" until_ok 120 bash -c "find $MEDIA/Gemeinsam/Dokumente -mindepth 2 -name '*.txt' | grep -q . && ! test -e $MEDIA/Gemeinsam/Eingang/scan_0042.txt"
  t W14 "Neuer Name statt scan_0042" bash -c "! find $MEDIA/Gemeinsam/Dokumente -name 'scan_0042.txt' | grep -q ."
fi

section "M Speicher & System"
ST=$(get "$ADM" /api/storage)
t M1 "Speicher-Übersicht mit Plattengröße" jqt "$ST" '.disk.total > 0 and .disk.free > 0'
t M2 "Belegung pro Benutzer" jqt "$ST" '.users | length >= 2'
t M3 "Vorschau-Funktionen verfügbar (Office, PDF)" jqt "$(get "$ADM" /api/capabilities)" '.office == true and .page_thumbs == true'
t M4 "Medien-Index ohne Fehler" jqt "$(get "$ADM" /api/index)" '.index.last_error == null and .index.items > 0'
t M5 "Wiederherstellungs-Status nach der Einrichtung nicht mehr öffentlich" jqt "$(curl -s $B/api/setup/restore)" '.done == true and .result == null'

section "N Festplatten: Ausfall, Notbetrieb, Tausch (Hot-Swap)"
D=$(disks)
OLD=$(echo "$D" | jq -r '.pool.devices[1].path')
OLDID=$(echo "$D" | jq '.pool.devices[1].devid')
OLD_IMG=$(losetup -n -O BACK-FILE "$OLD" | tr -d ' ')
compose stop >/dev/null 2>&1
umount "$MEDIA" && losetup -d "$OLD"
compose up -d >/dev/null 2>&1
t N12 "Server startet trotz fehlender Platte (Notbetrieb)" until_ok 90 health
t N13 "Fehlende Platte wird gemeldet" jqt "$(get "$ADM" /api/disks/summary)" '.state == "degraded"'
t N14 "Daten im Notbetrieb lesbar" is "$(code -b $ADM "$B/api/file?path=Gemeinsam/von-smb.txt")" 200
t N15 "Letzte intakte Platte kann nicht entfernt werden" has "$(post "$ADM" /api/disks/remove "{\"devid\":$(disks | jq '[.pool.devices[] | select(.missing | not)][0].devid')}")" "mindestens"
t N16 "Defekte Platte durch neue ersetzen" has "$(post "$ADM" /api/disks/replace "{\"devid\":$OLDID,\"device\":\"/dev/$L3\",\"confirm\":[\"$L3\"]}")" '"ok":true'
t N17 "Spiegel danach wieder vollständig" until_ok 60 bash -c "curl -s -b $ADM $B/api/disks | jq -e '.job == null and .pool.degraded == false and ([.pool.devices[] | select(.missing)] | length) == 0'"
t N18 "Datenprüfung (Scrub) ohne Fehler" bash -c "curl -s -b $ADM -H '$J' -d '{}' $B/api/disks/scrub >/dev/null; sleep 5; curl -s -b $ADM $B/api/disks | jq -e '.pool.scrub.status | test(\"finished\")' >/dev/null"
losetup -f "$OLD_IMG"
t N19 "Alte Platte wird als frühere Platte erkannt" until_ok 10 bash -c "curl -s -b $ADM $B/api/disks | jq -e '[.disks[] | select(.old_member == true)] | length == 1'"
NEWD=$(disks | jq -r '[.disks[] | select(.old_member == true)][0].name')
t N20 "Dritte Platte hinzufügen (Erweitern)" has "$(post "$ADM" /api/disks/add "{\"device\":\"/dev/$NEWD\",\"confirm\":[\"$NEWD\"]}")" '"ok":true'
t N21 "Spiegel hat jetzt 3 Platten" until_ok 30 bash -c "curl -s -b $ADM $B/api/disks | jq -e '.job == null and (.pool.devices | length) == 3'"

section "N Festplatten: alle Platten fehlen beim Start"
POOL_IMGS=$(for d in $(disks | jq -r '.pool.devices[].path'); do losetup -n -O BACK-FILE "$d" | tr -d ' '; done)
compose stop >/dev/null 2>&1
umount "$MEDIA"
for img in $POOL_IMGS; do losetup -d "$(losetup -j "$img" | cut -d: -f1)"; done
compose up -d >/dev/null 2>&1
t N22 "Oberfläche startet trotzdem" until_ok 120 health
t N23 "…aber nur lesend (Notbetrieb gemeldet)" bash -c "docker logs ${CONTAINER_PREFIX:-homevault} 2>&1 | grep -q 'nur lesender Notbetrieb'"
t N24 "Netzlaufwerk bleibt aus, solange der Spiegel fehlt" bash -c "docker logs ${CONTAINER_PREFIX:-homevault}-smb 2>&1 | tail -3 | grep -q 'Warte auf den Speicherverbund'"
for img in $POOL_IMGS; do losetup -f "$img"; done
t N25 "Platten wieder da: Spiegel wird von selbst eingehängt" until_ok 90 bash -c "health && curl -s -b $ADM $B/api/disks | jq -e '.pool.mounted == true and .maintenance == false'"
t N26 "Schreiben wieder möglich" until_ok 15 bash -c "curl -s -b $ADM -H '$J' -d '{\"path\":\"Gemeinsam/Nach-Ausfall\"}' $B/api/folder | grep -q '\"ok\":true'"
t N27 "Netzlaufwerk wieder erreichbar" until_ok 30 bash -c "smb Gemeinsam admin%admin1234 ls | grep -q Nach-Ausfall"

printf '\nERGEBNIS: %d bestanden, %d fehlgeschlagen\n' "$PASSED" "$FAILED"
[ "$FAILED" -eq 0 ]
