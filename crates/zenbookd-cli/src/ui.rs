use std::{env, io::stdout};

use colored::{ColoredString, Colorize};
use terminal_size::{Width, terminal_size_of};

const GAP: usize = 2;
const PAD: usize = 3;
const FALLBACK_COLUMNS: usize = 80;

pub const GAUGE_CELLS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tone {
    Plain,
    Dim,
    Accent,
    Ok,
    Warn,
    Danger,
}

impl Tone {
    fn paint(self, text: &str) -> ColoredString {
        match self {
            Tone::Plain => text.normal(),
            Tone::Dim => text.dimmed(),
            Tone::Accent => text.cyan(),
            Tone::Ok => text.green(),
            Tone::Warn => text.yellow(),
            Tone::Danger => text.red(),
        }
    }
}

struct Glyphs {
    top_left: char,
    top_right: char,
    bottom_left: char,
    bottom_right: char,
    horizontal: char,
    vertical: char,
    tee_left: char,
    tee_right: char,
    gauge_full: char,
    gauge_empty: char,
}

const UNICODE: Glyphs = Glyphs {
    top_left: '╭',
    top_right: '╮',
    bottom_left: '╰',
    bottom_right: '╯',
    horizontal: '─',
    vertical: '│',
    tee_left: '├',
    tee_right: '┤',
    gauge_full: '█',
    gauge_empty: '░',
};

const ASCII: Glyphs = Glyphs {
    top_left: '+',
    top_right: '+',
    bottom_left: '+',
    bottom_right: '+',
    horizontal: '-',
    vertical: '|',
    tee_left: '+',
    tee_right: '+',
    gauge_full: '#',
    gauge_empty: '.',
};

fn glyphs() -> &'static Glyphs {
    let utf8 = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.is_empty()))
        .is_some_and(|value| value.to_ascii_lowercase().contains("utf"));

    if utf8 { &UNICODE } else { &ASCII }
}

fn columns() -> usize {
    terminal_size_of(stdout())
        .map(|(Width(width), _)| width as usize)
        .unwrap_or(FALLBACK_COLUMNS)
}

pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone)]
struct Span {
    text: String,
    tone: Tone,
}

#[derive(Debug, Clone, Default)]
pub struct Line {
    spans: Vec<Span>,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(mut self, text: impl Into<String>, tone: Tone) -> Self {
        self.spans.push(Span {
            text: text.into(),
            tone,
        });

        self
    }

    pub fn plain(self, text: impl Into<String>) -> Self {
        self.push(text, Tone::Plain)
    }

    fn concat(mut self, other: Line) -> Self {
        self.spans.extend(other.spans);
        self
    }

    pub fn width(&self) -> usize {
        self.spans
            .iter()
            .map(|span| span.text.chars().count())
            .sum()
    }

    pub fn render(&self) -> String {
        self.spans
            .iter()
            .map(|span| span.tone.paint(&span.text).to_string())
            .collect()
    }
}

pub struct Row {
    label: String,
    value: Line,
}

pub fn field(label: impl Into<String>, value: Line) -> Row {
    Row {
        label: label.into(),
        value,
    }
}

struct Section {
    rows: Vec<Row>,
    label_width: usize,
}

pub struct Panel {
    title: String,
    sections: Vec<Section>,
}

impl Panel {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            sections: Vec::new(),
        }
    }

    pub fn section(mut self, rows: Vec<Row>) -> Self {
        if rows.is_empty() {
            return self;
        }

        let label_width = rows
            .iter()
            .map(|row| row.label.chars().count())
            .max()
            .unwrap_or(0);

        self.sections.push(Section { rows, label_width });

        self
    }

    fn natural_width(&self) -> usize {
        self.sections
            .iter()
            .flat_map(|section| {
                section
                    .rows
                    .iter()
                    .map(|row| section.label_width + GAP + row.value.width())
            })
            .max()
            .unwrap_or(0)
    }

    pub fn render(&self) -> String {
        self.render_within(columns())
    }

    fn render_within(&self, width: usize) -> String {
        if self.sections.is_empty() {
            return String::new();
        }

        let content = self.natural_width().max(self.title.chars().count() + 3);

        if content + 2 * PAD + 2 > width {
            return self.render_bare();
        }

        self.render_boxed(content)
    }

    fn render_bare(&self) -> String {
        let mut out = String::new();

        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }

            for row in &section.rows {
                out.push_str(&body(row, section.label_width).render());
                out.push('\n');
            }
        }

        out
    }

    fn render_boxed(&self, content: usize) -> String {
        let g = glyphs();
        let inner = content + 2 * PAD;

        let mut out = String::new();

        out.push_str(&self.heading(inner));
        out.push('\n');

        for (index, section) in self.sections.iter().enumerate() {
            if index > 0 {
                out.push_str(&rule(g.tee_left, g.tee_right, inner));
                out.push('\n');
            }

            out.push_str(&blank(inner));
            out.push('\n');

            for row in &section.rows {
                let content = body(row, section.label_width);
                let trailing = inner - PAD - content.width();

                let line = Line::new()
                    .push(g.vertical.to_string(), Tone::Dim)
                    .plain(" ".repeat(PAD))
                    .concat(content)
                    .plain(" ".repeat(trailing))
                    .push(g.vertical.to_string(), Tone::Dim);

                out.push_str(&line.render());
                out.push('\n');
            }

            out.push_str(&blank(inner));
            out.push('\n');
        }

        out.push_str(&rule(g.bottom_left, g.bottom_right, inner));
        out.push('\n');

        out
    }

    fn heading(&self, inner: usize) -> String {
        let g = glyphs();
        let used = self.title.chars().count() + 3;

        Line::new()
            .push(g.top_left.to_string(), Tone::Dim)
            .push(g.horizontal.to_string(), Tone::Dim)
            .push(format!(" {} ", self.title), Tone::Accent)
            .push(g.horizontal.to_string().repeat(inner - used), Tone::Dim)
            .push(g.top_right.to_string(), Tone::Dim)
            .render()
    }

    pub fn print(&self) {
        print!("{}", self.render());
    }
}

fn body(row: &Row, label_width: usize) -> Line {
    let padding = " ".repeat(label_width - row.label.chars().count());

    Line::new()
        .plain(format!("{}{padding}", row.label))
        .plain(" ".repeat(GAP))
        .concat(row.value.clone())
}

fn rule(left: char, right: char, inner: usize) -> String {
    let g = glyphs();

    Line::new()
        .push(left.to_string(), Tone::Dim)
        .push(g.horizontal.to_string().repeat(inner), Tone::Dim)
        .push(right.to_string(), Tone::Dim)
        .render()
}

fn blank(inner: usize) -> String {
    let vertical = glyphs().vertical.to_string();

    Line::new()
        .push(vertical.clone(), Tone::Dim)
        .plain(" ".repeat(inner))
        .push(vertical, Tone::Dim)
        .render()
}

pub fn gauge(percent: u32, cells: usize, tone: Tone) -> Line {
    let percent = percent.min(100) as usize;

    let filled = if percent == 0 {
        0
    } else {
        ((percent * cells + 50) / 100).clamp(1, cells)
    };

    let g = glyphs();

    Line::new()
        .push(g.gauge_full.to_string().repeat(filled), tone)
        .push(g.gauge_empty.to_string().repeat(cells - filled), Tone::Dim)
}

pub fn success(message: &str) {
    println!("{} {}", Tone::Ok.paint("✔"), message);
}

pub fn failure(headline: &str, cause: &str, hint: Option<&str>) {
    report(Tone::Danger, "✘", headline, cause, hint);
}

pub fn notice(headline: &str, cause: &str, hint: Option<&str>) {
    report(Tone::Warn, "!", headline, cause, hint);
}

fn report(tone: Tone, glyph: &str, headline: &str, cause: &str, hint: Option<&str>) {
    eprintln!("{} {}", tone.paint(glyph), tone.paint(headline).bold());

    if !cause.is_empty() {
        eprintln!("  {}", sanitize(cause));
    }

    if let Some(hint) = hint {
        eprintln!("  {}", Tone::Dim.paint(hint));
    }
}
