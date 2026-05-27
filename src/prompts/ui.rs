use crate::initializr::metadata::SelectOption;
use anyhow::{Context, Result, anyhow, bail};
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::Stylize,
    terminal::{self, Clear, ClearType},
};
use inquire::{Confirm, Text};
use std::{
    collections::BTreeSet,
    io::{self, Write},
};

pub trait Prompter {
    fn text(&mut self, message: &str, default: Option<&str>) -> Result<String>;

    fn select(
        &mut self,
        message: &str,
        options: &[SelectOption],
        default_id: Option<&str>,
    ) -> Result<String>;

    fn confirm(&mut self, message: &str, default: bool) -> Result<bool> {
        let _ = (message, default);
        bail!("confirmation prompts are not supported by this prompter")
    }

    fn multi_select(
        &mut self,
        message: &str,
        options: &[SelectOption],
        default_ids: &[String],
    ) -> Result<Vec<String>> {
        let _ = (message, options, default_ids);
        bail!("multi-select prompts are not supported by this prompter")
    }
}

pub struct InquirePrompter;

impl Prompter for InquirePrompter {
    fn text(&mut self, message: &str, default: Option<&str>) -> Result<String> {
        let mut prompt = Text::new(message);
        if let Some(default) = default {
            prompt = prompt.with_default(default);
        }
        prompt
            .prompt()
            .with_context(|| format!("failed to read response for prompt: {message}"))
    }

    fn select(
        &mut self,
        message: &str,
        options: &[SelectOption],
        default_id: Option<&str>,
    ) -> Result<String> {
        if options.is_empty() {
            return Err(anyhow!(
                "no Spring Initializr options available for prompt: {message}"
            ));
        }

        let labels = format_option_labels(options);
        let starting_cursor = default_id
            .and_then(|id| options.iter().position(|option| option.id == id))
            .unwrap_or(0);

        let selected_index = prompt_select(message, labels, starting_cursor)
            .with_context(|| format!("failed to read selection for prompt: {message}"))?;

        Ok(options[selected_index].id.clone())
    }

    fn confirm(&mut self, message: &str, default: bool) -> Result<bool> {
        Confirm::new(message)
            .with_default(default)
            .prompt()
            .with_context(|| format!("failed to read confirmation for prompt: {message}"))
    }

    fn multi_select(
        &mut self,
        message: &str,
        options: &[SelectOption],
        default_ids: &[String],
    ) -> Result<Vec<String>> {
        if options.is_empty() {
            return Ok(Vec::new());
        }

        let labels = format_option_labels(options);
        let ids = options.iter().map(|option| option.id.clone()).collect();
        prompt_multi_select(message, ids, labels, default_ids)
            .with_context(|| format!("failed to read selections for prompt: {message}"))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptKey {
    Char(char),
    Space,
    Backspace,
    Enter,
    Escape,
    Up,
    Down,
    Tab,
    BackTab,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectPromptOutcome {
    Continue,
    Selected(usize),
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectPromptState {
    labels: Vec<String>,
    cursor: usize,
}

impl SelectPromptState {
    pub(crate) fn new(labels: Vec<String>, starting_cursor: usize) -> Self {
        let cursor = if labels.is_empty() {
            0
        } else {
            starting_cursor.min(labels.len() - 1)
        };
        Self { labels, cursor }
    }

    pub(crate) fn apply_key(&mut self, key: PromptKey) -> SelectPromptOutcome {
        match key {
            PromptKey::Up | PromptKey::BackTab | PromptKey::Char('k') => {
                self.move_up();
                SelectPromptOutcome::Continue
            }
            PromptKey::Down | PromptKey::Tab | PromptKey::Char('j') => {
                self.move_down();
                SelectPromptOutcome::Continue
            }
            PromptKey::Enter => {
                if self.labels.is_empty() {
                    SelectPromptOutcome::Continue
                } else {
                    SelectPromptOutcome::Selected(self.cursor)
                }
            }
            PromptKey::Escape => SelectPromptOutcome::Canceled,
            PromptKey::Char(_) | PromptKey::Space | PromptKey::Backspace => {
                SelectPromptOutcome::Continue
            }
        }
    }

    pub(crate) fn cursor_label(&self) -> Option<&str> {
        self.labels.get(self.cursor).map(String::as_str)
    }

    fn move_up(&mut self) {
        if self.labels.is_empty() {
            return;
        }
        self.cursor = if self.cursor == 0 {
            self.labels.len() - 1
        } else {
            self.cursor - 1
        };
    }

    fn move_down(&mut self) {
        if self.labels.is_empty() {
            return;
        }
        self.cursor = (self.cursor + 1) % self.labels.len();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MultiSelectFocus {
    Search,
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MultiSelectPromptOutcome {
    Continue,
    Done(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MultiSelectPromptState {
    ids: Vec<String>,
    labels: Vec<String>,
    query: String,
    filtered: Vec<usize>,
    cursor: usize,
    selected: BTreeSet<usize>,
    focus: MultiSelectFocus,
}

impl MultiSelectPromptState {
    pub(crate) fn new(ids: Vec<String>, labels: Vec<String>, default_ids: &[String]) -> Self {
        let selected = ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| default_ids.contains(id).then_some(index))
            .collect();
        let mut state = Self {
            ids,
            labels,
            query: String::new(),
            filtered: Vec::new(),
            cursor: 0,
            selected,
            focus: MultiSelectFocus::Search,
        };
        state.refresh_filter();
        state
    }

    pub(crate) fn apply_key(&mut self, key: PromptKey) -> MultiSelectPromptOutcome {
        match self.focus {
            MultiSelectFocus::Search => self.apply_search_key(key),
            MultiSelectFocus::List => self.apply_list_key(key),
        }
    }

    pub(crate) fn focus(&self) -> MultiSelectFocus {
        self.focus
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    #[cfg(test)]
    pub(crate) fn cursor_id(&self) -> Option<&str> {
        self.filtered
            .get(self.cursor)
            .and_then(|index| self.ids.get(*index))
            .map(String::as_str)
    }

    pub(crate) fn selected_ids(&self) -> Vec<String> {
        self.ids_from_selected()
    }

    pub(crate) fn rendered_options(&self) -> Vec<String> {
        self.filtered
            .iter()
            .map(|index| {
                let marker = if self.selected.contains(index) {
                    "■"
                } else {
                    "□"
                };
                format!("{marker} {}", self.labels[*index])
            })
            .collect()
    }

    fn apply_search_key(&mut self, key: PromptKey) -> MultiSelectPromptOutcome {
        match key {
            PromptKey::Char(character) => {
                self.query.push(character);
                self.refresh_filter();
            }
            PromptKey::Space => {
                self.query.push(' ');
                self.refresh_filter();
            }
            PromptKey::Backspace => {
                self.query.pop();
                self.refresh_filter();
            }
            PromptKey::Tab | PromptKey::Down => {
                if !self.filtered.is_empty() {
                    self.focus = MultiSelectFocus::List;
                }
            }
            PromptKey::BackTab | PromptKey::Up => {
                if !self.filtered.is_empty() {
                    self.focus = MultiSelectFocus::List;
                    self.cursor = self.filtered.len() - 1;
                }
            }
            PromptKey::Enter => return MultiSelectPromptOutcome::Done(self.ids_from_selected()),
            PromptKey::Escape => return MultiSelectPromptOutcome::Done(Vec::new()),
        }
        MultiSelectPromptOutcome::Continue
    }

    fn apply_list_key(&mut self, key: PromptKey) -> MultiSelectPromptOutcome {
        match key {
            PromptKey::Down | PromptKey::Tab | PromptKey::Char('j') => self.move_down(),
            PromptKey::Up | PromptKey::BackTab | PromptKey::Char('k') => self.move_up(),
            PromptKey::Space => self.toggle_current(),
            PromptKey::Enter => return MultiSelectPromptOutcome::Done(self.ids_from_selected()),
            PromptKey::Escape => self.focus = MultiSelectFocus::Search,
            PromptKey::Backspace => {
                self.focus = MultiSelectFocus::Search;
                self.query.pop();
                self.refresh_filter();
            }
            PromptKey::Char(character) => {
                self.focus = MultiSelectFocus::Search;
                self.query.push(character);
                self.refresh_filter();
            }
        }
        MultiSelectPromptOutcome::Continue
    }

    fn refresh_filter(&mut self) {
        let query = self.query.trim().to_ascii_lowercase();
        self.filtered = self
            .labels
            .iter()
            .enumerate()
            .filter_map(|(index, label)| {
                let searchable = [label.as_str(), self.ids[index].as_str()]
                    .join(" ")
                    .to_ascii_lowercase();
                query
                    .split_whitespace()
                    .all(|term| searchable.contains(term))
                    .then_some(index)
            })
            .collect();

        if self.filtered.is_empty() {
            self.cursor = 0;
            self.focus = MultiSelectFocus::Search;
        } else if self.cursor >= self.filtered.len() {
            self.cursor = self.filtered.len() - 1;
        }
    }

    fn move_up(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.cursor = if self.cursor == 0 {
            self.filtered.len() - 1
        } else {
            self.cursor - 1
        };
    }

    fn move_down(&mut self) {
        if self.filtered.is_empty() {
            return;
        }
        self.cursor = (self.cursor + 1) % self.filtered.len();
    }

    fn toggle_current(&mut self) {
        let Some(index) = self.filtered.get(self.cursor) else {
            return;
        };
        if !self.selected.remove(index) {
            self.selected.insert(*index);
        }
    }

    fn ids_from_selected(&self) -> Vec<String> {
        self.ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| self.selected.contains(&index).then_some(id.clone()))
            .collect()
    }
}

fn prompt_select(message: &str, labels: Vec<String>, starting_cursor: usize) -> Result<usize> {
    let mut state = SelectPromptState::new(labels, starting_cursor);
    let _raw_mode = RawModeGuard::enable()?;
    let mut stdout = io::stdout();
    let mut rendered_lines = 0;

    loop {
        rendered_lines = render_select_prompt(&mut stdout, rendered_lines, message, &state)?;
        let key = read_prompt_key()?;
        match state.apply_key(key) {
            SelectPromptOutcome::Continue => {}
            SelectPromptOutcome::Selected(index) => {
                clear_prompt(&mut stdout, rendered_lines)?;
                let answer = state.cursor_label().unwrap_or_default();
                write!(stdout, "{}\r\n", completed_prompt_line(message, answer))?;
                stdout.flush()?;
                return Ok(index);
            }
            SelectPromptOutcome::Canceled => {
                clear_prompt(&mut stdout, rendered_lines)?;
                stdout.flush()?;
                bail!("prompt canceled")
            }
        }
    }
}

fn prompt_multi_select(
    message: &str,
    ids: Vec<String>,
    labels: Vec<String>,
    default_ids: &[String],
) -> Result<Vec<String>> {
    let mut state = MultiSelectPromptState::new(ids, labels, default_ids);
    let _raw_mode = RawModeGuard::enable()?;
    let mut stdout = io::stdout();
    let mut rendered_lines = 0;

    loop {
        rendered_lines = render_multi_select_prompt(&mut stdout, rendered_lines, message, &state)?;
        let key = read_prompt_key()?;
        match state.apply_key(key) {
            MultiSelectPromptOutcome::Continue => {}
            MultiSelectPromptOutcome::Done(ids) => {
                clear_prompt(&mut stdout, rendered_lines)?;
                let answer = if ids.is_empty() {
                    "none".to_string()
                } else {
                    ids.join(", ")
                };
                write!(stdout, "{}\r\n", completed_prompt_line(message, &answer))?;
                stdout.flush()?;
                return Ok(ids);
            }
        }
    }
}

fn render_select_prompt(
    stdout: &mut impl Write,
    rendered_lines: usize,
    message: &str,
    state: &SelectPromptState,
) -> Result<usize> {
    let mut lines = vec![question_line(message)];

    let options = select_visible_options(state);
    if options.is_empty() {
        lines.push(warning_line("  No matches"));
    } else {
        lines.extend(options);
    }

    lines.push(help_line("  Use arrows or j/k to move. Enter to select."));
    render_lines(stdout, rendered_lines, &lines)?;
    Ok(lines.len())
}

fn select_visible_options(state: &SelectPromptState) -> Vec<String> {
    visible_positions(state.labels.len(), state.cursor, 10)
        .map(|position| {
            let prefix = if position == state.cursor { ">" } else { " " };
            let line = format!("{prefix} {}", state.labels[position]);
            if position == state.cursor {
                active_line(line)
            } else {
                muted_line(line)
            }
        })
        .collect()
}

fn render_multi_select_prompt(
    stdout: &mut impl Write,
    rendered_lines: usize,
    message: &str,
    state: &MultiSelectPromptState,
) -> Result<usize> {
    let selected = state.selected_ids();
    let selected_label = if selected.is_empty() {
        "none".to_string()
    } else {
        selected.join(", ")
    };
    let focus = match state.focus() {
        MultiSelectFocus::Search => "search",
        MultiSelectFocus::List => "list",
    };
    let mut lines = vec![
        question_line(message),
        focus_line(
            format!("  search: {} ({focus})", state.query()),
            state.focus() == MultiSelectFocus::Search,
        ),
        selected_line(
            format!("  selected: {selected_label}"),
            !selected.is_empty(),
        ),
    ];

    let rendered_options = state.rendered_options();
    let options = visible_positions(rendered_options.len(), state.cursor, 10)
        .map(|position| {
            let prefix = if state.focus() == MultiSelectFocus::List && position == state.cursor {
                ">"
            } else {
                " "
            };
            let line = format!("{prefix} {}", rendered_options[position]);
            if state.focus() == MultiSelectFocus::List && position == state.cursor {
                active_line(line)
            } else if rendered_options[position].starts_with('■') {
                success_line(line)
            } else {
                muted_line(line)
            }
        })
        .collect::<Vec<_>>();

    if options.is_empty() {
        lines.push(warning_line("  No matches"));
    } else {
        lines.extend(options);
    }

    lines.push(help_line(
        "  Type to filter. Tab focuses list. Space toggles. Enter saves. Esc returns to search.",
    ));
    render_lines(stdout, rendered_lines, &lines)?;
    Ok(lines.len())
}

fn question_line(message: &str) -> String {
    active_line(format!("? {message}"))
}

fn completed_prompt_line(message: &str, answer: &str) -> String {
    success_line(format!("> {message} {answer}"))
}

fn focus_line(line: String, focused: bool) -> String {
    if focused {
        active_line(line)
    } else {
        muted_line(line)
    }
}

fn selected_line(line: String, has_selection: bool) -> String {
    if has_selection {
        success_line(line)
    } else {
        muted_line(line)
    }
}

fn active_line(line: impl Into<String>) -> String {
    format!("{}", line.into().cyan().bold())
}

fn success_line(line: impl Into<String>) -> String {
    format!("{}", line.into().green().bold())
}

fn warning_line(line: impl Into<String>) -> String {
    format!("{}", line.into().yellow())
}

fn help_line(line: impl Into<String>) -> String {
    muted_line(line)
}

fn muted_line(line: impl Into<String>) -> String {
    format!("{}", line.into().dark_grey())
}

fn visible_positions(len: usize, cursor: usize, page_size: usize) -> std::ops::Range<usize> {
    if len == 0 {
        return 0..0;
    }
    let start = (cursor / page_size) * page_size;
    let end = (start + page_size).min(len);
    start..end
}

fn render_lines(stdout: &mut impl Write, rendered_lines: usize, lines: &[String]) -> Result<()> {
    clear_prompt(stdout, rendered_lines)?;
    for line in lines {
        write!(stdout, "{line}\r\n")?;
    }
    stdout.flush()?;
    Ok(())
}

fn clear_prompt(stdout: &mut impl Write, rendered_lines: usize) -> Result<()> {
    if rendered_lines > 0 {
        execute!(
            stdout,
            cursor::MoveUp(rendered_lines as u16),
            cursor::MoveToColumn(0),
            Clear(ClearType::FromCursorDown)
        )?;
    }
    Ok(())
}

fn read_prompt_key() -> Result<PromptKey> {
    loop {
        if let Event::Key(event) = event::read()?
            && let Some(key) = prompt_key_from_event(event)
        {
            return Ok(key);
        }
    }
}

fn prompt_key_from_event(event: KeyEvent) -> Option<PromptKey> {
    match event.code {
        KeyCode::Char(' ') => Some(PromptKey::Space),
        KeyCode::Char(character) if is_text_modifier(event.modifiers) => {
            Some(PromptKey::Char(character))
        }
        KeyCode::Backspace => Some(PromptKey::Backspace),
        KeyCode::Enter => Some(PromptKey::Enter),
        KeyCode::Esc => Some(PromptKey::Escape),
        KeyCode::Up => Some(PromptKey::Up),
        KeyCode::Down => Some(PromptKey::Down),
        KeyCode::Tab => Some(PromptKey::Tab),
        KeyCode::BackTab => Some(PromptKey::BackTab),
        _ => None,
    }
}

fn is_text_modifier(modifiers: KeyModifiers) -> bool {
    modifiers.is_empty() || modifiers == KeyModifiers::SHIFT
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

fn format_option_labels(options: &[SelectOption]) -> Vec<String> {
    let base_labels: Vec<String> = options.iter().map(format_option_label).collect();

    base_labels
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let duplicate = base_labels
                .iter()
                .filter(|candidate| *candidate == label)
                .count()
                > 1;
            if duplicate {
                format!("{} ({})", label, options[index].id)
            } else {
                label.clone()
            }
        })
        .collect()
}

fn format_option_label(option: &SelectOption) -> String {
    if option.name.is_empty() || option.name == option.id {
        option.id.clone()
    } else {
        option.name.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_state_uses_simple_j_and_k_navigation() {
        let mut state = SelectPromptState::new(
            vec![
                "Maven".to_string(),
                "Gradle".to_string(),
                "Kotlin".to_string(),
            ],
            0,
        );

        assert_eq!(
            state.apply_key(PromptKey::Char('j')),
            SelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_label(), Some("Gradle"));
        assert_eq!(
            state.apply_key(PromptKey::Char('k')),
            SelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_label(), Some("Maven"));
        assert_eq!(
            state.apply_key(PromptKey::Char('x')),
            SelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_label(), Some("Maven"));
        assert_eq!(
            state.apply_key(PromptKey::Tab),
            SelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_label(), Some("Gradle"));
        assert_eq!(
            state.apply_key(PromptKey::BackTab),
            SelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_label(), Some("Maven"));
    }

    #[test]
    fn select_prompt_rendering_has_no_filter_field() -> anyhow::Result<()> {
        let state = SelectPromptState::new(vec!["Maven".to_string(), "Gradle".to_string()], 0);
        let mut output = Vec::new();

        let rendered = render_select_prompt(&mut output, 0, "Project type?", &state)?;

        let output = String::from_utf8(output)?;
        assert_eq!(rendered, 4);
        assert!(!output.contains("filter:"));
        assert!(output.contains("> Maven"));
        assert!(output.contains("Use arrows or j/k to move. Enter to select."));
        Ok(())
    }

    #[test]
    fn select_prompt_rendering_uses_ansi_visual_hierarchy() -> anyhow::Result<()> {
        let state = SelectPromptState::new(vec!["Maven".to_string(), "Gradle".to_string()], 0);
        let mut output = Vec::new();

        render_select_prompt(&mut output, 0, "Project type?", &state)?;

        let output = String::from_utf8(output)?;
        assert!(
            output.contains("\u{1b}["),
            "select prompts should use ANSI styling for visual hierarchy; got: {output:?}"
        );
        assert!(output.contains("? Project type?"));
        assert!(output.contains("> Maven"));
        assert!(output.contains("  Gradle"));
        Ok(())
    }

    #[test]
    fn multi_select_prompt_rendering_uses_ansi_visual_hierarchy() -> anyhow::Result<()> {
        let ids = vec!["web".to_string(), "data-jpa".to_string()];
        let labels = vec![
            "Spring Web [Web]".to_string(),
            "Spring Data JPA [SQL]".to_string(),
        ];
        let mut state = MultiSelectPromptState::new(ids, labels, &["web".to_string()]);
        state.apply_key(PromptKey::Tab);
        let mut output = Vec::new();

        render_multi_select_prompt(&mut output, 0, "Select dependencies", &state)?;

        let output = String::from_utf8(output)?;
        assert!(
            output.contains("\u{1b}["),
            "multi-select prompts should use ANSI styling for visual hierarchy; got: {output:?}"
        );
        assert!(output.contains("? Select dependencies"));
        assert!(output.contains("> ■ Spring Web [Web]"));
        assert!(output.contains("  □ Spring Data JPA [SQL]"));
        assert!(output.contains("Type to filter."));
        Ok(())
    }

    #[test]
    fn render_lines_use_carriage_returns_for_raw_mode_layout() -> anyhow::Result<()> {
        let mut output = Vec::new();

        render_lines(&mut output, 0, &["first".to_string(), "second".to_string()])?;

        assert_eq!(String::from_utf8(output)?, "first\r\nsecond\r\n");
        Ok(())
    }

    #[test]
    fn format_option_labels_prefer_names_and_disambiguate_duplicates() {
        let id_only = SelectOption {
            id: "java".to_string(),
            name: String::new(),
        };
        assert_eq!(
            format_option_labels(&[id_only]),
            ["java"],
            "id-only metadata should remain selectable"
        );

        let unique = [
            SelectOption {
                id: "java".to_string(),
                name: "Java".to_string(),
            },
            SelectOption {
                id: "kotlin".to_string(),
                name: "Kotlin".to_string(),
            },
        ];
        assert_eq!(format_option_labels(&unique), ["Java", "Kotlin"]);

        let duplicates = [
            SelectOption {
                id: "gradle-project".to_string(),
                name: "Gradle".to_string(),
            },
            SelectOption {
                id: "gradle-project-kotlin".to_string(),
                name: "Gradle".to_string(),
            },
        ];
        assert_eq!(
            format_option_labels(&duplicates),
            ["Gradle (gradle-project)", "Gradle (gradle-project-kotlin)"]
        );
    }
}
