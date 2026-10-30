//! Minimal HTML rendering for the reading pane and a small formatting
//! markup for writing mails (`**bold**`, `~~italic~~`, `__underline__`,
//! `• bullet`, `1. numbering`) that is shown styled while typing
//! and sent as HTML.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, RichText, TextFormat};

#[derive(Clone, Default, PartialEq, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub mono: bool,
    pub size: f32,
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    pub runs: Vec<Run>,
    /// Nesting of lists/quotes.
    pub indent: u8,
    pub quote: bool,
    pub rule: bool,
}

fn decode_entity(name: &str) -> Option<String> {
    if let Some(num) = name.strip_prefix('#') {
        let code = if let Some(hex) = num.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            num.parse().ok()?
        };
        return char::from_u32(code).map(String::from);
    }
    Some(
        match name {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{a0}",
            "auml" => "ä",
            "ouml" => "ö",
            "uuml" => "ü",
            "Auml" => "Ä",
            "Ouml" => "Ö",
            "Uuml" => "Ü",
            "szlig" => "ß",
            "euro" => "€",
            "copy" => "©",
            "reg" => "®",
            "trade" => "™",
            "ndash" => "–",
            "mdash" => "—",
            "hellip" => "…",
            "laquo" => "«",
            "raquo" => "»",
            "bdquo" => "„",
            "ldquo" => "“",
            "rdquo" => "”",
            "lsquo" => "‘",
            "rsquo" => "’",
            "sbquo" => "‚",
            "bull" => "•",
            "middot" => "·",
            "deg" => "°",
            "shy" => "",
            "zwnj" | "zwj" => "",
            _ => return None,
        }
        .to_string(),
    )
}

fn decode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match rest[1..]
            .find(';')
            .filter(|&j| j <= 10)
            .and_then(|j| decode_entity(&rest[1..j + 1]).map(|s| (s, j)))
        {
            Some((s, j)) => {
                out.push_str(&s);
                rest = &rest[j + 2..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(name) {
        let i = from + pos;
        let before_ok = i == 0 || lower.as_bytes()[i - 1].is_ascii_whitespace();
        let after = lower[i + name.len()..].trim_start();
        if before_ok && after.starts_with('=') {
            let start = tag.len() - after.len() + 1;
            let v = tag[start..].trim_start();
            return Some(match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or("").to_string(),
                _ => v
                    .split(|c: char| c.is_whitespace() || c == '>')
                    .next()
                    .unwrap_or("")
                    .to_string(),
            });
        }
        from = i + name.len();
    }
    None
}

/// Split text into (piece, is_url) parts; http(s)://, www. and mailto: are URLs.
pub fn split_urls(text: &str) -> Vec<(String, bool)> {
    let mut out = vec![];
    let mut rest = text;
    loop {
        let start = ["https://", "http://", "www.", "mailto:"]
            .iter()
            .filter_map(|p| rest.find(p))
            .min();
        let Some(i) = start
            .filter(|&i| i == 0 || !rest[..i].ends_with(|c: char| c.is_alphanumeric() || c == '/'))
        else {
            if !rest.is_empty() {
                out.push((rest.to_string(), false));
            }
            return out;
        };
        let len = rest[i..]
            .find(|c: char| c.is_whitespace() || c == '<' || c == '>' || c == '"')
            .unwrap_or(rest.len() - i);
        // Trailing punctuation belongs to the sentence, not to the link.
        let url = rest[i..i + len].trim_end_matches(['.', ',', ';', ':', '!', '?', ')']);
        if i > 0 {
            out.push((rest[..i].to_string(), false));
        }
        out.push((url.to_string(), true));
        rest = &rest[i + url.len()..];
    }
}

fn href(url: &str) -> String {
    if url.starts_with("www.") {
        format!("https://{url}")
    } else {
        url.to_string()
    }
}

/// Make plain-text URLs in parsed blocks clickable.
fn autolink(blocks: &mut [Block]) {
    for b in blocks {
        let mut runs = vec![];
        for r in b.runs.drain(..) {
            if r.style.link.is_some() || r.style.mono {
                runs.push(r);
                continue;
            }
            for (piece, is_url) in split_urls(&r.text) {
                let mut style = r.style.clone();
                if is_url {
                    style.link = Some(href(&piece));
                }
                runs.push(Run { text: piece, style });
            }
        }
        b.runs = runs;
    }
}

/// Parse HTML into blocks of styled runs.
pub fn parse(html: &str) -> Vec<Block> {
    let mut blocks = parse_inner(html);
    autolink(&mut blocks);
    blocks
}

fn parse_inner(html: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = vec![];
    let mut cur = Block::default();
    let mut style = Style {
        size: 14.0,
        ..Default::default()
    };
    let mut stack: Vec<(String, Style)> = vec![];
    let mut lists: Vec<(bool, u32)> = vec![];
    let mut quote = 0u8;
    let mut pre = 0u8;
    let mut skip: Option<String> = None;
    let mut i = 0;
    let b = html.as_bytes();

    fn flush(blocks: &mut Vec<Block>, cur: &mut Block, indent: u8, quote: bool) {
        // Trim trailing whitespace of the block.
        while let Some(last) = cur.runs.last_mut() {
            let t = last.text.trim_end_matches([' ', '\t']).to_string();
            if t.is_empty() {
                cur.runs.pop();
            } else {
                last.text = t;
                break;
            }
        }
        if !cur.runs.is_empty() || cur.rule {
            blocks.push(std::mem::take(cur));
        } else if blocks.last().is_some_and(|b: &Block| !b.runs.is_empty()) {
            // Keep a single empty block as paragraph spacing.
            blocks.push(Block::default());
        }
        cur.indent = indent;
        cur.quote = quote;
    }

    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = html[i..].find('>') else {
                break;
            };
            let tag = &html[i + 1..i + end];
            i += end + 1;
            if tag.starts_with("!--") {
                if !tag.ends_with("--") {
                    if let Some(e) = html[i..].find("-->") {
                        i += e + 3;
                    }
                }
                continue;
            }
            let closing = tag.starts_with('/');
            let name: String = tag
                .trim_start_matches('/')
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase();
            if let Some(s) = &skip {
                if closing && &name == s {
                    skip = None;
                }
                continue;
            }
            let indent = (lists.len() as u8) + quote;
            match (name.as_str(), closing) {
                ("style" | "script" | "head" | "title", false) if !tag.ends_with('/') => {
                    skip = Some(name.clone())
                }
                ("br", _) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                }
                ("hr", false) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                    cur.rule = true;
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                }
                ("img", false) => {
                    let alt = attr(tag, "alt")
                        .filter(|a| !a.trim().is_empty())
                        .unwrap_or_else(|| "Image".into());
                    cur.runs.push(Run {
                        text: format!("[{alt}]"),
                        style: Style {
                            italic: true,
                            ..style.clone()
                        },
                    });
                }
                ("ul" | "ol", false) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                    lists.push((name == "ol", 0));
                    cur.indent = lists.len() as u8 + quote;
                }
                ("ul" | "ol", true) => {
                    lists.pop();
                    flush(&mut blocks, &mut cur, lists.len() as u8 + quote, quote > 0);
                }
                ("li", false) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                    let marker = match lists.last_mut() {
                        Some((true, n)) => {
                            *n += 1;
                            format!("{n}. ")
                        }
                        _ => "•  ".to_string(),
                    };
                    cur.runs.push(Run {
                        text: marker,
                        style: style.clone(),
                    });
                }
                ("blockquote", false) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                    quote += 1;
                    cur.indent = indent + 1;
                    cur.quote = true;
                }
                ("blockquote", true) => {
                    quote = quote.saturating_sub(1);
                    flush(&mut blocks, &mut cur, lists.len() as u8 + quote, quote > 0);
                }
                (
                    "p" | "div" | "tr" | "table" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "pre"
                    | "center" | "section" | "article" | "header" | "footer" | "address",
                    _,
                ) => {
                    flush(&mut blocks, &mut cur, indent, quote > 0);
                    if !closing {
                        stack.push((name.clone(), style.clone()));
                        match name.as_str() {
                            "h1" => (style.size, style.bold) = (22.0, true),
                            "h2" => (style.size, style.bold) = (19.0, true),
                            "h3" | "h4" => (style.size, style.bold) = (16.0, true),
                            "h5" | "h6" => style.bold = true,
                            "pre" => {
                                style.mono = true;
                                pre += 1;
                            }
                            _ => {}
                        }
                        if attr(tag, "style").is_some_and(|s| {
                            s.to_ascii_lowercase().contains("white-space:pre")
                                || s.to_ascii_lowercase().contains("white-space: pre")
                        }) {
                            pre += 1;
                            stack.last_mut().unwrap().0.push_str("+pre");
                        }
                    } else if let Some(pos) = stack
                        .iter()
                        .rposition(|(n, _)| n.split('+').next() == Some(name.as_str()))
                    {
                        let (n, s) = stack.remove(pos);
                        if name == "pre" || n.ends_with("+pre") {
                            pre = pre
                                .saturating_sub(1 + u8::from(name == "pre" && n.ends_with("+pre")));
                        }
                        style = s;
                    }
                }
                ("td" | "th", false) => {
                    if !cur.runs.is_empty() {
                        cur.runs.push(Run {
                            text: "   ".into(),
                            style: style.clone(),
                        });
                    }
                    stack.push((name.clone(), style.clone()));
                    if name == "th" {
                        style.bold = true;
                    }
                }
                (
                    "b" | "strong" | "i" | "em" | "u" | "ins" | "s" | "strike" | "del" | "code"
                    | "tt" | "a" | "span" | "font" | "small" | "big" | "sup" | "sub",
                    false,
                ) => {
                    stack.push((name.clone(), style.clone()));
                    match name.as_str() {
                        "b" | "strong" => style.bold = true,
                        "i" | "em" => style.italic = true,
                        "u" | "ins" => style.underline = true,
                        "s" | "strike" | "del" => style.strike = true,
                        "code" | "tt" => style.mono = true,
                        "small" | "sup" | "sub" => style.size = (style.size - 2.0).max(10.0),
                        "big" => style.size += 2.0,
                        "a" => style.link = attr(tag, "href").filter(|h| !h.starts_with('#')),
                        _ => {}
                    }
                    if let Some(css) =
                        attr(tag, "style").map(|s| s.to_ascii_lowercase().replace(' ', ""))
                    {
                        if css.contains("font-weight:bold")
                            || css.contains("font-weight:7")
                            || css.contains("font-weight:6")
                        {
                            style.bold = true;
                        }
                        if css.contains("font-style:italic") {
                            style.italic = true;
                        }
                        if css.contains("text-decoration:underline") {
                            style.underline = true;
                        }
                    }
                }
                (n, true) if !n.is_empty() => {
                    if let Some(pos) = stack
                        .iter()
                        .rposition(|(sn, _)| sn.split('+').next() == Some(n))
                    {
                        style = stack[pos].1.clone();
                        stack.truncate(pos);
                    }
                }
                _ => {}
            }
            continue;
        }
        let end = html[i..].find('<').map(|e| i + e).unwrap_or(html.len());
        let raw = &html[i..end];
        i = end;
        if skip.is_some() {
            continue;
        }
        let text = decode(raw);
        if pre > 0 {
            for (k, line) in text.split('\n').enumerate() {
                if k > 0 {
                    flush(
                        &mut blocks,
                        &mut cur,
                        (lists.len() as u8) + quote,
                        quote > 0,
                    );
                }
                if !line.is_empty() {
                    cur.runs.push(Run {
                        text: line.replace('\r', ""),
                        style: style.clone(),
                    });
                }
            }
        } else {
            let mut collapsed = String::with_capacity(text.len());
            let mut last_space = cur.runs.last().is_none_or(|r| r.text.ends_with(' '));
            for c in text.chars() {
                if c.is_whitespace() && c != '\u{a0}' {
                    if !last_space {
                        collapsed.push(' ');
                        last_space = true;
                    }
                } else {
                    collapsed.push(if c == '\u{a0}' { ' ' } else { c });
                    last_space = false;
                }
            }
            if !collapsed.is_empty() {
                match cur.runs.last_mut() {
                    Some(r) if r.style == style => r.text.push_str(&collapsed),
                    _ => cur.runs.push(Run {
                        text: collapsed,
                        style: style.clone(),
                    }),
                }
            }
        }
    }
    flush(&mut blocks, &mut cur, 0, false);
    while blocks.last().is_some_and(|b| b.runs.is_empty() && !b.rule) {
        blocks.pop();
    }
    while blocks.first().is_some_and(|b| b.runs.is_empty() && !b.rule) {
        blocks.remove(0);
    }
    blocks
}

/// Render parsed blocks; returns a clicked link.
pub fn show(
    ui: &mut egui::Ui,
    blocks: &[Block],
    text: Color32,
    weak: Color32,
    link: Color32,
) -> Option<String> {
    let mut clicked = None;
    for b in blocks {
        if b.rule {
            ui.separator();
            continue;
        }
        if b.runs.is_empty() {
            ui.add_space(8.0);
            continue;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
            if b.indent > 0 {
                ui.add_space(18.0 * b.indent as f32);
            }
            if b.quote {
                let top = ui.cursor().top();
                ui.painter().vline(
                    ui.cursor().left() - 8.0,
                    top..=(top + 18.0),
                    egui::Stroke::new(2.0_f32, weak),
                );
            }
            for r in &b.runs {
                let mut rt = RichText::new(&r.text).size(r.style.size).color(if b.quote {
                    weak
                } else {
                    text
                });
                if r.style.bold && !r.style.mono {
                    rt = rt.family(egui::FontFamily::Name("bold".into()));
                }
                if r.style.italic {
                    rt = rt.italics();
                }
                if r.style.underline {
                    rt = rt.underline();
                }
                if r.style.strike {
                    rt = rt.strikethrough();
                }
                if r.style.mono {
                    rt = rt.monospace();
                }
                if let Some(href) = &r.style.link {
                    let resp = ui.add(
                        egui::Label::new(rt.color(link).underline()).sense(egui::Sense::click()),
                    );
                    if resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(href)
                        .clicked()
                    {
                        clicked = Some(href.clone());
                    }
                } else {
                    ui.add(egui::Label::new(rt).selectable(true).wrap());
                }
            }
        });
    }
    clicked
}

// ---------------------------------------------------------------- compose markup

/// Toggle markers at the start of `s`: (marker, kind).
/// The three markers are distinct and none is a prefix of another, so an
/// empty pair (e.g. "~~~~") can never be mistaken for a different style.
pub const BOLD: &str = "**";
pub const ITALIC: &str = "~~";
pub const UNDERLINE: &str = "__";

fn marker_at(s: &str) -> Option<(&'static str, u8)> {
    [(BOLD, 0), (ITALIC, 1), (UNDERLINE, 2)]
        .into_iter()
        .find(|(m, _)| s.starts_with(m))
}

/// Styled layout for the compose editor; markers are shown dimmed.
pub fn markup_job(text: &str, size: f32, color: Color32, weak: Color32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mut st = [false; 3];
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some((m, k)) = marker_at(rest) {
            st[k as usize] = !st[k as usize];
            job.append(
                m,
                0.0,
                TextFormat {
                    font_id: FontId::proportional(size),
                    color: weak,
                    ..Default::default()
                },
            );
            i += m.len();
            continue;
        }
        let ch = rest.chars().next().unwrap();
        let font_id = if st[0] {
            FontId::new(size, egui::FontFamily::Name("bold".into()))
        } else {
            FontId::proportional(size)
        };
        let mut fmt = TextFormat {
            font_id,
            color,
            ..Default::default()
        };
        fmt.italics = st[1];
        if st[2] {
            fmt.underline = egui::Stroke::new(1.0_f32, color);
        }
        job.append(&ch.to_string(), 0.0, fmt);
        i += ch.len_utf8();
    }
    job.wrap.max_width = wrap;
    job
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn inline_html(line: &str) -> String {
    split_urls(line)
        .into_iter()
        .map(|(piece, is_url)| {
            if is_url {
                format!(
                    "<a href=\"{}\">{}</a>",
                    escape(&href(&piece)),
                    escape(&piece)
                )
            } else {
                styled_html(&piece)
            }
        })
        .collect()
}

fn styled_html(line: &str) -> String {
    let mut out = String::new();
    let mut st = [false; 3];
    let tags = [("b", 0), ("i", 1), ("u", 2)];
    let mut i = 0;
    while i < line.len() {
        let rest = &line[i..];
        if let Some((m, k)) = marker_at(rest) {
            let k = k as usize;
            st[k] = !st[k];
            out.push_str(&format!("<{}{}>", if st[k] { "" } else { "/" }, tags[k].0));
            i += m.len();
            continue;
        }
        let ch = rest.chars().next().unwrap();
        out.push_str(&escape(&ch.to_string()));
        i += ch.len_utf8();
    }
    // Close tags left open at the end of the line.
    for k in (0..3).rev() {
        if st[k] {
            out.push_str(&format!("</{}>", tags[k].0));
        }
    }
    out
}

/// Convert the compose markup into HTML for sending.
pub fn markup_to_html(text: &str) -> String {
    let mut out = String::from(
        "<div style=\"font-family:Calibri,Segoe UI,Arial,sans-serif;font-size:11pt\">",
    );
    let mut list: Option<&str> = None;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let (kind, content) = if let Some(c) = trimmed
            .strip_prefix("• ")
            .or_else(|| trimmed.strip_prefix("- "))
        {
            (Some("ul"), c)
        } else if let Some(pos) = trimmed
            .find(". ")
            .filter(|&p| p > 0 && p <= 3 && trimmed[..p].chars().all(|c| c.is_ascii_digit()))
        {
            (Some("ol"), &trimmed[pos + 2..])
        } else {
            (None, line)
        };
        if list != kind {
            if let Some(l) = list {
                out.push_str(&format!("</{l}>"));
            }
            if let Some(k) = kind {
                out.push_str(&format!("<{k}>"));
            }
            list = kind;
        }
        match kind {
            Some(_) => out.push_str(&format!("<li>{}</li>", inline_html(content))),
            None if line.trim().is_empty() => out.push_str("<br>"),
            None => out.push_str(&format!("{}<br>", inline_html(line))),
        }
    }
    if let Some(l) = list {
        out.push_str(&format!("</{l}>"));
    }
    out.push_str("</div>");
    out
}

fn byte_at(text: &str, ch: usize) -> usize {
    text.char_indices()
        .nth(ch)
        .map(|(b, _)| b)
        .unwrap_or(text.len())
}

/// Wrap the selection (char indices) in `marker`, or remove the markers if
/// it is already wrapped. With an empty selection an empty pair is inserted.
/// Returns the new selection.
pub fn toggle_wrap(text: &mut String, sel: (usize, usize), marker: &str) -> (usize, usize) {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    let (ba, bb) = (byte_at(text, a), byte_at(text, b));
    let m = marker.chars().count();
    let before = &text[..ba];
    let after = &text[bb..];
    // Cursor right before a closing marker: pressing the shortcut again ends
    // the formatting, like in Outlook – just step over the marker.
    if a == b && after.starts_with(marker) && before.matches(marker).count() % 2 == 1 {
        return (a + m, a + m);
    }
    if a != b && before.ends_with(marker) && after.starts_with(marker) {
        text.replace_range(bb..bb + marker.len(), "");
        text.replace_range(ba - marker.len()..ba, "");
        return (a - m, b - m);
    }
    text.insert_str(bb, marker);
    text.insert_str(ba, marker);
    (a + m, b + m)
}

/// Turn the lines touched by the selection into a bulleted or numbered
/// list, or back into plain lines.
pub fn toggle_list(text: &mut String, sel: (usize, usize), numbered: bool) {
    let (a, b) = (sel.0.min(sel.1), sel.0.max(sel.1));
    let (ba, bb) = (byte_at(text, a), byte_at(text, b));
    let start = text[..ba].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = text[bb..].find('\n').map(|i| bb + i).unwrap_or(text.len());
    let lines: Vec<&str> = text[start..end].split('\n').collect();
    let is_marked = |l: &str| {
        if numbered {
            l.find(". ")
                .is_some_and(|p| p > 0 && p <= 3 && l[..p].chars().all(|c| c.is_ascii_digit()))
        } else {
            l.starts_with("• ")
        }
    };
    let all = lines.iter().all(|l| is_marked(l));
    let out: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            if all {
                if numbered {
                    l[l.find(". ").unwrap() + 2..].to_string()
                } else {
                    l["• ".len()..].to_string()
                }
            } else if numbered {
                format!("{}. {l}", i + 1)
            } else {
                format!("• {l}")
            }
        })
        .collect();
    text.replace_range(start..end, &out.join("\n"));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(b: &Block) -> String {
        b.runs.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn parses_inline_styles_and_links() {
        let blocks =
            parse("<p>Hello <b>bold</b> and <a href=\"https://x.de\">Link</a> &amp; more</p>");
        assert_eq!(blocks.len(), 1);
        assert_eq!(text_of(&blocks[0]), "Hello bold and Link & more");
        assert!(blocks[0]
            .runs
            .iter()
            .any(|r| r.style.bold && r.text == "bold"));
        assert!(blocks[0]
            .runs
            .iter()
            .any(|r| r.style.link.as_deref() == Some("https://x.de")));
    }

    #[test]
    fn parses_lists_headings_and_skips_style() {
        let blocks = parse("<style>p{color:red}</style><h1>Title</h1><ol><li>one</li><li>two</li></ol><ul><li>a</li></ul>");
        let texts: Vec<String> = blocks
            .iter()
            .filter(|b| !b.runs.is_empty())
            .map(text_of)
            .collect();
        assert_eq!(texts, vec!["Title", "1. one", "2. two", "•  a"]);
        assert!(blocks[0].runs[0].style.bold && blocks[0].runs[0].style.size > 20.0);
    }

    #[test]
    fn keeps_line_breaks_of_plain_text_mails() {
        let blocks = parse("<div style=\"white-space:pre-wrap\">Line 1\nLine 2</div>");
        let texts: Vec<String> = blocks
            .iter()
            .filter(|b| !b.runs.is_empty())
            .map(text_of)
            .collect();
        assert_eq!(texts, vec!["Line 1", "Line 2"]);
    }

    #[test]
    fn links_plain_urls() {
        assert_eq!(
            split_urls("See https://x.de/a?b=1. Or www.y.de, mail: mailto:a@b.de"),
            vec![
                ("See ".to_string(), false),
                ("https://x.de/a?b=1".to_string(), true),
                (". Or ".to_string(), false),
                ("www.y.de".to_string(), true),
                (", mail: ".to_string(), false),
                ("mailto:a@b.de".to_string(), true),
            ]
        );
        let blocks =
            parse("<div style=\"white-space:pre-wrap\">Agenda: https://contoso.com/agenda</div>");
        assert_eq!(
            blocks[0].runs[1].style.link.as_deref(),
            Some("https://contoso.com/agenda")
        );
        assert!(markup_to_html("Link www.contoso.com")
            .contains("<a href=\"https://www.contoso.com\">www.contoso.com</a>"));
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(
            decode("Gr&uuml;&szlig;e &#8364; &#x41; &unknown;"),
            "Grüße € A &unknown;"
        );
    }

    #[test]
    fn wraps_and_unwraps_selection() {
        let mut t = "Hello World".to_string();
        let sel = toggle_wrap(&mut t, (6, 11), "**");
        assert_eq!(t, "Hello **World**");
        assert_eq!(sel, (8, 13));
        toggle_wrap(&mut t, sel, "**");
        assert_eq!(t, "Hello World");
        let mut u = "naïve".to_string();
        toggle_wrap(&mut u, (0, 5), ITALIC);
        assert_eq!(u, "~~naïve~~");
        // Typing with shortcuts: on, text, off, text.
        let mut t = "This is ".to_string();
        let sel = toggle_wrap(&mut t, (8, 8), BOLD);
        assert_eq!(sel, (10, 10));
        t.insert_str(10, "crucial");
        let sel = toggle_wrap(&mut t, (17, 17), BOLD);
        assert_eq!(sel, (19, 19));
        t.push_str(" and ");
        let sel = toggle_wrap(&mut t, (24, 24), ITALIC);
        t.insert_str(sel.0, "italic");
        assert_eq!(t, "This is **crucial** and ~~italic~~");
        assert!(markup_to_html(&t).contains("This is <b>crucial</b> and <i>italic</i>"));
    }

    #[test]
    fn toggles_lists() {
        let mut t = "a\nb\nc".to_string();
        toggle_list(&mut t, (0, 3), false);
        assert_eq!(t, "• a\n• b\nc");
        toggle_list(&mut t, (0, 5), false);
        assert_eq!(t, "a\nb\nc");
        toggle_list(&mut t, (4, 5), true);
        assert_eq!(t, "a\nb\n1. c");
    }

    #[test]
    fn markup_to_html_formats() {
        let html = markup_to_html("Hello **World** and ~~you~~\n• one\n• two\n1. a\n<b>");
        assert!(html.contains("Hello <b>World</b> and <i>you</i><br>"));
        assert!(html.contains("<ul><li>one</li><li>two</li></ul>"));
        assert!(html.contains("<ol><li>a</li></ol>"));
        assert!(html.contains("&lt;b&gt;"));
    }
}
