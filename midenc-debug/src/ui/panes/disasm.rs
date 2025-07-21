use midenc_session::diagnostics::{Report, SourceId, SourceSpan};
use ratatui::{
    prelude::*,
    widgets::{block::*, *},
};

use crate::{
    ui::{action::Action, panes::Pane, state::State, tui::Frame},
    ResolvedLocation,
};

pub struct DisassemblyPane {
    focused: bool,
    focused_border_style: Style,
}

impl DisassemblyPane {
    pub fn new(focused: bool, focused_border_style: Style) -> Self {
        Self {
            focused,
            focused_border_style,
        }
    }

    fn border_style(&self) -> Style {
        match self.focused {
            true => self.focused_border_style,
            false => Style::default(),
        }
    }

    fn border_type(&self) -> BorderType {
        match self.focused {
            true => BorderType::Thick,
            false => BorderType::Plain,
        }
    }
}

impl Pane for DisassemblyPane {
    fn height_constraint(&self) -> Constraint {
        match self.focused {
            true => Constraint::Max(7),
            false => Constraint::Max(7),
        }
    }

    fn update(&mut self, action: Action, _state: &mut State) -> Result<Option<Action>, Report> {
        match action {
            Action::Focus => {
                self.focused = true;
            }
            Action::UnFocus => {
                self.focused = false;
            }
            _ => {}
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, state: &State) -> Result<(), Report> {
        let (current_proc, lines) = match state.executor.callstack.current_frame() {
            None => {
                let proc = Line::from("in <unknown>").right_aligned();
                (
                    proc,
                    state
                        .executor
                        .recent
                        .iter()
                        .map(|op| Line::from(vec![Span::styled(format!(" | {op}"), Color::White)]))
                        .collect::<Vec<_>>(),
                )
            }
            Some(frame) => {
                let proc = frame
                    .procedure(state.session.name())
                    .map(|proc| Line::from(format!("in {proc}")))
                    .unwrap_or_else(|| Line::from("in <unknown>"))
                    .right_aligned();
                (
                    proc,
                    frame
                        .recent()
                        .iter()
                        .map(|op| {
                            Line::from(vec![Span::styled(
                                format!(" | {}", &op.display()),
                                Color::White,
                            )])
                        })
                        .collect::<Vec<_>>(),
                )
            }
        };
        let selected_line = lines.len().saturating_sub(1);

        let list = List::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .highlight_symbol(symbols::scrollbar::HORIZONTAL.end)
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(Style::default().add_modifier(Modifier::BOLD));
        let mut list_state = ListState::default().with_selected(Some(selected_line));

        frame.render_stateful_widget(list, area, &mut list_state);
        frame.render_widget(
            Block::default()
                .title("Disassembly")
                .borders(Borders::ALL)
                .border_style(self.border_style())
                .border_type(self.border_type())
                .title_bottom(current_proc)
                .title(
                    Line::styled(
                        format!(" at cycle {}", state.executor.cycle),
                        Style::default().add_modifier(Modifier::ITALIC),
                    )
                    .right_aligned(),
                ),
            area,
        );
        Ok(())
    }
}
