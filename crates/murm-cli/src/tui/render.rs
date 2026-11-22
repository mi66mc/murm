use super::*;
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph},
};
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
fn list(frame: &mut Frame<'_>, area: Rect, title: &str, items: Vec<String>, selected: usize) {
    let mut selection = ListState::default()
        .with_selected((!items.is_empty()).then_some(selected.min(items.len().saturating_sub(1))));
    let items = items.into_iter().map(ListItem::new).collect::<Vec<_>>();
    frame.render_stateful_widget(
        List::new(items)
            .block(Block::bordered().title(title.to_owned()))
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("› "),
        area,
        &mut selection,
    );
}
/// Produces only the visible display rows; no byte slicing or terminal controls.
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
pub fn render(frame: &mut Frame<'_>, state: &AppState) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(3),
    ])
    .split(frame.area());
    let profile = state
        .active
        .as_ref()
        .map(|p| format!("{} · {} · {:?}", sanitize(&p.name), p.public_key, p.mode))
        .unwrap_or_else(|| "Nenhum perfil selecionado".into());
    let header = format!(
        "Murm · experimental · {}\n{profile}",
        if state.vault.unlocked {
            "cofre aberto"
        } else {
            "cofre bloqueado"
        }
    );
    frame.render_widget(Paragraph::new(header).block(Block::bordered()), areas[0]);
    frame.render_widget(
        Paragraph::new("1 Perfis  2 Artigos  3 Rascunhos  4 Relays  5 Fila  |  ? Ajuda"),
        areas[1],
    );
    if state.help {
        frame.render_widget(Paragraph::new("Setas: navegar · Enter: abrir/confirmar · Esc: voltar/cancelar · Ctrl-C: sair\nPerfis: n gerar · i importar privada · o importar pública · u desbloquear · l bloquear · r renomear · d remover\nTab: próximo campo. Senhas/privadas não são exibidas. Não há login no relay.").block(Block::bordered().title("Ajuda")),areas[2]);
    } else if let Some(form) = &state.form {
        frame.render_widget(
            Paragraph::new(format!(
                "{}\n\nTab: próximo campo · Enter: confirmar · Esc: cancelar",
                form.display_lines().join("\n")
            ))
            .block(Block::bordered().title(format!("{:?}", form.kind))),
            areas[2],
        );
    } else if state.screen == Screen::Profiles {
        let items = state
            .profiles
            .iter()
            .map(|p| {
                ListItem::new(format!(
                    "{} · {:?} · {}\n{}",
                    sanitize(&p.name),
                    p.mode,
                    if p.mode == crate::ProfileMode::ReadOnly {
                        "leitura"
                    } else if state.vault.unlocked {
                        "assinatura disponível"
                    } else {
                        "bloqueado"
                    },
                    p.public_key
                ))
            })
            .collect::<Vec<_>>();
        let mut selection = ListState::default().with_selected(
            (!items.is_empty()).then_some(state.selected.min(items.len().saturating_sub(1))),
        );
        frame.render_stateful_widget(
            List::new(items)
                .block(
                    Block::bordered()
                        .title("Perfis locais · n gerar · i privada · o pública · u desbloquear"),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("› "),
            areas[2],
            &mut selection,
        );
    } else {
        use super::views;
        let area = areas[2];
        match state.screen {
            Screen::Articles => list(
                frame,
                area,
                "Artigos · visão atual local · Enter ler · h histórico · e revisar/restaurar · w retirar · s sync",
                state.articles.iter().map(views::articles::label).collect(),
                state.selected,
            ),
            Screen::Drafts => list(
                frame,
                area,
                "Rascunhos · n novo · e editor externo · p publicar (desbloqueado)",
                state.drafts.iter().map(views::drafts::label).collect(),
                state.selected,
            ),
            Screen::Relays => list(
                frame,
                area,
                "Relays deste perfil · n adicionar · Enter /info · s sync · d remover",
                state
                    .active
                    .as_ref()
                    .map(|p| p.relays.iter().map(|u| views::relays::label(u)).collect())
                    .unwrap_or_default(),
                state.selected,
            ),
            Screen::Outbox => list(
                frame,
                area,
                "Fila · r tentar pendentes · stored não significa entrega/leitura",
                state
                    .outbox
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}\n{}",
                            entry.event_id,
                            entry
                                .destinations
                                .iter()
                                .map(views::outbox::destination_label)
                                .collect::<Vec<_>>()
                                .join("\n")
                        )
                    })
                    .collect(),
                state.selected,
            ),
            Screen::History => list(
                frame,
                area,
                "Histórico retido local · não é a visão atual · Enter edição exata",
                state
                    .history
                    .iter()
                    .map(|e| {
                        format!(
                            "rev {} · {} · {}",
                            e.event()
                                .header
                                .get("revision")
                                .map(|v| v.to_string())
                                .unwrap_or_default(),
                            sanitize(
                                e.event()
                                    .header
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                            ),
                            e.event().id
                        )
                    })
                    .collect(),
                state.selected,
            ),
            Screen::Article => {
                let block = Block::bordered().title(sanitize(&state.detail_title));
                let inner = block.inner(area);
                frame.render_widget(block, area);
                frame.render_widget(
                    Paragraph::new(viewport(
                        &state.detail,
                        state.scroll,
                        inner.width as usize,
                        inner.height as usize,
                    )),
                    inner,
                );
            }
            Screen::Confirm => {
                let block = Block::bordered().title("Confirme com Enter; Esc cancela");
                let inner = block.inner(area);
                frame.render_widget(block, area);
                frame.render_widget(
                    Paragraph::new(viewport(
                        &state.confirmation_text,
                        0,
                        inner.width as usize,
                        inner.height as usize,
                    )),
                    inner,
                );
            }
            _ => frame.render_widget(
                Paragraph::new("Processando formulário…").block(Block::bordered()),
                area,
            ),
        }
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{}{}",
            if state.busy {
                "Trabalhando… Esc cancela a operação local. "
            } else {
                ""
            },
            sanitize(&state.status)
        ))
        .block(Block::bordered()),
        areas[3],
    );
}
