use super::*;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
const BG: Color = Color::Rgb(15, 20, 29);
const PANEL: Color = Color::Rgb(22, 29, 40);
const TEXT: Color = Color::Rgb(220, 226, 237);
const MUTED: Color = Color::Rgb(142, 158, 180);
const ACCENT: Color = Color::Rgb(103, 218, 202);
fn block(title: impl Into<String>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Rgb(54, 70, 91)))
        .title(Line::from(title.into()).style(Style::default().fg(ACCENT).bold()))
}
fn short(value: impl ToString) -> String {
    let value = value.to_string();
    if value.chars().count() > 18 {
        format!("{}…", value.chars().take(16).collect::<String>())
    } else {
        value
    }
}
fn title(screen: Screen) -> &'static str {
    match screen {
        Screen::Profiles => "Profiles",
        Screen::Articles => "Articles",
        Screen::Drafts => "Drafts",
        Screen::Relays => "Relay settings",
        Screen::Outbox => "Outbox",
        Screen::History => "Revision history",
        Screen::Article => "Reader",
        Screen::Editor => "Markdown editor",
        Screen::Confirm => "Confirm action",
        Screen::ProfileForm | Screen::Unlock => "Profile settings",
    }
}
fn hints(state: &AppState) -> &'static str {
    if state.editor_leave.is_some() {
        return "Y Discard changes   N / Esc Keep editing";
    }
    if state.screen == Screen::Editor {
        return "Ctrl+S Save   Tab Title / Body   Ctrl+Z Undo   Ctrl+Y Redo   Esc Back";
    }
    if state.form.is_some() {
        return "Tab Next field   Shift+Tab Previous   Enter Confirm   Esc Cancel";
    }
    if state.screen == Screen::Confirm {
        return "Enter Confirm   Esc Cancel";
    }
    match state.screen {
        Screen::Profiles => {
            "N New   I Import private   O Import public   Enter Select   U Unlock   L Lock"
        }
        Screen::Articles => "Enter Read   E Revise   H History   W Withdraw   S Sync",
        Screen::Drafts => "N New draft   E Edit here   Enter Read   P Publish saved draft",
        Screen::Relays => "N Add relay   Enter Info   S Sync selected   D Remove",
        Screen::Outbox => "Enter Details   R Retry pending   F5 Refresh   Stored is not delivered",
        Screen::History => "Enter Read exact revision   Esc Back",
        _ => "Up / Down Scroll   PgUp / PgDn Page   Esc Back",
    }
}
fn entry(title: String, subtitle: String) -> ListItem<'static> {
    ListItem::new(Text::from(vec![
        Line::from(sanitize(&title).replace('\n', " ")).style(Style::default().fg(TEXT).bold()),
        Line::from(sanitize(&subtitle).replace('\n', " ")).style(Style::default().fg(MUTED)),
        Line::from(""),
    ]))
}
fn entries(state: &AppState) -> Vec<ListItem<'static>> {
    match state.screen {
        Screen::Profiles => state
            .profiles
            .iter()
            .map(|p| {
                entry(
                    p.name.clone(),
                    format!(
                        "{}  ·  {}",
                        short(p.public_key),
                        if p.mode == crate::ProfileMode::ReadOnly {
                            "read only"
                        } else {
                            "signing identity"
                        }
                    ),
                )
            })
            .collect(),
        Screen::Articles | Screen::History => {
            let source = if state.screen == Screen::History {
                &state.history
            } else {
                &state.articles
            };
            source
                .iter()
                .map(|a| {
                    entry(
                        if a.title.is_empty() {
                            "Untitled article".into()
                        } else {
                            a.title.clone()
                        },
                        super::views::articles::label(a),
                    )
                })
                .collect()
        }
        Screen::Drafts => state
            .drafts
            .iter()
            .map(|d| {
                entry(
                    if d.title.is_empty() {
                        "Untitled draft".into()
                    } else {
                        d.title.clone()
                    },
                    super::views::drafts::label(d),
                )
            })
            .collect(),
        Screen::Relays => state
            .active
            .as_ref()
            .map(|p| {
                p.relays
                    .iter()
                    .map(|url| {
                        entry(
                            super::views::relays::label(url),
                            "Configured contact · Enter to inspect policies".into(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        Screen::Outbox => state
            .outbox
            .iter()
            .map(|e| {
                entry(
                    short(e.event_id),
                    format!(
                        "{} stored  ·  {} pending  ·  {} refused  ·  {} destinations",
                        e.destinations
                            .iter()
                            .filter(|d| d.state == crate::DestinationState::Stored)
                            .count(),
                        e.destinations
                            .iter()
                            .filter(|d| matches!(
                                d.state,
                                crate::DestinationState::Pending | crate::DestinationState::RetryAt
                            ))
                            .count(),
                        e.destinations
                            .iter()
                            .filter(|d| d.state == crate::DestinationState::Refused)
                            .count(),
                        e.destinations.len()
                    ),
                )
            })
            .collect(),
        _ => vec![],
    }
}
fn metadata(state: &AppState) -> String {
    let i = state.selected;
    match state.screen {
        Screen::Profiles=>state.profiles.get(i).map(|p|format!("{}\n\nPUBLIC KEY\n{}\n\nMODE\n{:?}\n\nRELAYS\n{}\n\nThe name is local. The public key is the identity.",p.name,p.public_key,p.mode,p.relays.join("\n"))),
        Screen::Articles|Screen::History=>{
            let source=if state.screen==Screen::History {&state.history}else{&state.articles};
            source.get(i).map(|a|format!("{}\n\nREVISION {} · {:?}\n\nSIGNER\n{}\n\nPUBLICATION\n{}\n\nEVENT\n{}\n\nFull source opens with Enter.",a.title,a.revision,a.status,a.signer,a.publication,a.id))
        }
        Screen::Drafts=>state.drafts.get(i).map(|d|format!("{}\n\nREVISION {}\n\nPUBLICATION\n{}\n\nDRAFT\n{}\n\nE opens the Markdown editor.\nCtrl+S saves locally.\nP publishes the saved version.",d.title,d.revision.map(|v|v.to_string()).unwrap_or_default(),d.publication,d.id)),
        Screen::Outbox=>state.outbox.get(i).map(|e|format!("EVENT\n{}\n\n{}\n\nStored means this relay accepted a copy, not delivery or permanent availability.",e.event_id,e.destinations.iter().map(destination_label).collect::<Vec<_>>().join("\n\n"))),
        Screen::Relays=>state.active.as_ref().and_then(|p|p.relays.get(i)).map(|u|format!("{}\n\nEnter inspects /info policies.\nS synchronizes retained events.\nD removes this local contact.\n\nA relay is not an account or a trusted source of truth.",u)),
        _=>None,
    }.unwrap_or_else(||"Select an item to inspect its details.".into())
}
fn empty(state: &AppState) -> (&'static str, &'static str) {
    match state.screen {
        Screen::Profiles => (
            "No profiles yet",
            "Press N to create a test identity, or O to import a public key.",
        ),
        Screen::Articles => (
            "No articles yet",
            "Add a relay and press S to sync, or create a draft with 3 then N.",
        ),
        Screen::Drafts => (
            "No drafts yet",
            "Press N to start writing. Drafts are local until you publish.",
        ),
        Screen::Relays => (
            "No relays yet",
            "Press N to add a relay URL. Local testing: http://127.0.0.1:8080",
        ),
        Screen::Outbox => (
            "Nothing queued",
            "Published events and their storage status will appear here.",
        ),
        _ => (
            "No retained revisions",
            "History includes only revisions available in the local cache.",
        ),
    }
}
fn list_view(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let items = entries(state);
    let count = items.len();
    let panels = if area.width >= 86 && count > 0 {
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).split(area)
    } else {
        Layout::horizontal([Constraint::Percentage(100)]).split(area)
    };
    let list_block = block(format!(" {}  {} ", title(state.screen), count));
    if count == 0 {
        let inner = list_block.inner(panels[0]);
        frame.render_widget(list_block, panels[0]);
        let (heading, instruction) = empty(state);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(heading).style(Style::default().fg(TEXT).bold()),
                Line::from(""),
                Line::from(instruction).style(Style::default().fg(MUTED)),
            ])
            .wrap(Wrap { trim: false }),
            inner,
        );
    } else {
        let selected = state.selected.min(count.saturating_sub(1));
        let mut selection = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(
            List::new(items)
                .block(list_block)
                .highlight_style(Style::default().bg(Color::Rgb(34, 58, 74)))
                .highlight_symbol("▎ ")
                .repeat_highlight_symbol(true),
            panels[0],
            &mut selection,
        );
    }
    if panels.len() > 1 {
        frame.render_widget(
            Paragraph::new(sanitize(&metadata(state)))
                .style(Style::default().fg(MUTED))
                .wrap(Wrap { trim: false })
                .block(block(" Details ")),
            panels[1],
        );
    }
}
fn editor_view(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let Some(editor) = &state.editor else {
        return;
    };
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);
    for (rect, label, focus, widget) in [
        (rows[0], " Title ", EditorFocus::Title, &editor.title),
        (
            rows[1],
            " Markdown source ",
            EditorFocus::Body,
            &editor.body,
        ),
    ] {
        let panel = block(label).border_style(Style::default().fg(if editor.focus == focus {
            ACCENT
        } else {
            Color::Rgb(54, 70, 91)
        }));
        let inner = panel.inner(rect);
        frame.render_widget(panel, rect);
        frame.render_widget(widget, inner);
    }
    let cursor = if editor.focus == EditorFocus::Body {
        editor.body.cursor()
    } else {
        editor.title.cursor()
    };
    frame.render_widget(
        Paragraph::new(format!(
            " {}  ·  Ln {}, Col {}  ·  {} lines  ·  Save does not publish",
            if state.busy {
                "Saving…"
            } else if editor.dirty {
                "Modified"
            } else {
                "Saved"
            },
            cursor.0 + 1,
            cursor.1 + 1,
            editor.body.lines().len()
        ))
        .style(Style::default().fg(MUTED)),
        rows[2],
    );
}
fn overlay(frame: &mut Frame<'_>, area: Rect, heading: &str, text: &str) {
    let width = area.width.saturating_sub(4).min(76);
    let height = area.height.saturating_sub(2).clamp(1, 12);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(sanitize(text))
            .style(Style::default().fg(TEXT).bg(PANEL))
            .wrap(Wrap { trim: false })
            .block(block(format!(" {heading} "))),
        rect,
    );
}
pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().fg(TEXT).bg(BG)),
        area,
    );
    let compact = area.height < 18;
    let shell = Layout::vertical([
        Constraint::Length(if compact { 1 } else { 2 }),
        Constraint::Min(1),
        Constraint::Length(if compact { 2 } else { 3 }),
    ])
    .split(area);
    let profile = state
        .active
        .as_ref()
        .map(|p| sanitize(&p.name))
        .unwrap_or_else(|| "No profile".into());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" Murm ", Style::default().fg(ACCENT).bold()),
            Span::styled(
                format!(" / {}  ", title(state.screen)),
                Style::default().fg(TEXT),
            ),
            Span::styled(
                format!(
                    "· {profile} · {}",
                    if state.vault.unlocked {
                        "UNLOCKED"
                    } else {
                        "LOCKED"
                    }
                ),
                Style::default().fg(MUTED),
            ),
        ])),
        shell[0],
    );
    let wide = area.width >= 100 && state.screen != Screen::Editor;
    let panels = if wide {
        Layout::horizontal([Constraint::Length(20), Constraint::Min(1)]).split(shell[1])
    } else {
        Layout::horizontal([Constraint::Percentage(100)]).split(shell[1])
    };
    let content = if wide {
        let nav = [
            ("1 Profiles", Screen::Profiles),
            ("2 Articles", Screen::Articles),
            ("3 Drafts", Screen::Drafts),
            ("4 Relays", Screen::Relays),
            ("5 Outbox", Screen::Outbox),
        ];
        let items = nav
            .iter()
            .map(|(label, screen)| {
                ListItem::new(format!(" {label} ")).style(if *screen == state.screen {
                    Style::default()
                        .fg(ACCENT)
                        .bg(Color::Rgb(34, 58, 74))
                        .bold()
                } else {
                    Style::default().fg(MUTED)
                })
            })
            .collect::<Vec<_>>();
        frame.render_widget(List::new(items).block(block(" Workspace ")), panels[0]);
        panels[1]
    } else {
        let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).split(panels[0]);
        let nav = if state.screen == Screen::Editor {
            " Writing · plain Pandoc Markdown"
        } else if area.width < 60 {
            "1 Profiles 2 Read 3 Drafts 4 Net 5 Queue"
        } else {
            " 1 Profiles   2 Articles   3 Drafts   4 Relays   5 Outbox"
        };
        frame.render_widget(
            Paragraph::new(nav).style(Style::default().fg(MUTED)),
            rows[0],
        );
        rows[1]
    };
    if state.help {
        frame.render_widget(Paragraph::new("NAVIGATION\n1–5 / Tab: switch screens · Arrows: select · Enter: open\nEsc: back/cancel · F5: refresh · Ctrl+C: exit\n\nWRITING\nDrafts: N new · E edit inside the TUI · P publish saved draft\nEditor: Tab title/body · Ctrl+S local save · Ctrl+A select all\nShift+arrows select · Ctrl+Z undo · Ctrl+Y redo\nEsc back; unsaved changes require confirmation.\n\nIDENTITY\nProfiles: N generate · I private import · O public import\nU unlock · L lock · R rename · D remove · Shift+P change password\n\nNETWORK\nRelays: N add · Enter info · S sync selected · D remove\nOutbox: R retry pending. Stored is not delivered.\n\nPress ? again to return.").style(Style::default().fg(TEXT)).wrap(Wrap{trim:false}).block(block(" Help ")),content);
    } else if let Some(form) = &state.form {
        let lines = form
            .display_lines()
            .into_iter()
            .map(|s| Line::from(format!(" {s}")))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(lines)
                .style(Style::default().fg(TEXT))
                .wrap(Wrap { trim: false })
                .block(block(format!(" {} ", form.label()))),
            content,
        );
    } else {
        match state.screen {
            Screen::Editor => editor_view(frame, content, state),
            Screen::Article => {
                let panel = block(format!(" {} ", sanitize(&state.detail_title)));
                let inner = panel.inner(content);
                frame.render_widget(panel, content);
                frame.render_widget(
                    Paragraph::new(super::text::viewport(
                        &state.detail,
                        state.scroll,
                        inner.width as usize,
                        inner.height as usize,
                    )),
                    inner,
                );
            }
            Screen::Confirm => frame.render_widget(
                Paragraph::new(sanitize(&state.confirmation_text))
                    .wrap(Wrap { trim: false })
                    .block(block(" Confirm action ")),
                content,
            ),
            _ => list_view(frame, content, state),
        }
    }
    let status = if state.status.is_empty() {
        "Local-first · no relay login".into()
    } else {
        sanitize(&state.status)
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(hints(state)).style(Style::default().fg(ACCENT)),
            Line::from(format!(
                "{}{}",
                if state.busy {
                    "Working · navigation remains available | "
                } else {
                    ""
                },
                status
            ))
            .style(Style::default().fg(MUTED)),
            Line::from(" ? Help  ·  Ctrl+C Exit").style(Style::default().fg(MUTED)),
        ]),
        shell[2],
    );
    if state.editor_leave.is_some() {
        overlay(
            frame,
            content,
            "Unsaved changes",
            "Discard the changes that have not been saved?\n\nY  Discard and leave\nN / Esc  Keep editing\n\nCtrl+S saves locally before you leave.",
        );
    }
}
