# MailCraft

Ein nativer, Outlook-ähnlicher Desktop-Client – **ein einziges Rust-Programm**, kein Server, kein Browser.
E-Mail, Kalender, Personen und Aufgaben in einem Fenster, mit lokaler SQLite-Datenbank als Offline-Cache.

| Modul | Funktionen |
|---|---|
| **E-Mail** | Ordnerbereich mit Favoriten, Nachrichtenliste mit Datumsgruppen (Heute, Gestern, Diese Woche …), Lesebereich, Antworten / Allen antworten / Weiterleiten, Entwürfe, Löschen → „Gelöschte Elemente“, Archivieren, Junk, Verschieben (auch per Drag & Drop), Gelesen/Ungelesen, Kennzeichnen, Anheften, Farbkategorien, Wichtigkeit, Suche über alle Ordner, Filter (Ungelesen / Gekennzeichnet), Empfänger-Vorschläge aus dem Adressbuch, Signatur, Anzeige des Original-HTML im Browser |
| **Kalender** | Tag, Arbeitswoche, Woche, Monat, Mini-Monatskalender, Jetzt-Linie, überlappende Termine nebeneinander, ganztägige Termine, Termin per Doppelklick anlegen, Ort, Teilnehmer, „Anzeigen als“, Erinnerung, Kategorie |
| **Personen** | Kontaktliste mit Buchstabengruppen, Profilkarte, Favoriten, anlegen/bearbeiten/löschen, „E-Mail senden“ |
| **To Do** | Mein Tag, Wichtig, Geplant, Aufgaben + alle Microsoft-To-Do-Listen, Fälligkeitsdatum, Notizen, erledigte Aufgaben ein-/ausklappen |
| **Allgemein** | Hell/Dunkel, Hintergrund-Synchronisierung, Tastenkürzel (N, R, Umschalt+R, F, Entf, Q, Einfg, ↑/↓, /, Strg+Enter, Strg+1…4, F9) |

## Konten

* **Lokales Postfach** – immer vorhanden, mit Beispieldaten; funktioniert komplett offline.
* **IMAP/SMTP** – jedes Postfach (Gmail, GMX, WEB.DE, T-Online, iCloud, Posteo, Yahoo … mit Vorlagen).
  IMAP über SSL/TLS (Port 993), SMTP über STARTTLS (587) oder SSL/TLS (465).
  Kalender, Kontakte und Aufgaben werden bei IMAP-Konten lokal gespeichert.
* **Microsoft 365 / Outlook.com** – über Microsoft Graph: E-Mail, Kalender, Kontakte und Microsoft To Do
  werden synchronisiert; Änderungen gehen direkt an den Server.

### Microsoft-Anmeldung einrichten

MailCraft nutzt den OAuth-Gerätecode-Flow (kein Redirect, kein Client-Secret):

1. Im [Entra Admin Center](https://entra.microsoft.com) → *App-Registrierungen* → *Neue Registrierung*.
   Kontotyp: „Konten in einem beliebigen Organisationsverzeichnis und persönliche Microsoft-Konten“.
2. Unter *Authentifizierung* „Öffentliche Clientflows zulassen“ aktivieren.
3. Unter *API-Berechtigungen* (Microsoft Graph, delegiert) hinzufügen:
   `User.Read Mail.ReadWrite Mail.Send Calendars.ReadWrite Contacts.ReadWrite Tasks.ReadWrite offline_access`.
4. Die *Anwendungs-ID (Client-ID)* als Umgebungsvariable setzen und MailCraft starten:

```sh
export MAILCRAFT_GRAPH_CLIENT_ID=<client-id>
cargo run --release
```

Dann *Einstellungen → Konten → Microsoft 365 / Outlook.com verbinden*, den angezeigten Code auf der Microsoft-Seite eingeben – fertig.

## Bauen & starten

```sh
cargo run --release
```

Unter Linux werden zur Laufzeit die üblichen Desktop-Bibliotheken benötigt (OpenGL, `libxkbcommon-x11` bzw. Wayland) – auf normalen Desktops sind sie vorhanden.

| Variable | Standard | Bedeutung |
|---|---|---|
| `MAILCRAFT_DB` | `<Datenverzeichnis>/MailCraft/mailcraft.db` | Pfad der Datenbank |
| `MAILCRAFT_GRAPH_CLIENT_ID` | – | Client-ID für die Microsoft-Anmeldung |
| `MAILCRAFT_GRAPH_TENANT` | `common` | `common`, `organizations`, `consumers` oder Tenant-ID |
| `MAILCRAFT_SYNC_MINUTES` | `5` | Intervall der Hintergrund-Synchronisierung |

## Aufbau

```
src/
  main.rs            Start: Datenbank, Tokio-Runtime, Fenster
  ui/                egui-Oberfläche (mod.rs = Fensterrahmen, je Modul eine Datei)
  providers/         imap.rs (IMAP/SMTP), graph.rs (Microsoft Graph), mod.rs (Verteilung)
  db.rs              SQLite-Schema und Abfragen
  mail.rs            MIME-Parsing, HTML→Text
  seed.rs            Beispieldaten des lokalen Postfachs
```

Die Oberfläche liest nur aus der lokalen Datenbank; alles mit Netzwerkzugriff läuft im Hintergrund und meldet das Ergebnis über einen Kanal zurück, damit das Fenster nie hängt.

## Bekannte Grenzen (v0.1)

* E-Mails werden im Lesebereich als Text dargestellt; das Original-HTML öffnet sich per 🌐 im Browser.
* Anlagen werden angezeigt (📎), aber noch nicht gespeichert/angehängt.
* Pro Ordner werden die neuesten 100 Nachrichten zwischengespeichert.
* Zugangsdaten (IMAP-Passwort, Microsoft-Token) liegen unverschlüsselt in der lokalen Datenbank.
* Noch nicht enthalten: Regeln, Abwesenheitsnotiz, Besprechungsanfragen annehmen/ablehnen, Serientermine anlegen, freigegebene Postfächer, Suchordner.
