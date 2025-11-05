use std::{collections::BTreeMap, ops::Deref, sync::Arc};

use miden_assembly::diagnostics::SourceCode;
use midenc_session::diagnostics::{LineIndex, Report, SourceFile, SourceId, SourceSpan};
use ratatui::{
    prelude::*,
    widgets::{block::*, *},
};

use crate::{
    ui::{
        action::Action,
        panes::Pane,
        state::State,
        syntax_highlighting::{Highlighter, HighlighterState, NoopHighlighter, SyntectHighlighter},
        tui::Frame,
    },
    ResolvedLocation,
};

pub struct SourceCodePane {
    focused: bool,
    current_source_id: SourceId,
    current_span: SourceSpan,
    current_line: u32,
    current_col: u32,
    num_lines: u32,
    selected_line: u32,
    syntax_highlighter: Box<dyn Highlighter>,
    syntax_highlighting_states: BTreeMap<SourceId, Box<dyn HighlighterState>>,
    current_file: Option<HighlightedFile>,
    theme: Theme,
}

struct HighlightedFile {
    source_file: Arc<SourceFile>,
    /// The syntax highlighted lines of `source_file`, cached so that patching
    /// them with the current selected line can be done efficiently
    lines: Vec<Vec<Span<'static>>>,
    selected_line: u32,
    selected_span: SourceSpan,
    gutter_width: u8,
}

impl SourceCodePane {
    fn highlight_file(&mut self, resolved: &ResolvedLocation) -> HighlightedFile {
        let highlighter_state = self
            .syntax_highlighting_states
            .entry(resolved.source_file.id())
            .or_insert_with(|| {
                let span_contents = resolved
                    .source_file
                    .read_span(&resolved.source_file.source_span().into(), 0, 0)
                    .expect("failed to read span of file");
                self.syntax_highlighter.start_highlighter_state(span_contents.as_ref())
            });
        let resolved_span = resolved.span.into_slice_index();
        let content = resolved.source_file.content();
        let last_line = content.last_line_index();
        let max_line_no = last_line.number().to_usize();
        let gutter_width = max_line_no.ilog10() as u8;
        let lines = (0..(max_line_no - 1))
            .map(|line_index| {
                let line_index = miden_debug_types::LineIndex::from(line_index as u32);
                let line_no = line_index.number().to_u32();
                let span = content.line_range(line_index).expect("invalid line index");
                let span = span.start.to_usize()..span.end.to_usize();

                let line_content = strip_newline(&content.as_bytes()[span.start..span.end]);

                // Only highlight a portion of the line if the full span fits on that line
                let is_highlighted = span.contains(&resolved_span.start)
                    && span.contains(&resolved_span.end)
                    && span != resolved_span;

                let line_content =
                    strip_newline(&content.as_bytes()[span.start..span.end]).into_owned();
                let highlighted = if is_highlighted {
                    let selection = if resolved.span.is_empty() {
                        // Select the closest character to the span
                        //let start = core::cmp::max(span.start, resolved_span.start);
                        //let end = core::cmp::min(span.end, resolved_span.end.saturating_add(1));
                        //(start - span.start)..(end - span.start)
                        0..(span.end - span.start)
                    } else {
                        (resolved_span.start - span.start)..(resolved_span.end - span.start)
                    };
                    highlighter_state.highlight_line_with_selection(
                        line_content.into(),
                        selection,
                        self.theme.current_span,
                    )
                } else {
                    highlighter_state.highlight_line(line_content.into())
                };

                highlighted
            })
            .collect::<Vec<_>>();

        HighlightedFile {
            source_file: resolved.source_file.clone(),
            lines,
            selected_line: resolved.line,
            selected_span: resolved.span,
            gutter_width,
        }
    }

    /// Get the cached lines of the source file, or compute them for the first time
    /// if the file has changed.
    ///
    /// Each line consists of a vector of styled [Span]s, so that we can modify the
    /// styles based on the relevant source span.
    fn current_source_lines(&mut self, resolved: &ResolvedLocation) -> Vec<Vec<Span<'static>>> {
        let file_changed = self
            .current_file
            .as_ref()
            .map(|file| file.source_file.id() != resolved.source_file.id())
            .unwrap_or(true);

        // NOTE: We could cache all of the files we highlight, but that could get memory-dense
        if file_changed {
            let file = self.highlight_file(resolved);
            let lines = file.lines.clone();
            self.current_file = Some(file);
            lines
        } else {
            self.current_file.as_ref().unwrap().lines.clone()
        }
    }

    /// Get the [ResolvedLocation] for the current state
    fn current_location(&self, state: &State) -> Option<ResolvedLocation> {
        match state.executor.callstack.current_frame() {
            Some(frame) => {
                let resolved = frame.last_resolved(&state.session);
                resolved.cloned()
            }
            None if !self.current_source_id.is_unknown() => {
                let source_file = state.session.source_manager.get(self.current_source_id).ok();
                source_file.map(|src| ResolvedLocation {
                    source_file: src,
                    line: self.current_line,
                    col: self.current_col,
                    span: self.current_span,
                })
            }
            None => {
                // Render empty source pane
                None
            }
        }
    }
}

struct Theme {
    focused_border_style: Style,
    current_line: Style,
    current_span: Style,
    line_number: Style,
    gutter_border: Style,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            focused_border_style: Style::default(),
            current_line: Style::default()
                .bg(Color::Black)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            current_span: Style::default()
                .fg(Color::White)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
            line_number: Style::default(),
            gutter_border: Style::default(),
        }
    }
}
impl Theme {
    pub fn patch_from_syntect(&mut self, theme: &syntect::highlighting::Theme) {
        use crate::ui::syntax_highlighting::convert_color;
        if let Some(bg) = theme.settings.line_highlight.map(convert_color) {
            self.current_line.bg = Some(bg);
        }
        if let Some(bg) = theme.settings.selection.map(convert_color) {
            self.current_span.bg = Some(bg);
        }
        if let Some(fg) = theme.settings.selection_foreground.map(convert_color) {
            self.current_span.fg = Some(fg);
        }
        if let Some(bg) = theme.settings.gutter.map(convert_color) {
            self.line_number.bg = Some(bg);
            self.gutter_border.bg = Some(bg);
        }
        if let Some(fg) = theme.settings.gutter_foreground.map(convert_color) {
            self.line_number.fg = Some(fg);
            self.gutter_border.fg = Some(fg);
        }
    }
}

impl SourceCodePane {
    pub fn new(focused: bool, focused_border_style: Style) -> Self {
        let theme = Theme {
            focused_border_style,
            ..Default::default()
        };
        Self {
            focused,
            current_source_id: SourceId::UNKNOWN,
            num_lines: 0,
            selected_line: 0,
            current_line: 0,
            current_col: 0,
            current_span: SourceSpan::default(),
            syntax_highlighter: Box::new(NoopHighlighter),
            syntax_highlighting_states: Default::default(),
            current_file: None,
            theme,
        }
    }

    fn reload(&mut self, state: &State) {
        self.current_source_id = SourceId::UNKNOWN;
        self.current_span = SourceSpan::default();
        self.current_line = 0;
        self.current_col = 0;
        self.num_lines = 0;
        self.selected_line = 0;
        self.current_file = None;

        if let Some(frame) = state.executor.callstack.current_frame() {
            if let Some(loc) = frame.last_resolved(&state.session) {
                self.current_source_id = loc.source_file.id();
                self.current_span = loc.span;
                self.current_line = loc.line;
                self.current_col = loc.col;
                self.num_lines = loc.source_file.line_count() as u32;
                self.selected_line = loc.line;
            }
        }
    }

    fn border_style(&self) -> Style {
        match self.focused {
            true => self.theme.focused_border_style,
            false => Style::default(),
        }
    }

    fn border_type(&self) -> BorderType {
        match self.focused {
            true => BorderType::Thick,
            false => BorderType::Plain,
        }
    }

    fn enable_syntax_highlighting(&mut self, state: &State) {
        use std::io::IsTerminal;

        use midenc_session::ColorChoice;

        let nocolor = !state.session.options.color.should_attempt_color();
        if nocolor {
            return;
        }

        let syntax_set = syntect::parsing::SyntaxSet::load_defaults_nonewlines();
        let theme_set = syntect::highlighting::ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-eighties.dark"].clone();
        self.theme.patch_from_syntect(&theme);
        self.syntax_highlighter = Box::new(SyntectHighlighter::new(syntax_set, theme, false));
    }
}

impl Pane for SourceCodePane {
    fn init(&mut self, state: &State) -> Result<(), Report> {
        self.enable_syntax_highlighting(state);

        if let Some(frame) = state.executor.callstack.current_frame() {
            if let Some(loc) = frame.last_resolved(&state.session) {
                self.current_source_id = loc.source_file.id();
                self.current_span = loc.span;
                self.current_line = loc.line;
                self.current_col = loc.col;
                self.num_lines = loc.source_file.line_count() as u32;
                self.selected_line = loc.line;
            }
        }

        Ok(())
    }

    fn height_constraint(&self) -> Constraint {
        match self.focused {
            true => Constraint::Fill(3),
            false => Constraint::Fill(3),
        }
    }

    fn update(&mut self, action: Action, state: &mut State) -> Result<Option<Action>, Report> {
        match action {
            Action::Down => {
                if self.num_lines > 0 {
                    self.selected_line = self.selected_line.saturating_add(1) % self.num_lines;
                }
                return Ok(Some(Action::Update));
            }
            Action::Up => {
                if self.num_lines > 0 {
                    self.selected_line =
                        self.selected_line.saturating_add(self.num_lines.saturating_sub(1))
                            % self.num_lines;
                }
                return Ok(Some(Action::Update));
            }
            Action::Focus => {
                self.focused = true;
                static STATUS_LINE: &str = "[j,k → movement]";
                return Ok(Some(Action::TimedStatusLine(STATUS_LINE.into(), 3)));
            }
            Action::UnFocus => {
                self.focused = false;
            }
            Action::Submit => {}
            Action::Update | Action::Reload => {
                if action == Action::Reload {
                    self.reload(state);
                }

                if let Some(frame) = state.executor.callstack.current_frame() {
                    if let Some(loc) = frame.last_resolved(&state.session) {
                        let source_id = loc.source_file.id();
                        if source_id != self.current_source_id {
                            self.current_source_id = source_id;
                            self.num_lines = loc.source_file.line_count() as u32;
                            self.selected_line = loc.line;
                        } else if self.selected_line != loc.line {
                            self.selected_line = loc.line;
                        }
                        self.current_span = loc.span;
                        self.current_line = loc.line;
                        self.current_col = loc.col;
                    }
                }
            }
            _ => {}
        }

        Ok(None)
    }

    fn draw(&mut self, frame: &mut Frame<'_>, area: Rect, state: &State) -> Result<(), Report> {
        let resolved = self.current_location(state);
        if resolved.is_none() {
            frame.render_widget(
                Block::default()
                    .title("Source Code")
                    .borders(Borders::ALL)
                    .border_style(self.border_style())
                    .border_type(self.border_type())
                    .title_bottom(
                        Line::from("no source code available for current instruction")
                            .right_aligned(),
                    )
                    .title(
                        Line::styled("nofile", Style::default().add_modifier(Modifier::ITALIC))
                            .right_aligned(),
                    ),
                area,
            );
            return Ok(());
        }

        let resolved = unsafe { resolved.unwrap_unchecked() };

        // Get the cached (highlighted) lines for the current source file
        let mut lines = self.current_source_lines(&resolved);
        let selected_line = resolved.line.saturating_sub(1) as usize;
        // Extract the current selected line as a vector of raw syntect parts
        let selected_line_deconstructed = lines[selected_line]
            .iter()
            .map(|span| {
                (
                    crate::ui::syntax_highlighting::convert_to_syntect_style(span.style, false),
                    span.content.as_ref(),
                )
            })
            .collect::<Vec<_>>();

        // Modify the selected line's highlighting style to reflect the selection
        let syntect_style = syntect::highlighting::StyleModifier {
            foreground: self
                .theme
                .current_span
                .fg
                .map(crate::ui::syntax_highlighting::convert_to_syntect_color),
            background: self
                .theme
                .current_span
                .bg
                .map(crate::ui::syntax_highlighting::convert_to_syntect_color),
            font_style: if self.theme.current_span.add_modifier.is_empty() {
                None
            } else {
                Some(crate::ui::syntax_highlighting::convert_to_font_style(
                    self.theme.current_span.add_modifier,
                ))
            },
        };
        let span = resolved
            .source_file
            .content()
            .line_range((selected_line as u32).into())
            .unwrap();
        let resolved_span = resolved.span.into_slice_index();
        let selected = if resolved.span.is_empty() {
            // Select the closest character to the span
            0..(span.end.to_usize() - span.start.to_usize())
        } else {
            (resolved_span.start - span.start.to_usize())
                ..(resolved_span.end - span.start.to_usize())
        };
        let highlighter_state =
            self.syntax_highlighting_states.get_mut(&resolved.source_file.id()).unwrap();
        let mut parts = syntect::util::modify_range(
            selected_line_deconstructed.as_slice(),
            selected,
            syntect_style,
        )
        .into_iter()
        .map(|(style, str)| {
            Span::styled(
                str.to_string(),
                crate::ui::syntax_highlighting::convert_style(style, true),
            )
        })
        .collect();
        lines[selected_line].clear();
        lines[selected_line].append(&mut parts);

        let gutter_width = self.current_file.as_ref().unwrap().gutter_width as usize;
        let lines = lines.into_iter().enumerate().map(|(line_index, highlighted_parts)| {
            let line_number_style = if line_index == selected_line {
                self.theme.current_line
            } else {
                self.theme.line_number
            };
            Line::from_iter(
                [
                    Span::styled(
                        format!("{line_no:gutter_width$}", line_no = line_index + 1),
                        line_number_style,
                    ),
                    Span::styled(" | ", line_number_style),
                ]
                .into_iter()
                .chain(highlighted_parts),
            )
        });

        // Render the syntax-highlighted lines
        let selected_line = self.selected_line.saturating_sub(1);
        let list = List::new(lines)
            .block(Block::default().borders(Borders::ALL))
            .highlight_symbol(symbols::scrollbar::HORIZONTAL.end)
            .highlight_spacing(HighlightSpacing::Always)
            .scroll_padding(15);
        let mut list_state = ListState::default().with_selected(Some(selected_line as usize));

        frame.render_stateful_widget(list, area, &mut list_state);
        frame.render_widget(
            Block::default()
                .title("Source Code")
                .borders(Borders::ALL)
                .border_style(self.border_style())
                .border_type(self.border_type())
                .title_bottom(
                    Line::from(format!("{} of {}", self.selected_line, self.num_lines,))
                        .right_aligned(),
                )
                .title(
                    Line::styled(
                        resolved.source_file.deref().uri().as_str(),
                        Style::default().add_modifier(Modifier::ITALIC),
                    )
                    .right_aligned(),
                ),
            area,
        );
        Ok(())
    }
}

fn strip_newline(s: &[u8]) -> std::borrow::Cow<'_, str> {
    if let Some(sans_newline) = s.strip_suffix(b"\n") {
        String::from_utf8_lossy(sans_newline)
    } else {
        String::from_utf8_lossy(s)
    }
}
