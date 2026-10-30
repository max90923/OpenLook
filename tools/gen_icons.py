#!/usr/bin/env python3
"""Copy the Fluent UI System Icons (MIT, Microsoft) used by MailCraft into
assets/icons and generate src/ui/icons.rs.

Usage: tools/gen_icons.py <path to @fluentui/svg-icons package>/icons
Colour icons are taken as they are; single-colour ("regular") icons get the
Outlook accent colour given below.
"""
import os, sys

SRC = sys.argv[1]
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "icons")

BLUE, PURPLE, RED, YELLOW, GREEN, GREY = "#0F6CBD", "#8764B8", "#C4314B", "#E8A33D", "#107C10", "#616161"

# phosphor constant (used throughout the UI code) -> (fluent icon, colour for regular icons)
MAP = [
    ("ENVELOPE_SIMPLE", "mail", None),
    ("ENVELOPE_SIMPLE_OPEN", "mail_read", BLUE),
    ("FILE_PLUS", "document_add", None),
    ("X", "delete", RED),
    ("ARCHIVE", "archive", GREY),
    ("PROHIBIT", "mail_prohibited", RED),
    ("TRAY", "mail_inbox", BLUE),
    ("ARROW_BEND_UP_LEFT", "arrow_reply", PURPLE),
    ("ARROW_BEND_DOUBLE_UP_LEFT", "arrow_reply_all", PURPLE),
    ("ARROW_BEND_UP_RIGHT", "arrow_forward", PURPLE),
    ("FOLDER_SIMPLE", "folder_arrow_right", YELLOW),
    ("FOLDER_PLUS", "folder_add", YELLOW),
    ("FOLDER_MINUS", "folder_prohibited", RED),
    ("FOLDER_OPEN", "folder_open", YELLOW),
    ("FUNNEL_SIMPLE", "filter", BLUE),
    ("FUNNEL", "filter", BLUE),
    ("PRINTER", "print", GREY),
    ("TAG", "tag_multiple", PURPLE),
    ("FLAG", "flag", None),
    ("PUSH_PIN", "pin", None),
    ("ADDRESS_BOOK", "book_contacts", None),
    ("CALENDAR_PLUS", "calendar", None),
    ("CALENDAR_BLANK", "calendar_month", BLUE),
    ("CALENDAR_CHECK", "calendar_checkmark", None),
    ("CALENDAR_DOTS", "calendar_week_numbers", BLUE),
    ("USERS_THREE", "calendar_people", None),
    ("SQUARE", "calendar_day", BLUE),
    ("COLUMNS", "calendar_work_week", BLUE),
    ("SQUARES_FOUR", "calendar_week_start", BLUE),
    ("USER_PLUS", "person_add", None),
    ("USER", "person", None),
    ("USERS", "people", None),
    ("USERS_FOUR", "people_add", None),
    ("USER_MINUS", "person_delete", RED),
    ("USER_CIRCLE", "person", None),
    ("USER_CHECK", "person_available", None),
    ("IDENTIFICATION_CARD", "contact_card", None),
    ("LIST", "text_bullet_list_ltr", BLUE),
    ("LIST_CHECKS", "clipboard_task", None),
    ("STAR", "star", None),
    ("CHECK_SQUARE_OFFSET", "clipboard_task", None),
    ("CHECK", "checkmark_circle", None),
    ("FLOPPY_DISK", "save", PURPLE),
    ("REPEAT", "arrow_repeat_all", BLUE),
    ("CLOCK", "clock", None),
    ("BELL", "alert", None),
    ("ARROWS_CLOCKWISE", "arrow_sync", None),
    ("PENCIL_LINE", "rename", BLUE),
    ("GEAR", "settings", None),
    ("PALETTE", "paint_brush", None),
    ("SIDEBAR", "panel_left", BLUE),
    ("LAYOUT", "panel_right", BLUE),
    ("ROWS", "preview_link", BLUE),
    ("CHATS", "chat_multiple", None),
    ("MAGNIFYING_GLASS_PLUS", "zoom_in", BLUE),
    ("KEYBOARD", "keyboard", GREY),
    ("INFO", "info", BLUE),
    ("QUESTION", "question_circle", None),
    ("PAPERCLIP", "attach", BLUE),
    ("SIGNATURE", "signature", BLUE),
    ("PAPER_PLANE_RIGHT", "send", None),
    ("NOTE", "note", YELLOW),
    ("BOOK_OPEN", "book_open", None),
    ("TIMER", "timer", GREEN),
    ("STOP", "record_stop", RED),
    ("PHONE_CALL", "call", BLUE),
    ("AIRPLANE_TILT", "mail_clock", None),
    ("BROOM", "broom", BLUE),
    ("PLUS", "add", BLUE),
    ("WARNING", "warning", None),
    ("GLOBE", "globe", None),
    ("LIGHTBULB", "lightbulb", None),
    ("FILE_TEXT", "document_text", None),
    ("FILE", "document", None),
    ("FILE_IMAGE", "image", None),
    ("FILE_PDF", "document_pdf", RED),
    ("TEXT_B", "text_bold", GREY),
    ("TEXT_ITALIC", "text_italic", GREY),
    ("TEXT_UNDERLINE", "text_underline", GREY),
    ("TEXT_AA", "text_case_title", BLUE),
    ("LIST_BULLETS", "text_bullet_list_ltr", GREY),
    ("LIST_NUMBERS", "text_number_list_ltr", GREY),
    ("ARROWS_DOWN_UP", "arrow_sort", BLUE),
    ("COPY", "copy", BLUE),
    ("PENCIL_SIMPLE", "edit", None),
    ("MICROSOFT_OUTLOOK_LOGO", "mail_multiple", None),
    ("PLAY", "play_circle", GREEN),
    ("SIGN_OUT", "sign_out", GREY),
    ("LIGHTNING", "flash", YELLOW),
]

def find(base, size):
    for variant in ("color", "regular"):
        for s in (size, 24, 20, 32, 28, 16):
            p = os.path.join(SRC, f"{base}_{s}_{variant}.svg")
            if os.path.exists(p):
                return p, variant
    sys.exit(f"Symbol fehlt: {base}")

os.makedirs(OUT, exist_ok=True)
rows = []
for key, base, colour in MAP:
    files = []
    for size in (20, 32):
        path, variant = find(base, size)
        svg = open(path, encoding="utf-8").read()
        if variant == "regular" and colour:
            svg = svg.replace("<svg ", f'<svg fill="{colour}" ', 1)
        name = f"{key.lower()}_{size}.svg"
        open(os.path.join(OUT, name), "w", encoding="utf-8").write(svg)
        files.append(name)
    rows.append((key, files[0], files[1]))

with open(os.path.join(ROOT, "src", "ui", "icons.rs"), "w", encoding="utf-8") as f:
    f.write("//! Generated by tools/gen_icons.py – Fluent UI System Icons (MIT, Microsoft),\n")
    f.write("//! looked up by the Phosphor glyph the UI code uses.\n\n")
    f.write("use egui_phosphor::regular as ph;\n\n")
    f.write("/// (glyph, uri small, svg small, uri large, svg large)\n")
    f.write("type Entry = (&'static str, &'static str, &'static [u8], &'static str, &'static [u8]);\n\n")
    f.write("static ICONS: &[Entry] = &[\n")
    for key, small, large in rows:
        f.write(f'    (ph::{key}, "bytes://icons/{small}", include_bytes!("../../assets/icons/{small}"), '
                f'"bytes://icons/{large}", include_bytes!("../../assets/icons/{large}")),\n')
    f.write("];\n\n")
    f.write("/// SVG for a glyph: (uri, bytes); `large` picks the 32 px drawing.\n")
    f.write("pub fn svg(glyph: &str, large: bool) -> Option<(&'static str, &'static [u8])> {\n")
    f.write("    ICONS.iter().find(|e| e.0 == glyph).map(|e| if large { (e.3, e.4) } else { (e.1, e.2) })\n")
    f.write("}\n")
print(f"{len(rows)} Symbole erzeugt")
