use crate::{ClientError, Draft};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier, Style};
use ratatui_textarea::{CursorMove, TextArea, WrapMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorFocus {
    Title,
    Body,
}
#[derive(Debug, Clone, Copy)]
pub enum EditorLeave {
    Back,
    Quit,
}
pub struct DraftEditor {
    pub draft: Draft,
    pub title: TextArea<'static>,
    pub body: TextArea<'static>,
    pub focus: EditorFocus,
    pub dirty: bool,
}
fn valid_source(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\t' | '\r'))
}
fn textarea(text: &str) -> TextArea<'static> {
    let mut area = TextArea::new(text.split('\n').map(str::to_owned).collect());
    area.set_wrap_mode(WrapMode::WordOrGlyph);
    area.set_max_histories(50);
    area.set_style(Style::default().fg(Color::Rgb(220, 226, 237)));
    area.set_cursor_style(
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    );
    area.set_cursor_line_style(Style::default());
    area.set_selection_style(Style::default().bg(Color::Rgb(43, 68, 91)));
    area
}
/// Size of the selected range, including line separators; textarea columns are Unicode scalar indices.
fn selected_size(area: &TextArea<'_>) -> (usize, usize) {
    let Some(((start_row, start_col), (end_row, end_col))) = area.selection_range() else {
        return (0, 0);
    };
    let mut bytes = end_row.saturating_sub(start_row);
    let mut chars = bytes;
    for row in start_row..=end_row {
        let line = &area.lines()[row];
        let start = if row == start_row { start_col } else { 0 };
        let end = if row == end_row {
            end_col
        } else {
            line.chars().count()
        };
        for c in line.chars().skip(start).take(end.saturating_sub(start)) {
            bytes += c.len_utf8();
            chars += 1;
        }
    }
    (bytes, chars)
}
impl DraftEditor {
    pub fn new(draft: Draft) -> Result<Self, ClientError> {
        let source = draft.content.as_str().ok_or(ClientError::InvalidInput)?;
        let title = draft
            .header
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !valid_source(source) || !valid_source(title) {
            return Err(ClientError::InvalidInput);
        }
        let title = textarea(title);
        let body = textarea(source);
        Ok(Self {
            draft,
            title,
            body,
            focus: EditorFocus::Body,
            dirty: false,
        })
    }
    pub fn source(&self) -> String {
        self.body.lines().join("\n")
    }
    pub fn title_text(&self) -> String {
        self.title.lines().join(" ")
    }
    fn active(&mut self) -> &mut TextArea<'static> {
        if self.focus == EditorFocus::Title {
            &mut self.title
        } else {
            &mut self.body
        }
    }
    pub fn input(&mut self, key: KeyEvent) {
        if matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.focus = if self.focus == EditorFocus::Body {
                EditorFocus::Title
            } else {
                EditorFocus::Body
            };
            return;
        }
        if self.focus == EditorFocus::Title && key.code == KeyCode::Enter {
            self.focus = EditorFocus::Body;
            return;
        }
        let changed = if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('z') => self.active().undo(),
                KeyCode::Char('y') => self.active().redo(),
                KeyCode::Char('a') => {
                    // The widget's select_all uses a u16 jump, which truncates long articles.
                    let area = self.active();
                    area.cancel_selection();
                    area.move_cursor(CursorMove::Top);
                    area.move_cursor(CursorMove::Head);
                    area.start_selection();
                    area.move_cursor(CursorMove::Bottom);
                    area.move_cursor(CursorMove::End);
                    false
                }
                _ => self.active().input(key),
            }
        } else {
            self.active().input(key)
        };
        self.dirty |= changed;
    }
    pub fn paste(&mut self, text: &str) -> Result<(), ClientError> {
        if !valid_source(text) {
            return Err(ClientError::InvalidInput);
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if self.focus == EditorFocus::Title {
            let title = text.replace(['\n', '\t'], " ");
            let (_, selected) = selected_size(&self.title);
            if self.title_text().chars().count().saturating_sub(selected) + title.chars().count()
                > 200
            {
                return Err(ClientError::InvalidInput);
            }
            self.dirty |= self.title.insert_str(title);
        } else {
            let bytes = self.body.lines().iter().map(String::len).sum::<usize>()
                + self.body.lines().len().saturating_sub(1);
            let (selected, _) = selected_size(&self.body);
            if bytes.saturating_sub(selected).saturating_add(text.len()) > 4_194_304 {
                return Err(ClientError::InvalidInput);
            }
            self.dirty |= self.body.insert_str(text);
        }
        Ok(())
    }
    pub fn draft_for_save(&self) -> Result<Draft, ClientError> {
        let title = self.title_text();
        let source = self.source();
        if title.chars().count() > 200 || source.len() > 4_194_304 {
            return Err(ClientError::InvalidInput);
        }
        let mut draft = self.draft.clone();
        draft
            .header
            .insert("title".into(), serde_json::json!(title));
        draft.content = serde_json::json!(source);
        Ok(draft)
    }
    pub fn saved(&mut self, draft: Draft) {
        self.draft = draft;
        self.dirty = false;
    }
}
