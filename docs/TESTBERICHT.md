# Testbericht homevault

**Ergebnis: 162 von 162 Tests bestanden.**  
Getestet am 11. Oktober 2026 auf einem frisch installierten Debian 13, Installation genau nach README.

## Fazit

**homevault ist für den Einsatz zu Hause benutzbar.** Alle Funktionen haben im Ende-zu-Ende-Test auf einem frischen Debian-Server funktioniert: Installation, Einrichtung, Benutzer, Dateien, Fotos und Videos (inkl. iPhone-Formate), Zeitleiste, Alben, Papierkorb, Teilen-Links, Dokument-Vorschau, Netzlaufwerk, Backup mit Wiederherstellung, Live-Sync auf andere NAS, Eingangs-Ordner mit lokaler KI, Festplatten-Spiegel mit Ausfall und Tausch.

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
| Instanzen | homevault mit Festplatten-Verwaltung und KI (Ollama), zweiter homevault als Backup- und Sync-Ziel, dritter leerer Server für die Wiederherstellung |
| Clients | `curl` (wie der Browser), `smbclient` (wie Windows/macOS), `ffprobe`, `sqlite3` |
| Image-Größen | Web-App 1,19 GB (davon ca. 1 GB LibreOffice für die Office-Vorschau), Platten-Dienst 38 MB, Samba 73 MB |

## Vorgehen

1. `tests/debian-server.sh` installiert Docker aus den Debian-Quellen, legt Datenordner und drei Test-Platten an und installiert homevault nach README (`.env` aus der Vorlage) und baut die Images dabei aus dem aktuellen Quellcode (`docker-compose.build.yml`), damit genau dieser Stand getestet wird.
2. `tests/e2e.sh` prüft jede Funktion über dieselben Schnittstellen, die Weboberfläche und Netzlaufwerk benutzen, und vergleicht Dateien bytegenau.
3. Zusätzlich: 32 Unit-Tests (`cargo test`), TypeScript-Prüfung und Bedienung im Browser (Einrichtungsassistent, Rahmenauswahl, Vorschauen, Einstellungen, Festplatten-Seite, Vollbildansicht mit Tastatur, Benutzer anlegen).

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
| E11 | Gleichzeitige Uploads gleichen Namens: keiner überschreibt | ✅ bestanden |

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
| F11 | Umbenennen auf den Namen des Live-Videos wird abgelehnt | ✅ bestanden |
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
| M5 | Wiederherstellungs-Status nach der Einrichtung nicht mehr öffentlich | ✅ bestanden |

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

### N Festplatten: alle Platten fehlen beim Start

Server aus, alle Platten des Spiegels entfernt, Server an. homevault darf dann nicht auf den alten Ordner unter dem Spiegel schreiben. Danach werden die Platten wieder eingesteckt, ohne Neustart von Hand.


| Nr. | Test | Ergebnis |
|---|---|---|
| N22 | Oberfläche startet trotzdem | ✅ bestanden |
| N23 | …aber nur lesend (Notbetrieb gemeldet) | ✅ bestanden |
| N24 | Netzlaufwerk bleibt aus, solange der Spiegel fehlt | ✅ bestanden |
| N25 | Platten wieder da: Spiegel wird von selbst eingehängt | ✅ bestanden |
| N26 | Schreiben wieder möglich | ✅ bestanden |
| N27 | Netzlaufwerk wieder erreichbar | ✅ bestanden |

### S Live-Sync zu anderen NAS

Regeln „Quelle → Ziel“ gegen den zweiten homevault als NAS (SMB). Änderungen aus Browser und Netzlaufwerk, Löschen mit Versionen, Benutzerrechte, Schutz vor Überschneidungen.


| Nr. | Test | Ergebnis |
|---|---|---|
| S1 | Regel anlegen: Gemeinsam/Familie → Papas NAS | ✅ bestanden |
| S2 | Erster Abgleich überträgt vorhandene Dateien | ✅ bestanden |
| S3 | Zweite Regel in denselben Ordner wird abgelehnt | ✅ bestanden |
| S4 | Regel in einen Backup-Ordner wird abgelehnt | ✅ bestanden |
| S5 | Neue Dateien (Browser und Netzlaufwerk) kommen von selbst an | ✅ bestanden |
| S6 | Gelöschtes verschwindet auf dem NAS, alte Version bleibt | ✅ bestanden |
| S7 | Status der Regel: aktuell, ohne Fehler | ✅ bestanden |
| S8 | Benutzer darf nur freigegebene NAS wählen | ✅ bestanden |
| S9 | Benutzer synchronisiert seinen Privat-Ordner | ✅ bestanden |
| S10 | …in seinen eigenen Ordner auf dem NAS | ✅ bestanden |
| S11 | Benutzer sieht nur seine eigenen Regeln | ✅ bestanden |
| S12 | Admin sieht alle Regeln | ✅ bestanden |
| S13 | Zielordner mit fremden Dateien: erst nachfragen | ✅ bestanden |
| S14 | Fremde Dateien bleiben unangetastet | ✅ bestanden |
| S15 | Pausierte Regel überträgt nichts | ✅ bestanden |

### W Eingang: automatisch einsortieren

Fotos (Datum, Ort per GPS, Live Photos) ohne KI, Dokumente mit lokaler KI (Ollama, im Test das kleinste Modell `qwen2.5:0.5b`), Verlauf und Rückgängig.


| Nr. | Test | Ergebnis |
|---|---|---|
| W1 | Eingangs-Ordner werden angelegt (Gemeinsam und pro Benutzer) | ✅ bestanden |
| W2 | Foto landet nach Aufnahmedatum in Fotos/2024/2024-07 | ✅ bestanden |
| W3 | Video mit GPS: Ordner mit Ortsname (offline erkannt) | ✅ bestanden |
| W4 | Live Photo: Foto und Video ziehen zusammen um | ✅ bestanden |
| W5 | Privater Eingang sortiert in den eigenen Privat-Ordner | ✅ bestanden |
| W6 | Eingang ist danach leer | ✅ bestanden |
| W7 | Verlauf zeigt, was wohin kam | ✅ bestanden |
| W8 | Fremder privater Verlauf bleibt unsichtbar | ✅ bestanden |
| W9 | Rückgängig: Datei zurück nach Eingang/Zurückgelegt | ✅ bestanden |
| W10 | Zurückgelegtes wird nicht erneut sortiert | ✅ bestanden |
| W11 | Texterkennung (OCR) für Scans im Image vorhanden | ✅ bestanden |
| W12 | KI-Modell wird geladen und ist bereit | ✅ bestanden |
| W13 | Dokument wird von der KI benannt und in Dokumente/ einsortiert | ✅ bestanden |
| W14 | Neuer Name statt scan_0042 | ✅ bestanden |

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

## Code-Prüfung vor der Veröffentlichung

Nach dem ersten grünen Durchlauf wurde der gesamte Code noch einmal gezielt auf Fehler durchgesehen: Backend (Sicherheit, Datenverlust, gleichzeitige Zugriffe), Oberfläche (Abläufe, Zustände) sowie Docker, Installationsskript und GitHub Actions. Gefunden wurden keine Sicherheitslücken wie Pfad-Ausbrüche, fremde Privat-Ordner oder Befehlsinjektion, aber 38 Fehler in Randfällen. 37 sind behoben. Einer entfällt: ARM-Runner gibt es bei GitHub kostenlos nur für öffentliche Repositories, und homevault ist öffentlich. Für die wichtigsten gibt es neue Tests (E11, F11, M5, N22–N27).

**Datensicherheit**
- Die Festplatten fehlen beim Start ganz: Früher blieb die Oberfläche für immer unerreichbar. Jetzt startet sie schreibgeschützt mit Warnung, und das Netzlaufwerk bleibt aus (sonst hätte es den alten Ordner unter dem Spiegel gezeigt). Sobald die Platten wieder da sind, hängt der Platten-Dienst den Spiegel selbst ein und alles läuft wieder normal (N22–N27).
- Das Einrichten des Spiegels schlägt fehl: Früher blieb homevault dauerhaft im Wartungsmodus. Jetzt geht es normal weiter. Während des Umzugs pausiert auch das Netzlaufwerk, damit nichts verloren geht.
- Medien-Index: Ein kurz unlesbarer Ordner (z. B. Plattenfehler) oder ein gleichzeitiges Löschen/Wiederherstellen konnte Favoriten, Alben und Teilen-Links löschen. Jetzt wird in diesem Fall nichts als gelöscht gewertet.
- Gleichzeitige Uploads gleichen Namens überschreiben sich nicht mehr gegenseitig (atomare Namensvergabe, E11).
- Das Wiederherstellen aus dem Papierkorb überschreibt kein neueres Live-Video gleichen Namens mehr.
- Wird ein Foto auf den Namen seines eigenen Live-Videos umbenannt, ging es früher verloren. Das wird jetzt abgelehnt (F11).
- Abgebrochene Uploads hinterlassen keine unsichtbaren Reste mehr, die das Speicherlimit belegen. Wird ein Benutzer gelöscht, wird auch sein privater Papierkorb geleert.
- Ein doppelt gestarteter Backup wird jetzt zuverlässig abgelehnt.

**Privatsphäre**
- Teilen-Links liefern nur noch Dateien aus, die der Ersteller selbst sehen darf. Verschiebt jemand eine geteilte Datei in seinen Privat-Ordner, ist sie über den Link nicht mehr erreichbar.
- Der Fortschritt einer Komplett-Wiederherstellung (mit Ordnernamen) ist nach der Einrichtung nicht mehr ohne Anmeldung abrufbar (M5).

**Oberfläche**
- Nach einem Ordnerwechsel zeigte die Vollbildansicht ein Foto aus dem falschen Ordner.
- Die Zeitleiste sprang während der Indizierung alle 3 Sekunden an den Anfang.
- Die Pfeiltasten wirkten auch beim Vorspulen im Video und in Eingabefeldern.
- Uploads laufen jetzt weiter, wenn man den Ordner oder die Ansicht wechselt. Beim Schließen der Seite gibt es eine Warnung.
- Beim Spiegel-Einrichten und bei der Komplett-Wiederherstellung wurde „fertig“ zu früh gemeldet.
- Beim Login meldete ein nicht erreichbarer Server „Passwort falsch“.
- Ein ungültiges Speicherlimit wurde stillschweigend zu „kein Limit“.
- Kaputte Adressen ließen die App abstürzen.
- Behoben sind außerdem Zustandsfehler in den Dialogen für Wiederherstellen, Benutzer anlegen und Festplatten.

**Installation und Veröffentlichung**
- `install.sh` installiert jetzt Docker Compose v2 passend zum System: Debian 13 und Ubuntu aus den eigenen Paketquellen, Debian 12 über Dockers Paketquelle. Die Paketerkennung wurde in Containern mit Debian 12/13 und Ubuntu 22.04/24.04 geprüft.
- Bricht `install.sh` ab, beginnt der nächste Aufruf wieder als Neuinstallation statt als „Aktualisierung“ ohne Fragen.
- GitHub Action: Das Haupt-Image schlug beim Zusammenfassen fehl, weil das Suchmuster `digest-homevault-*` auch die Teile von `homevault-disks` und `homevault-samba` traf. Die Images sind jetzt außerdem mit dem Repository verknüpft.
- `ADMIN_USER`/`ADMIN_PASSWORD` und `INDEX_INTERVAL_MINUTES` aus der README werden jetzt auch wirklich an den Container übergeben.
- Der Platzhalter-Token aus der Vorlage wird abgelehnt.

## Qualität der KI (Eingang)

Lokal mit dem Standardmodell `qwen2.5:3b` geprüft (im Debian-Test nur das kleine Modell, weil es um den Ablauf geht):

| Datei | einsortiert als |
|---|---|
| `Dokument1.docx` (Steuerbescheid) | `Steuer/2026-02-04 Finanzamt Einkommensteuer und Solidaritätszuschlag 2024.docx` |
| `scan_0042.txt` (Stromrechnung) | `Rechnungen/2026-03-12 Stadtwerke München GmbH Stromrechnung Februar.txt` |
| `vertrag.pdf` | `Verträge/Vertrag Test.pdf` |
| `notiz.txt` (ein Wort) | `Unsortiert/notiz.txt` (zu wenig Text, kein Raten) |

## Nicht getestet (Grenzen dieses Tests)

| Bereich | Warum nicht | Risiko / Empfehlung |
|---|---|---|
| Echte Festplatten, echter Hot-Swap-Wechselrahmen | Auf einem Mac nicht möglich; getestet mit Loop-Geräten über denselben Kernel-Weg (Btrfs, mount, replace). | Ob das Ein- und Ausstecken im Betrieb klappt, hängt vom Controller ab (AHCI-Hotplug im BIOS aktivieren). Einmal mit echten Platten üben. |
| SMART-Werte | Loop-Geräte haben keine SMART-Daten. | Anzeige auf echter Hardware prüfen. |
| Windows-Explorer, macOS Finder, iPhone-Dateien-App | Getestet mit `smbclient` (gleiches SMB-Protokoll). | Gering. |
| Browser auf iPhone/Android | Oberfläche im Desktop-Browser und in Handy-Größe geprüft, nicht auf echten Geräten. | Gering bis mittel (Long-Press, PDF-Ansicht auf iOS). |
| Große Bibliotheken (z. B. 50.000 Fotos, mehrere TB) | Testdaten nur wenige MB. | Erster Index-Durchlauf, Video-Umwandlung und Spiegel-Anlage dauern dann deutlich länger. Spiegel am besten anlegen, bevor viele Daten da sind. |
| Zeitgesteuerte Abläufe (Backup um 3 Uhr, monatliche Datenprüfung, Papierkorb nach 30 Tagen) | Zeitplan-Logik per Unit-Test geprüft, nicht über echte Tage. | Nach der ersten Nacht den Backup-Verlauf ansehen. |
| KI-Geschwindigkeit auf dem echten Server | Getestet auf schnellerer Hardware. | Auf einem älteren Core i3 ca. 30–90 s pro Dokument; läuft im Hintergrund. |
| Scanner, die per SMB in den Eingang speichern | Mit `smbclient` nachgestellt. | Gering; Scanner auf SMB2/3 und PDF stellen. |
| Stromausfall während Spiegel-Anlage oder Plattentausch | Nicht simuliert. | Diese Vorgänge nur mit stabiler Stromversorgung starten (USV empfohlen). |
| ARM64 (z. B. Raspberry Pi), `install.sh` komplett, GitHub-Actions | Brauchen die veröffentlichten Images auf GitHub. | Nach dem ersten erfolgreichen Workflow-Lauf `install.sh` auf einem frischen Server ausführen. |

