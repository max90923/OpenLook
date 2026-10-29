# MailCraft

Ein nativer Desktop-Client für E-Mail, Kalender, Kontakte und Aufgaben – aufgebaut wie das **klassische Outlook** (2016/2019/365) und als **ein einziges Rust-Programm**: kein Server, kein Browser, eigene Fenster.

## Oberfläche

* **Eigene Titelleiste** mit Symbolleiste für den Schnellzugriff (Senden/Empfangen, Menüband ein/aus), Fenstertitel „Posteingang - konto - MailCraft“, Minimieren/Maximieren/Schließen.
* **Menüband** mit Registerkarten *Datei · Start · Senden/Empfangen · Ordner · Ansicht · Hilfe*, Gruppen mit großen und kleinen Schaltflächen, Drop-down-Menüs und „Was möchten Sie tun?“.
* **Datei-Backstage**: Kontoinformationen, Konto hinzufügen, Kontoeinstellungen (synchronisieren, entfernen), Postfachtools („Gelöschte Elemente“ leeren), Optionen, Beenden.
* **Ordnerbereich** mit Favoriten und Ordnerbaum pro Konto (Ungelesen-Zähler, Entwürfe in [ ]), minimierbar, unten die Modulleiste *E-Mail · Kalender · Personen · Aufgaben*.
* **Statusleiste** mit Elemente/Ungelesen, Sync-Status, Verbindungsstatus und Zoom-Regler.
* **Eigene Betriebssystem-Fenster** wie in Outlook für: Nachricht lesen, Nachricht verfassen, Termin/Besprechung, Kontakt, Aufgabe, Optionen, Konto hinzufügen und Erinnerungen.
* Designs **Bunt** und **Schwarz**.

## Module

| Modul | Funktionen |
|---|---|
| **E-Mail** | HTML-Anzeige (fett, kursiv, Links, Überschriften, Listen, Zitate, Tabellen), Anlagen öffnen/speichern/alle speichern, Unterhaltungsansicht, Ordner anlegen/umbenennen/löschen, Regeln, Drucken (`Strg+P`), Nachrichtenliste mit Gruppen (Heute, Gestern, Wochentag, Letzte Woche …), Alle/Ungelesen, Sortierung nach Datum/Von/Betreff, Suche im aktuellen Ordner oder in allen Postfächern, Lesebereich rechts/unten/aus, Nachrichtenvorschau, Antworten/Allen antworten/Weiterleiten, Löschen → „Gelöschte Elemente“, Archivieren, Junk-E-Mail, Verschieben (auch per Drag & Drop), Ungelesen/Gelesen, Kategorien, Zur Nachverfolgung, Anheften, Doppelklick öffnet die Nachricht im eigenen Fenster, Original-HTML im Browser |
| **Verfassen** | Formatierung (fett, kursiv, unterstrichen, Aufzählung, Nummerierung – wird beim Tippen formatiert angezeigt und als HTML gesendet), Datei anfügen, Weiterleiten mit Anlagen, Senden-Schaltfläche links, Von/An/Cc/Bcc, Adressbuch, Namen überprüfen, AutoVervollständigen aus den Kontakten, Signatur, Wichtigkeit hoch/niedrig, Entwürfe, Nachfrage „Änderungen speichern?“ |
| **Kalender** | Datumsnavigator (zwei Monate), Tag/Arbeitswoche/Woche/Monat, Heute, Nächste 7 Tage, Arbeitszeit hervorgehoben, Jetzt-Linie, überlappende Termine nebeneinander, ganztägige Ereignisse, Kalender ein-/ausblenden, Termin und Besprechung (Teilnehmer), Serientermine (täglich, wöchentlich mit Wochentagen, monatlich, jährlich, mit/ohne Enddatum), Besprechungsanfragen annehmen/mit Vorbehalt/ablehnen (Microsoft 365), Anzeigen als, Erinnerung, Kategorien |
| **Regeln** | „Regeln und Benachrichtigungen“: Bedingung (Absender, Betreff, Empfänger, Text enthält) → Aktion (verschieben, als gelesen markieren, kennzeichnen, Kategorie, löschen); gelten für neue Nachrichten nach jedem Senden/Empfangen, „Regeln jetzt anwenden“ für den ganzen Posteingang |
| **Automatische Antworten** | Abwesenheitsnotiz mit Zeitraum und getrennten Texten für intern/extern (Microsoft 365) |
| **Erinnerungen** | Erinnerungsfenster mit „Alle schließen“, „Element öffnen“, „Schließen“ und „Erneut erinnern“ |
| **Personen** | Ansichten Personen, Visitenkarte und Liste; Kontaktfenster mit Visitenkarten-Vorschau; Favoriten; E-Mail/Besprechung an Kontakt |
| **Aufgaben** | Tabellenansicht mit „Klicken Sie hier, um eine neue Aufgabe zu erstellen“, Fällig am, In Ordner, Nachverfolgung (Heute, Morgen, Diese Woche, Nächste Woche, Kein Datum), Ansichten Einfache Liste/Aktiv/Erledigt/Heute/Überfällig, Aufgabenfenster |

Tastenkombinationen wie in Outlook: `Strg+N` neues Element, `Strg+R` / `Strg+Umschalt+R` antworten / allen antworten, `Strg+F` weiterleiten, `Entf` löschen, `Strg+Q` / `Strg+U` gelesen / ungelesen, `Einfg` kennzeichnen, `Strg+E` suchen, `Alt+S` senden, `Strg+1…4` Module, `F9` Senden/Empfangen, `Strg+F1` Menüband.

## Konten

* **Lokales Postfach** – immer vorhanden, mit Beispieldaten; funktioniert komplett offline.
* **IMAP/SMTP** – jedes Postfach; für Gmail, GMX, WEB.DE, T-Online, iCloud, Posteo, Yahoo, mailbox.org, freenet und AOL werden die Server automatisch eingetragen. IMAP über SSL/TLS (993), SMTP über STARTTLS (587) oder SSL/TLS (465). Kalender, Kontakte und Aufgaben werden bei IMAP-Konten lokal gespeichert.
* **Microsoft 365 / Outlook.com** – über Microsoft Graph: E-Mail, Kalender, Kontakte und Microsoft To Do werden synchronisiert; Änderungen gehen direkt an den Server.

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

Dann *Datei → Konto hinzufügen → Erweiterte Optionen → Microsoft 365*, den angezeigten Code auf der Microsoft-Seite eingeben – fertig.

## Bauen & starten

```sh
cargo run --release
```

Ergebnis ist eine einzelne ausführbare Datei (`target/release/mailcraft`, unter Windows `mailcraft.exe`). Unter Windows wird automatisch die Systemschrift Segoe UI verwendet. Unter Linux werden zur Laufzeit die üblichen Desktop-Bibliotheken benötigt (OpenGL, `libxkbcommon-x11` bzw. Wayland).

| Variable | Standard | Bedeutung |
|---|---|---|
| `MAILCRAFT_DB` | `<Datenverzeichnis>/MailCraft/mailcraft.db` | Pfad der Datenbank |
| `MAILCRAFT_GRAPH_CLIENT_ID` | – | Client-ID für die Microsoft-Anmeldung |
| `MAILCRAFT_GRAPH_TENANT` | `common` | `common`, `organizations`, `consumers` oder Tenant-ID |
| `MAILCRAFT_SYNC_MINUTES` | `5` | Intervall für automatisches Senden/Empfangen |

## Aufbau

```
src/
  main.rs            Start: Datenbank, Tokio-Runtime, Hauptfenster
  ui/
    mod.rs           Hauptfenster: Menüband, Ordnerbereich, Statusleiste, Fensterverwaltung
    chrome.rs        Titelleiste, Menüband-Bausteine, native Unterfenster
    theme.rs         Designs „Bunt“ und „Schwarz“
    mail.rs          E-Mail-Modul    compose.rs   Nachrichtenfenster
    calendar.rs      Kalender + Terminfenster      reminders.rs Erinnerungen
    people.rs        Personen + Kontaktfenster     tasks.rs     Aufgaben + Aufgabenfenster
    backstage.rs     Datei-Backstage, Optionen, „Konto hinzufügen“
  providers/         imap.rs (IMAP/SMTP), graph.rs (Microsoft Graph), mod.rs (Verteilung)
  db.rs              SQLite-Schema und Abfragen
  mail.rs            MIME-Parsing, HTML→Text
  seed.rs            Beispieldaten des lokalen Postfachs
```

Die Oberfläche liest nur aus der lokalen Datenbank; alles mit Netzwerkzugriff läuft im Hintergrund, damit das Fenster nie hängt. Symbole: [Phosphor Icons](https://phosphoricons.com) (MIT).

## Sicherheit

IMAP-Kennwörter und Microsoft-Tokens werden im Schlüsselbund des Betriebssystems gespeichert (Windows-Anmeldeinformationsverwaltung, macOS-Schlüsselbund, unter Linux Secret Service/GNOME Keyring/KWallet). Ältere Installationen werden beim Start automatisch umgestellt. Ist kein Schlüsselbund verfügbar, bleiben die Daten in der lokalen Datenbank.

## Bekannte Grenzen

* Die Formatierung beim Verfassen ist auf fett, kursiv, unterstrichen und Listen beschränkt (keine Schriftarten, Farben oder Bilder im Text).
* Im Lesebereich werden Bilder in HTML-Mails als Platzhalter angezeigt; das Original öffnet sich per 🌐 im Browser.
* Pro Ordner werden die neuesten 100 Nachrichten zwischengespeichert.
* Abwesenheitsnotizen und Besprechungsantworten gibt es nur für Microsoft-365-Konten (IMAP hat dafür keinen Standard).
* Regeln laufen im Client (wie Outlooks „nur auf diesem Computer“-Regeln), also nur, wenn MailCraft geöffnet ist.
* Nicht enthalten: freigegebene Postfächer und Kalender anderer Personen, Exchange-Öffentliche Ordner, Notizen und Journal.
