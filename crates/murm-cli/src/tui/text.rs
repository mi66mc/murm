use unicode_width::UnicodeWidthChar;
pub fn sanitize(text: &str) -> String {
    let mut output = String::new();
    for c in text.chars() {
        match c {
            '\n' => output.push('\n'),
            '\t' => output.push_str("  "),
            c if c.is_control() => output.push_str(&format!("\\u{{{:04x}}}", c as u32)),
            c => output.push(c),
        }
    }
    output
}
/// Render only visible display rows, without byte slicing or terminal controls.
pub fn viewport(text: &str, scroll: usize, width: usize, height: usize) -> String {
    if width == 0 || height == 0 {
        return String::new();
    }
    let mut lines = vec![String::new(); height];
    let (mut row, mut column) = (0usize, 0usize);
    for c in text.chars() {
        let fragment = if c == '\t' {
            "  ".into()
        } else if c != '\n' && c.is_control() {
            format!("\\u{{{:04x}}}", c as u32)
        } else {
            c.to_string()
        };
        for c in fragment.chars() {
            if c == '\n' {
                row = row.saturating_add(1);
                column = 0;
            } else {
                let n = c.width().unwrap_or(0);
                if column.saturating_add(n) > width {
                    row = row.saturating_add(1);
                    column = 0;
                }
                if row >= scroll && row < scroll.saturating_add(height) {
                    lines[row - scroll].push(c);
                }
                column = column.saturating_add(n);
            }
            if row >= scroll.saturating_add(height) {
                return lines.join("\n");
            }
        }
    }
    lines.join("\n")
}
