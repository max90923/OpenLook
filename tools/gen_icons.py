#!/usr/bin/env python3
"""Generate assets/icons and src/ui/icons.rs from System UIcons
(https://github.com/CoreyGinnivan/system-uicons, Unlicense - public domain).

Usage: tools/gen_icons.py <path to @iconify-json/system-uicons>/icons.json
Every icon is drawn as a line icon in the given Outlook accent colour;
"name:filled" also fills the shape.
"""
import json, os, sys

ICONS = json.load(open(sys.argv[1], encoding="utf-8"))["icons"]
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "icons")

BLUE, PURPLE, RED, YELLOW, GREEN, GREY = "#0F6CBD", "#8764B8", "#C4314B", "#E8A33D", "#107C10", "#616161"

# Reply arrows drawn for OpenLook (public domain like the rest).
ARROW = '<path fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round" d="{}"/>'
CUSTOM = {
    "reply": ARROW.format("M8.5 5.5l-5 5l5 5m-5-5h8a6 6 0 0 1 6 6v1"),
    "reply-all": ARROW.format("M10.5 5.5l-5 5l5 5m-5-5h6a6 6 0 0 1 6 6v1M6.5 5.5l-5 5l5 5"),
    "forward": ARROW.format("M12.5 5.5l5 5l-5 5m5-5h-8a6 6 0 0 0-6 6v1"),
}

# phosphor constant (used throughout the UI code) -> (System UIcons name, colour)
MAP = [
    ("ENVELOPE_SIMPLE", "mail", BLUE),
    ("ENVELOPE_SIMPLE_OPEN", "mail-open", BLUE),
    ("FILE_PLUS", "create", BLUE),
    ("X", "trash", GREY),
    ("TRASH", "trash", GREY),
    ("X_CIRCLE", "cross", RED),
    ("FLAG", "flag:filled", RED),
    ("FLAG_BANNER", "flag", GREY),
    ("PAPERCLIP", "paperclip", BLUE),
    ("PAPERCLIP_HORIZONTAL", "paperclip", GREY),
    ("ARCHIVE", "archive", GREY),
    ("PROHIBIT", "no-sign", RED),
    ("TRAY", "inbox", BLUE),
    ("ARROW_BEND_UP_LEFT", "reply", PURPLE),
    ("ARROW_BEND_DOUBLE_UP_LEFT", "reply-all", PURPLE),
    ("ARROW_BEND_UP_RIGHT", "forward", PURPLE),
    ("FOLDER_SIMPLE", "folder-closed", YELLOW),
    ("FOLDER_PLUS", "folder-add", YELLOW),
    ("FOLDER_MINUS", "folder-minus", RED),
    ("FOLDER_OPEN", "folder-open", YELLOW),
    ("FUNNEL_SIMPLE", "filter", BLUE),
    ("FUNNEL", "funnel", BLUE),
    ("PRINTER", "printer", GREY),
    ("TAG", "tags", PURPLE),
    ("PUSH_PIN", "bookmark", BLUE),
    ("ADDRESS_BOOK", "contacts", BLUE),
    ("CALENDAR_PLUS", "calendar-add", BLUE),
    ("CALENDAR_BLANK", "calendar-month", BLUE),
    ("CALENDAR_CHECK", "calendar-date", GREEN),
    ("CALENDAR_DOTS", "calendar-days", BLUE),
    ("GRID_FOUR", "calendar-split", BLUE),
    ("SQUARE", "calendar-day", BLUE),
    ("COLUMNS", "calendar-week", BLUE),
    ("SQUARES_FOUR", "grid-squares", BLUE),
    ("USERS_THREE", "users", BLUE),
    ("USER_PLUS", "user-add", BLUE),
    ("USER_MINUS", "user-remove", RED),
    ("USER", "user", BLUE),
    ("USERS", "users", BLUE),
    ("USERS_FOUR", "users", PURPLE),
    ("USER_CIRCLE", "user-circle", BLUE),
    ("USER_CHECK", "user-male-circle", GREEN),
    ("IDENTIFICATION_CARD", "postcard", BLUE),
    ("LIST", "list", BLUE),
    ("LIST_CHECKS", "todo", BLUE),
    ("STAR", "star", YELLOW),
    ("CHECK_SQUARE_OFFSET", "clipboard-check", BLUE),
    ("CHECK", "check-circle", GREEN),
    ("FLOPPY_DISK", "floppy", PURPLE),
    ("REPEAT", "refresh", BLUE),
    ("CLOCK", "clock", BLUE),
    ("BELL", "bell", YELLOW),
    ("ARROWS_CLOCKWISE", "refresh-alt", BLUE),
    ("PENCIL_LINE", "write", BLUE),
    ("PENCIL_SIMPLE", "pen", BLUE),
    ("GEAR", "settings", GREY),
    ("PALETTE", "sliders", PURPLE),
    ("SIDEBAR", "panel-left", BLUE),
    ("LAYOUT", "panel-right", BLUE),
    ("ROWS", "document-justified", BLUE),
    ("CHATS", "speech-bubble", BLUE),
    ("MAGNIFYING_GLASS_PLUS", "zoom-in", BLUE),
    ("KEYBOARD", "keyboard", GREY),
    ("INFO", "info-circle", BLUE),
    ("QUESTION", "question-circle", BLUE),
    ("SIGNATURE", "message-writing", BLUE),
    ("PAPER_PLANE_RIGHT", "paper-plane", BLUE),
    ("NOTE", "paper-folded", YELLOW),
    ("BOOK_OPEN", "book-text", BLUE),
    ("TIMER", "clock", GREEN),
    ("STOP", "record", RED),
    ("PHONE_CALL", "phone-portrait", BLUE),
    ("AIRPLANE_TILT", "alarm-clock", BLUE),
    ("BROOM", "mail-minus", BLUE),
    ("PLUS", "plus", BLUE),
    ("WARNING", "warning-triangle", YELLOW),
    ("GLOBE", "globe", BLUE),
    ("LIGHTBULB", "lightbulb-on", YELLOW),
    ("FILE_TEXT", "document-words", BLUE),
    ("FILE", "document", GREY),
    ("FILE_IMAGE", "picture", BLUE),
    ("FILE_PDF", "document", RED),
    ("TEXT_AA", "paragraph-left", BLUE),
    ("LIST_BULLETS", "list", GREY),
    ("LIST_NUMBERS", "list-numbered", GREY),
    ("ARROWS_DOWN_UP", "sort", BLUE),
    ("COPY", "duplicate", BLUE),
    ("PLAY", "play-button", GREEN),
    ("SIGN_OUT", "exit-right", GREY),
    ("LIGHTNING", "lightning", YELLOW),
    ("MICROSOFT_OUTLOOK_LOGO", "inbox-alt", BLUE),
]


def svg(name, colour):
    name, _, variant = name.partition(":")
    body = CUSTOM.get(name) or ICONS[name]["body"]
    if variant == "filled":
        body = body.replace('fill="none"', 'fill="currentColor"', 1)
    body = body.replace("currentColor", colour)
    return f'<svg xmlns="http://www.w3.org/2000/svg" width="21" height="21" viewBox="0 0 21 21" stroke-width="1.25">{body}</svg>'


for f in os.listdir(OUT):
    if f.endswith(".svg"):
        os.remove(os.path.join(OUT, f))
rows = []
for key, name, colour in MAP:
    file = f"{key.lower()}.svg"
    open(os.path.join(OUT, file), "w", encoding="utf-8").write(svg(name, colour))
    rows.append((key, file))

with open(os.path.join(ROOT, "src", "ui", "icons.rs"), "w", encoding="utf-8") as f:
    f.write("//! Generated by tools/gen_icons.py – System UIcons (Unlicense, public domain),\n")
    f.write("//! looked up by the Phosphor glyph the UI code uses.\n\n")
    f.write("use egui_phosphor::regular as ph;\n\n")
    f.write("/// (glyph, uri, svg)\n")
    f.write("static ICONS: &[(&str, &str, &[u8])] = &[\n")
    for key, file in rows:
        f.write(f'    (ph::{key}, "bytes://icons/{file}", include_bytes!("../../assets/icons/{file}")),\n')
    f.write("];\n\n")
    f.write("/// SVG for a glyph: (uri, bytes). The drawing scales to any size.\n")
    f.write("pub fn svg(glyph: &str, _large: bool) -> Option<(&'static str, &'static [u8])> {\n")
    f.write("    ICONS.iter().find(|e| e.0 == glyph).map(|e| (e.1, e.2))\n")
    f.write("}\n")
print(f"{len(rows)} icons generated")
