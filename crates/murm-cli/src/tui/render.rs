use super::*;
use ratatui::{Frame,layout::{Layout,Constraint},widgets::{Block,List,ListItem,ListState,Paragraph},style::{Style,Color,Modifier}};
pub fn sanitize(text:&str)->String{
    let mut output=String::new();for c in text.chars(){
        match c{'\n'=>output.push('\n'),'\t'=>output.push_str("  "),c if c.is_control()=>output.push_str(&format!("\\u{{{:04x}}}",c as u32)),c=>output.push(c)}
    }output
}
pub fn render(frame:&mut Frame<'_>,state:&AppState){
    let areas=Layout::vertical([Constraint::Length(3),Constraint::Length(2),Constraint::Min(3),Constraint::Length(3)]).split(frame.area());
    let profile=state.active.as_ref().map(|p|format!("{} · {} · {:?}",sanitize(&p.name),p.public_key,p.mode)).unwrap_or_else(||"Nenhum perfil selecionado".into());
    let header=format!("Murm · experimental · {}\n{profile}",if state.vault.unlocked{"cofre aberto"}else{"cofre bloqueado"});
    frame.render_widget(Paragraph::new(header).block(Block::bordered()),areas[0]);
    frame.render_widget(Paragraph::new("1 Perfis  2 Artigos  3 Rascunhos  4 Relays  5 Fila  |  ? Ajuda"),areas[1]);
    if state.help{
        frame.render_widget(Paragraph::new("Setas: navegar · Enter: abrir/confirmar · Esc: voltar/cancelar · Ctrl-C: sair\nPerfis: n gerar · i importar privada · o importar pública · u desbloquear · l bloquear · r renomear · d remover\nTab: próximo campo. Senhas/privadas não são exibidas. Não há login no relay.").block(Block::bordered().title("Ajuda")),areas[2]);
    }else if let Some(form)=&state.form{
        frame.render_widget(Paragraph::new(format!("{}\n\nTab: próximo campo · Enter: confirmar · Esc: cancelar",form.display_lines().join("\n"))).block(Block::bordered().title(format!("{:?}",form.kind))),areas[2]);
    }else if state.screen==Screen::Profiles{
        let items=state.profiles.iter().map(|p|ListItem::new(format!("{} · {:?} · {}\n{}",sanitize(&p.name),p.mode,if p.mode==crate::ProfileMode::ReadOnly{"leitura"}else if state.vault.unlocked{"assinatura disponível"}else{"bloqueado"},p.public_key))).collect::<Vec<_>>();
        let mut selection=ListState::default().with_selected((!items.is_empty()).then_some(state.selected.min(items.len().saturating_sub(1))));
        frame.render_stateful_widget(List::new(items).block(Block::bordered().title("Perfis locais · n gerar · i privada · o pública · u desbloquear")).highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)).highlight_symbol("› "),areas[2],&mut selection);
    }else{
        frame.render_widget(Paragraph::new("Tela em preparação. Use 1 para voltar aos perfis.").block(Block::bordered()),areas[2]);
    }
    frame.render_widget(Paragraph::new(format!("{}{}",
        if state.busy{"Trabalhando… Esc cancela a operação local. "}else{""},sanitize(&state.status))).block(Block::bordered()),areas[3]);
}
