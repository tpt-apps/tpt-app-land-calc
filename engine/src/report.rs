//! Pro edition: minimal single-purpose PDF report writer (PDF 1.4,
//! Helvetica, A4 portrait). No external crates — just a text-stream emitter
//! with a correct xref table. Same approach as the FEA Lite Pro report:
//! paginated styled lines plus ruled tables.

/// One line in the report body.
pub enum Row {
    /// Document title (18 pt bold).
    Title(String),
    /// Section heading (13 pt bold).
    Head(String),
    /// Body text (10 pt regular).
    Body(String),
    /// Muted note (9 pt regular, gray).
    Note(String),
    /// Vertical gap (points).
    Gap(f64),
    /// A table: header row + data rows, equal column widths across the
    /// text width, light rules between rows.
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        /// Relative column widths (must sum to ~1.0; default equal).
        weights: Option<Vec<f64>>,
    },
}

/// Renders the rows into a PDF document (bytes).
pub fn render_report_pdf(_title: &str, rows: &[Row]) -> Vec<u8> {
    const PAGE_W: f64 = 595.28; // A4 portrait, points
    const PAGE_H: f64 = 841.89;
    const MARGIN: f64 = 56.0;
    const TEXT_W: f64 = PAGE_W - 2.0 * MARGIN;
    const LINE: f64 = 15.0;

    // ---- layout: break rows into pages of positioned lines ----
    enum Op {
        Text { x: f64, y: f64, size: f64, bold: bool, gray: bool, text: String },
        Rule { y: f64 },
    }
    let mut pages: Vec<Vec<Op>> = vec![Vec::new()];
    let mut y = PAGE_H - MARGIN;

    fn push_text(page: &mut Vec<Op>, y: &mut f64, x: f64, size: f64, bold: bool, gray: bool, text: &str) {
        page.push(Op::Text { x, y: *y, size, bold, gray, text: text.to_string() });
        *y -= size * 1.4;
    }

    for row in rows {
        match row {
            Row::Title(text) => {
                y -= 8.0;
                let page = pages.last_mut().unwrap();
                push_text(page, &mut y, MARGIN, 18.0, true, false, text);
                y -= 4.0;
            }
            Row::Head(text) => {
                y -= 6.0;
                let page = pages.last_mut().unwrap();
                push_text(page, &mut y, MARGIN, 13.0, true, false, text);
            }
            Row::Body(text) => {
                let page = pages.last_mut().unwrap();
                push_text(page, &mut y, MARGIN, 10.0, false, false, text);
            }
            Row::Note(text) => {
                let page = pages.last_mut().unwrap();
                push_text(page, &mut y, MARGIN, 9.0, false, true, text);
            }
            Row::Gap(points) => y -= points,
            Row::Table { columns, rows: data, weights } => {
                if data.is_empty() {
                    continue;
                }
                let n = columns.len().max(1);
                let total_w: f64 = weights.as_ref().map(|w| w.iter().sum()).unwrap_or(n as f64);
                let col_w: Vec<f64> = (0..n)
                    .map(|i| {
                        let w = weights
                            .as_ref()
                            .and_then(|ws| ws.get(i).copied())
                            .unwrap_or(1.0);
                        TEXT_W * w / total_w
                    })
                    .collect();
                // Page-break the table body when it would run off the page.
                for (r, cells) in std::iter::once((0usize, columns)).chain(
                    data.iter().enumerate().map(|(i, row)| (i + 1, row)),
                ) {
                    if y < MARGIN + 3.0 * LINE {
                        pages.push(Vec::new());
                        y = PAGE_H - MARGIN;
                    }
                    let page = pages.last_mut().unwrap();
                    let mut x = MARGIN;
                    for (c, cell) in cells.iter().enumerate() {
                        let w = col_w.get(c).copied().unwrap_or(TEXT_W);
                        let size = if r == 0 { 10.0 } else { 9.5 };
                        page.push(Op::Text {
                            x,
                            y,
                            size,
                            bold: r == 0,
                            gray: false,
                            text: clip_text(cell, w - 6.0, size),
                        });
                        x += w;
                    }
                    // Light rule under every row (heavier under the header).
                    page.push(Op::Rule { y: y - 4.0 });
                    y -= LINE;
                }
                y -= 6.0;
            }
        }
        if y < MARGIN + LINE {
            pages.push(Vec::new());
            y = PAGE_H - MARGIN;
        }
    }

    // ---- emit PDF ----
    let mut out: Vec<u8> = Vec::new();
    out.extend_from_slice(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n");
    let mut offsets: Vec<usize> = Vec::new();

    let obj = |out: &mut Vec<u8>, offsets: &mut Vec<usize>, body: &str| {
        offsets.push(out.len());
        out.extend_from_slice(body.as_bytes());
    };

    // 1: catalog, 2: pages, then per page: content stream obj + page obj.
    let page_count = pages.len();
    obj(&mut out, &mut offsets, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    obj(&mut out, &mut offsets, &format!("2 0 obj\n<< /Type /Pages /Kids [{}] /Count {page_count} >>\nendobj\n",
        (0..page_count).map(|p| format!("{} 0 R", 3 + p * 2 + 1)).collect::<Vec<_>>().join(" ")));

    let mut fonts = "|F1 Helvetica|F2 Helvetica-Bold".to_string();
    // Font objects (3-page-slot-relative so keep them first): F1 = obj
    // (3 + 2*page_count), F2 = obj (4 + 2*page_count).
    let font1_no = 3 + page_count * 2;
    let font2_no = font1_no + 1;

    for (pi, ops) in pages.iter().enumerate() {
        let mut stream = String::new();
        for op in ops {
            match op {
                Op::Text { x, y, size, bold, gray, text } => {
                    let font = if *bold { "F2" } else { "F1" };
                    let color = if *gray { "0.45 g" } else { "0 g" };
                    stream.push_str(&format!(
                        "BT {color} /{font} {size} Tf 1 0 0 1 {x:.2} {y:.2} Tm ({}) Tj ET\n",
                        pdf_escape(text)
                    ));
                }
                Op::Rule { y } => {
                    stream.push_str(&format!(
                        "0.85 G 0.6 w {MARGIN:.2} {y:.2} m {:.2} {y:.2} l S\n",
                        PAGE_W - MARGIN
                    ));
                }
            }
        }
        let content_no = 3 + pi * 2;
        obj(&mut out, &mut offsets, &format!(
            "{content_no} 0 obj\n<< /Length {} >>\nstream\n{}\nendstream\nendobj\n",
            stream.len(), stream));
        obj(&mut out, &mut offsets, &format!(
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PAGE_W} {PAGE_H}] /Resources << /Font << /F1 {font1_no} 0 R /F2 {font2_no} 0 R >> >> /Contents {content_no} 0 R >>\nendobj\n",
            content_no + 1));
    }
    let _ = &mut fonts;

    obj(&mut out, &mut offsets, &format!(
        "{font1_no} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n"));
    obj(&mut out, &mut offsets, &format!(
        "{font2_no} 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>\nendobj\n"));

    let xref_pos = out.len();
    let obj_count = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {obj_count}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {obj_count} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

/// Escapes text for a PDF literal string (WinAnsi-safe subset; non-latin1
/// characters are transliterated out).
fn pdf_escape(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '(' => "\\(".to_string(),
            ')' => "\\)".to_string(),
            '\\' => "\\\\".to_string(),
            c if (c as u32) < 256 => c.to_string(),
            _ => "?".to_string(),
        })
        .collect()
}

/// Rough width clip so table cells don't overlap their neighbor: ~0.5·size
/// per char for Helvetica digits/lowercase mix.
fn clip_text(text: &str, max_w: f64, size: f64) -> String {
    let per_char = size * 0.52;
    let max_chars = ((max_w / per_char).floor() as usize).max(4);
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{cut}\u{2026}")
}
