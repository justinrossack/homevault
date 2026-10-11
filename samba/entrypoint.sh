#!/bin/sh
# Samba mit denselben Benutzern wie die Weboberfläche.
# homevault schreibt die Logins nach /data/.homevault/smbpasswd, Samba liest
# diese Datei bei jeder Anmeldung. Neue Benutzer gelten damit sofort.
set -eu

PUID="${PUID:-1000}"
PGID="${PGID:-1000}"
PASSDB=/data/.homevault/smbpasswd

# Alle Dateizugriffe laufen über denselben Linux-Benutzer wie homevault,
# damit Web und SMB gegenseitig ihre Dateien bearbeiten dürfen.
getent group "$PGID" >/dev/null || addgroup -g "$PGID" hvfiles
GROUP="$(getent group "$PGID" | cut -d: -f1)"
FILES_USER="$(getent passwd "$PUID" | cut -d: -f1)"
if [ -z "$FILES_USER" ]; then
    adduser -D -H -u "$PUID" -G "$GROUP" -s /sbin/nologin hvfiles
    FILES_USER=hvfiles
fi

until [ -s "$PASSDB" ]; do
    echo "Warte auf $PASSDB (wird von homevault angelegt) ..."
    sleep 2
done

# Samba braucht zu jedem Login ein Linux-Konto (ohne Shell, ohne Rechte).
sync_users() {
    while IFS=: read -r name uid _; do
        [ -n "$name" ] || continue
        id "$name" >/dev/null 2>&1 \
            || adduser -D -H -u "$uid" -G "$GROUP" -s /sbin/nologin "$name" 2>/dev/null \
            || echo "Warnung: Linux-Konto für $name konnte nicht angelegt werden"
    done < "$PASSDB"
}
sync_users
(
    last="$(md5sum "$PASSDB")"
    while sleep 3; do
        cur="$(md5sum "$PASSDB" 2>/dev/null || true)"
        if [ "$cur" != "$last" ]; then
            sync_users
            last="$cur"
        fi
    done
) &

cat > /etc/samba/smb.conf <<CONF
[global]
   server string = homevault
   server role = standalone server
   workgroup = WORKGROUP
   security = user
   passdb backend = smbpasswd:$PASSDB
   map to guest = never
   server min protocol = SMB2_10
   smb encrypt = desired
   load printers = no
   printing = bsd
   printcap name = /dev/null
   disable spoolss = yes
   # bessere Kompatibilität mit macOS Finder
   vfs objects = catia fruit streams_xattr
   fruit:metadata = stream
   fruit:model = MacSamba
   fruit:delete_empty_adfiles = yes
   force user = $FILES_USER
   force group = $GROUP
   create mask = 0664
   directory mask = 0775
   # laufende Web-Uploads verstecken
   veto files = /.upload-*.part/

[Gemeinsam]
   path = /data/Gemeinsam
   read only = no

# %U = angemeldeter Benutzer, jeder landet in seinem eigenen Ordner
[Privat]
   path = /data/Privat/%U
   read only = no
CONF

testparm -s >/dev/null
echo "Samba bereit: Freigaben Gemeinsam und Privat"
exec smbd --foreground --no-process-group --debug-stdout
