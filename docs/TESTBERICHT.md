# Testbericht homevault

**Ergebnis: 124 von 124 Tests bestanden.**  
Getestet am 11. Oktober 2026 auf einem frisch installierten Debian 13, Installation genau nach README.

## Fazit

**homevault ist für den Einsatz zu Hause benutzbar.** Alle Funktionen haben im Ende-zu-Ende-Test auf einem frischen Debian-Server funktioniert: Installation, Einrichtung, Benutzer, Dateien, Fotos und Videos (inkl. iPhone-Formate), Zeitleiste, Alben, Papierkorb, Teilen-Links, Dokument-Vorschau, Netzlaufwerk, Backup mit Wiederherstellung, Festplatten-Spiegel mit Ausfall und Tausch.

Empfehlung für den Start:
1. Festplatten-Spiegel **und** Backup auf ein zweites Gerät einrichten. Der Spiegel schützt vor einem Plattendefekt, das Backup vor Löschen, Erpressungstrojanern und Totalverlust.
2. Einmal den Ausfall üben: eine Platte im ausgeschalteten Zustand abziehen, starten, Warnung prüfen, wieder einstecken bzw. tauschen.
3. Nicht direkt ins Internet stellen (siehe README).

Die Punkte unter „Nicht getestet“ sollten beim ersten Einsatz auf echter Hardware im Blick behalten werden.

## Testumgebung

| | |
|---|---|
| Betriebssystem | Debian 13.7 „trixie“ (Container mit eigenem Docker-Dienst, wie ein eigener Server) |
| Docker | 26.1.5 und Docker Compose 2.26.1 aus den Debian-Paketquellen |
| Festplatten | 3 × 2 GB (Loop-Geräte), Systemplatte zum Gegenprüfen |
| Instanzen | homevault mit Festplatten-Verwaltung, zweiter homevault als Backup-Ziel, dritter leerer Server für die Wiederherstellung |
| Clients | `curl` (wie der Browser), `smbclient` (wie Windows/macOS), `ffprobe`, `sqlite3` |
| Image-Größen | Web-App 1,19 GB (davon ca. 1 GB LibreOffice für die Office-Vorschau), Platten-Dienst 38 MB, Samba 73 MB |

## Vorgehen

1. `tests/debian-server.sh` installiert Docker aus den Debian-Quellen, legt Datenordner und drei Test-Platten an und installiert homevault nach README (`.env` aus der Vorlage) und baut die Images dabei aus dem aktuellen Quellcode (`docker-compose.build.yml`), damit genau dieser Stand getestet wird.
2. `tests/e2e.sh` prüft jede Funktion über dieselben Schnittstellen, die Weboberfläche und Netzlaufwerk benutzen, und vergleicht Dateien bytegenau.
3. Zusätzlich: 23 Unit-Tests (`cargo test`), TypeScript-Prüfung und Bedienung im Browser (Einrichtungsassistent, Rahmenauswahl, Vorschauen, Einstellungen, Festplatten-Seite).

## Ergebnisse


### A Installation

Installation nach README auf frischem Debian: `cp .env.example .env`, Festplatten-Verwaltung einschalten, Container bauen und starten.


| Nr. | Test | Ergebnis |
|---|---|---|
| A1 | Alle Container laufen | ✅ bestanden |
| A2 | Web-App antwortet (Gesundheitscheck) | ✅ bestanden |
| A3 | Weboberfläche wird ausgeliefert | ✅ bestanden |
| A4 | Platten-Dienst ist gesund | ✅ bestanden |

### B Ersteinrichtung

Einrichtungsassistent über die API, so wie ihn der Browser aufruft.


| Nr. | Test | Ergebnis |
|---|---|---|
| B1 | Server meldet: Einrichtung nötig | ✅ bestanden |
| B2 | Zu kurzes Passwort wird abgelehnt | ✅ bestanden |
| B3 | Admin-Konto wird angelegt und angemeldet | ✅ bestanden |
| B4 | Benutzername wird kleingeschrieben | ✅ bestanden |
| B5 | Einrichtung ist danach gesperrt | ✅ bestanden |
| B6 | Servername gespeichert | ✅ bestanden |

### N Festplatten (Spiegel/RAID)

Drei virtuelle 2-GB-Platten (Loop-Geräte). Aus zweien wird ein Btrfs-RAID1 angelegt, während schon Daten vorhanden sind.


| Nr. | Test | Ergebnis |
|---|---|---|
| N1 | Platten werden erkannt (3 freie Test-Platten) | ✅ bestanden |
| N2 | Systemplatte ist gesperrt | ✅ bestanden |
| N3 | Ohne Namensbestätigung wird nichts formatiert | ✅ bestanden |
| N4 | Spiegel aus 2 Platten wird angelegt | ✅ bestanden |
| N5 | Schreiben während des Umzugs gesperrt (Wartung) | ✅ bestanden |
| N6 | Web-App startet auf dem Spiegel neu | ✅ bestanden |
| N7 | Anmeldung bleibt erhalten (Daten mit umgezogen) | ✅ bestanden |
| N8 | Vorhandene Dateien mit umgezogen | ✅ bestanden |
| N9 | Server sieht Btrfs-Spiegel unter MEDIA_DIR | ✅ bestanden |
| N10 | Profil RAID1, 2 Platten, kein Notbetrieb | ✅ bestanden |
| N11 | Zustand für Seitenleiste: ok | ✅ bestanden |

### C Anmeldung & Sitzungen

Anmeldung, Schutz ohne Anmeldung, Cookie-Sicherheit.


| Nr. | Test | Ergebnis |
|---|---|---|
| C1 | Falsches Passwort wird abgelehnt | ✅ bestanden |
| C2 | Ohne Anmeldung kein Zugriff auf Dateien | ✅ bestanden |
| C3 | Session-Cookie ist HttpOnly + SameSite | ✅ bestanden |

### D Benutzerverwaltung

Admin legt Benutzerin „lisa“ an; Rechte, Speicherlimit und Passwortänderung.


| Nr. | Test | Ergebnis |
|---|---|---|
| D1 | Benutzer anlegen | ✅ bestanden |
| D2 | Ungültiger Name wird abgelehnt | ✅ bestanden |
| D3 | Doppelter Name wird abgelehnt | ✅ bestanden |
| D4 | Neuer Benutzer kann sich anmelden | ✅ bestanden |
| D5 | Normaler Benutzer darf keine Benutzer verwalten | ✅ bestanden |
| D6 | Privater Ordner wurde angelegt | ✅ bestanden |
| D7 | Speicherlimit setzen (1 MB) | ✅ bestanden |
| D8 | Upload über dem Limit wird abgelehnt | ✅ bestanden |
| D9 | Eigenes Passwort ändern | ✅ bestanden |
| D10 | Altes Passwort gilt nicht mehr | ✅ bestanden |

### E Dateien

Hochladen, Ordner, Umbenennen, Streaming, Sicherheit gegen Pfad-Ausbruch und Schadcode in HTML-Dateien.


| Nr. | Test | Ergebnis |
|---|---|---|
| E1 | Foto hochladen (Gemeinsam) | ✅ bestanden |
| E2 | Doppelter Name bekommt (1) | ✅ bestanden |
| E3 | Ordner anlegen | ✅ bestanden |
| E4 | Umbenennen | ✅ bestanden |
| E5 | Ordnerinhalt auflisten | ✅ bestanden |
| E6 | Teilweises Laden (Range, für Videos) | ✅ bestanden |
| E7 | Download mit Dateinamen | ✅ bestanden |
| E8 | Ausbruch mit ../ wird blockiert | ✅ bestanden |
| E9 | Fremder privater Ordner nicht sichtbar | ✅ bestanden |
| E10 | Hochgeladene HTML-Datei läuft in Sandbox | ✅ bestanden |

### F Fotos & Videos

Echte Testdateien: JPEG mit EXIF-Datum, HEIC, Live Photo (HEIC + HEVC-MOV), HEVC-Video wie vom iPhone, H.264-Video.


| Nr. | Test | Ergebnis |
|---|---|---|
| F1 | Vorschaubild (JPEG) | ✅ bestanden |
| F2 | HEIC wird erkannt (Vorschau nötig) | ✅ bestanden |
| F3 | HEIC-Vorschau als JPEG | ✅ bestanden |
| F4 | Live Photo: Foto + MOV werden verknüpft | ✅ bestanden |
| F5 | Live Photo: MOV wird nicht einzeln angezeigt | ✅ bestanden |
| F6 | Live-Video wird als H.264 geliefert | ✅ bestanden |
| F7 | H.264-Video wird direkt abgespielt | ✅ bestanden |
| F8 | HEVC-Video wird für alle Browser umgewandelt | ✅ bestanden |
| F9 | Umgewandeltes Video ist abspielbar | ✅ bestanden |
| F10 | Video-Vorschaubild | ✅ bestanden |

### G Zeitleiste, Favoriten, Alben

Medien-Index, Aufnahmedatum, Favoriten und Alben inkl. Umbenennen.


| Nr. | Test | Ergebnis |
|---|---|---|
| G1 | Zeitleiste enthält die Medien | ✅ bestanden |
| G2 | Aufnahmedatum aus EXIF (Foto) | ✅ bestanden |
| G3 | Aufnahmedatum aus Video-Metadaten | ✅ bestanden |
| G4 | Favorit setzen | ✅ bestanden |
| G5 | Favoriten-Liste | ✅ bestanden |
| G6 | Album anlegen | ✅ bestanden |
| G7 | Fotos zum Album hinzufügen | ✅ bestanden |
| G8 | Umbenennen behält Album und Favorit | ✅ bestanden |
| G9 | Anderer Benutzer sieht fremdes Album nicht | ✅ bestanden |

### H Papierkorb

Löschen, Wiederherstellen (mit Album-Zugehörigkeit), Ordner, privater Papierkorb, Leeren.


| Nr. | Test | Ergebnis |
|---|---|---|
| H1 | Löschen verschiebt in den Papierkorb | ✅ bestanden |
| H2 | Eintrag im Papierkorb | ✅ bestanden |
| H3 | Wiederherstellen | ✅ bestanden |
| H4 | Album/Favorit nach Wiederherstellen erhalten | ✅ bestanden |
| H5 | Ordner mit Inhalt löschen | ✅ bestanden |
| H6 | Privater Papierkorb für andere unsichtbar | ✅ bestanden |
| H7 | Papierkorb leeren | ✅ bestanden |

### I Teilen-Links

Album-Link mit Passwort, Ablauf und Download-Sperre, aufgerufen wie von einem Gast ohne Konto.


| Nr. | Test | Ergebnis |
|---|---|---|
| I1 | Link mit Passwort erstellen | ✅ bestanden |
| I2 | Ohne Passwort nur Passwortabfrage | ✅ bestanden |
| I3 | Falsches Passwort abgelehnt | ✅ bestanden |
| I4 | Richtiges Passwort öffnet | ✅ bestanden |
| I5 | Gast sieht Album | ✅ bestanden |
| I6 | Gast lädt Vorschaubild | ✅ bestanden |
| I7 | Download verboten, wenn nicht erlaubt | ✅ bestanden |
| I8 | Fremdes Bild über Link nicht erreichbar | ✅ bestanden |
| I9 | Öffentliche Seite /s/<token> wird ausgeliefert | ✅ bestanden |
| I10 | Gelöschter Link funktioniert nicht mehr | ✅ bestanden |

### J Dokument-Vorschau

PDF, Word (über LibreOffice), Markdown, ZIP; nicht unterstütztes Format.


| Nr. | Test | Ergebnis |
|---|---|---|
| J1 | Dateitypen erkannt | ✅ bestanden |
| J2 | PDF-Vorschaubild (erste Seite) | ✅ bestanden |
| J3 | Word wird in PDF umgewandelt | ✅ bestanden |
| J4 | Word-Vorschaubild | ✅ bestanden |
| J5 | ZIP-Inhalt auflisten | ✅ bestanden |
| J6 | Einzelne Datei aus ZIP laden | ✅ bestanden |
| J7 | Nicht unterstützt wird klar gemeldet | ✅ bestanden |

### K Netzlaufwerk (SMB)

Mit `smbclient` (dasselbe Protokoll wie Windows-Explorer und Finder) gegen den Samba-Container.


| Nr. | Test | Ergebnis |
|---|---|---|
| K1 | Anmeldung mit Web-Zugangsdaten | ✅ bestanden |
| K2 | Falsches Passwort abgelehnt | ✅ bestanden |
| K3 | Neues Passwort gilt auch für SMB | ✅ bestanden |
| K4 | Upload per SMB | ✅ bestanden |
| K5 | SMB-Datei erscheint im Web | ✅ bestanden |
| K6 | Privat zeigt nur eigenen Ordner | ✅ bestanden |
| K7 | Gelöschter Benutzer kommt nicht mehr rein | ✅ bestanden |

### L Backup auf anderes NAS

Ein zweiter homevault („Papas NAS“) dient als SMB-Backup-Ziel, ein dritter, leerer Server testet die Komplett-Wiederherstellung.


| Nr. | Test | Ergebnis |
|---|---|---|
| L1 | Verbindungstest mit falschem Passwort scheitert | ✅ bestanden |
| L2 | Verbindungstest erfolgreich | ✅ bestanden |
| L3 | Backup-Ziel anlegen (Passwort wird nicht zurückgegeben) | ✅ bestanden |
| L4 | Backup läuft erfolgreich durch | ✅ bestanden |
| L5 | Dateien und Datenbank liegen auf dem Ziel | ✅ bestanden |
| L6 | Gelöschtes bleibt im Backup unter .versionen | ✅ bestanden |
| L7 | Backup-Versionen werden aufgelistet | ✅ bestanden |
| L8 | Im Backup blättern | ✅ bestanden |
| L9 | Gelöschte Datei in älterer Version gefunden | ✅ bestanden |
| L10 | Datei an den Originalort zurückholen | ✅ bestanden |
| L11 | Datei ist wieder da (identisch) | ✅ bestanden |
| L12 | Ordner in neuen Ordner „Wiederhergestellt …“ zurückholen | ✅ bestanden |
| L13 | Neuer Server: Komplett-Wiederherstellung startet | ✅ bestanden |
| L14 | Neuer Server übernimmt Benutzer aus dem Backup | ✅ bestanden |
| L15 | Anmeldung mit altem Konto funktioniert | ✅ bestanden |
| L16 | Dateien und Alben sind wieder da | ✅ bestanden |

### M Speicher & System

Speicher-Übersicht, Funktionsprüfung, Medien-Index.


| Nr. | Test | Ergebnis |
|---|---|---|
| M1 | Speicher-Übersicht mit Plattengröße | ✅ bestanden |
| M2 | Belegung pro Benutzer | ✅ bestanden |
| M3 | Vorschau-Funktionen verfügbar (Office, PDF) | ✅ bestanden |
| M4 | Medien-Index ohne Fehler | ✅ bestanden |

### N Festplatten: Ausfall, Notbetrieb, Tausch (Hot-Swap)

Server aus, eine Platte entfernt, Server an. Dann Tausch gegen eine neue Platte, Datenprüfung, Wiedereinstecken der alten Platte und Erweitern auf drei Platten.


| Nr. | Test | Ergebnis |
|---|---|---|
| N12 | Server startet trotz fehlender Platte (Notbetrieb) | ✅ bestanden |
| N13 | Fehlende Platte wird gemeldet | ✅ bestanden |
| N14 | Daten im Notbetrieb lesbar | ✅ bestanden |
| N15 | Letzte intakte Platte kann nicht entfernt werden | ✅ bestanden |
| N16 | Defekte Platte durch neue ersetzen | ✅ bestanden |
| N17 | Spiegel danach wieder vollständig | ✅ bestanden |
| N18 | Datenprüfung (Scrub) ohne Fehler | ✅ bestanden |
| N19 | Alte Platte wird als frühere Platte erkannt | ✅ bestanden |
| N20 | Dritte Platte hinzufügen (Erweitern) | ✅ bestanden |
| N21 | Spiegel hat jetzt 3 Platten | ✅ bestanden |

## Gefundene und behobene Fehler

Der erste Durchlauf ergab 118 von 124. Die Abweichungen:

| Test | Ursache | Behebung |
|---|---|---|
| L14–L16 | **Fehler in homevault:** Das Backup übertrug leere Ordner nicht. Waren die privaten Ordner leer, fehlte `Privat/` im Backup, und die Komplett-Wiederherstellung brach mit „directory not found“ ab. | Backup überträgt jetzt auch leere Ordner (`--create-empty-src-dirs`); die Wiederherstellung überspringt Bereiche, die im Backup fehlen. Zweiter Durchlauf: bestanden. |
| A4, E10, I1 | Fehler im Testskript (Hilfsfunktionen und eine Variable waren in Unter-Shells nicht verfügbar). | Testskript korrigiert; A1 hätte deswegen fälschlich bestanden und wurde dabei mit abgesichert. |

Bei der Entwicklung vorher zusätzlich gefunden und behoben:
- Datenordner gehört `root`, homevault läuft als Benutzer 1000: früher Absturz, jetzt verständliche Meldung mit dem Befehl zur Behebung.
- Neu angelegter Spiegel gehörte `root`: der Besitzer des bisherigen Datenordners wird jetzt übernommen.
- Wieder eingesteckte, früher entfernte Platte wurde fälschlich als Teil des Spiegels angezeigt: wird jetzt als „frühere Platte“ erkannt und kann neu verwendet werden.
- Vorschaubilder für PDF/Word schlugen wegen eines Dateinamens fehl (pdftoppm hängt die Endung selbst an).

## Nicht getestet (Grenzen dieses Tests)

| Bereich | Warum nicht | Risiko / Empfehlung |
|---|---|---|
| Echte Festplatten, echter Hot-Swap-Wechselrahmen | Auf einem Mac nicht möglich; getestet mit Loop-Geräten über denselben Kernel-Weg (Btrfs, mount, replace). | Ob das Ein- und Ausstecken im Betrieb klappt, hängt vom Controller ab (AHCI-Hotplug im BIOS aktivieren). Einmal mit echten Platten üben. |
| SMART-Werte | Loop-Geräte haben keine SMART-Daten. | Anzeige auf echter Hardware prüfen. |
| Windows-Explorer, macOS Finder, iPhone-Dateien-App | Getestet mit `smbclient` (gleiches SMB-Protokoll). | Gering. |
| Browser auf iPhone/Android | Oberfläche im Desktop-Browser und in Handy-Größe geprüft, nicht auf echten Geräten. | Gering bis mittel (Long-Press, PDF-Ansicht auf iOS). |
| Große Bibliotheken (z. B. 50.000 Fotos, mehrere TB) | Testdaten nur wenige MB. | Erster Index-Durchlauf, Video-Umwandlung und Spiegel-Anlage dauern dann deutlich länger. Spiegel am besten anlegen, bevor viele Daten da sind. |
| Zeitgesteuerte Abläufe (Backup um 3 Uhr, monatliche Datenprüfung, Papierkorb nach 30 Tagen) | Zeitplan-Logik per Unit-Test geprüft, nicht über echte Tage. | Nach der ersten Nacht den Backup-Verlauf ansehen. |
| Stromausfall während Spiegel-Anlage oder Plattentausch | Nicht simuliert. | Diese Vorgänge nur mit stabiler Stromversorgung starten (USV empfohlen). |
| ARM64 (z. B. Raspberry Pi), `install.sh`, GitHub-Actions | Erst möglich, wenn das Projekt auf GitHub liegt. | Nach dem ersten Upload prüfen. |

