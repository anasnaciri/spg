use crate::initializr::metadata::SelectOption;
use anyhow::{Context, Result, anyhow};
use inquire::{Select, Text};

pub trait Prompter {
    fn text(&mut self, message: &str, default: Option<&str>) -> Result<String>;

    fn select(
        &mut self,
        message: &str,
        options: &[SelectOption],
        default_id: Option<&str>,
    ) -> Result<String>;
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

        let labels: Vec<String> = options.iter().map(format_option_label).collect();
        let starting_cursor = default_id
            .and_then(|id| options.iter().position(|option| option.id == id))
            .unwrap_or(0);

        let selection = configure_select(Select::new(message, labels.clone()), starting_cursor)
            .prompt()
            .with_context(|| format!("failed to read selection for prompt: {message}"))?;

        let index = labels
            .iter()
            .position(|label| label == &selection)
            .with_context(|| format!("selection did not match any option for prompt: {message}"))?;

        Ok(options[index].id.clone())
    }
}

fn format_option_label(option: &SelectOption) -> String {
    if option.name.is_empty() || option.name == option.id {
        option.id.clone()
    } else {
        format!("{} ({})", option.name, option.id)
    }
}

fn configure_select<T>(select: Select<'_, T>, starting_cursor: usize) -> Select<'_, T>
where
    T: std::fmt::Display,
{
    select
        .with_starting_cursor(starting_cursor)
        .with_vim_mode(true)
        .with_help_message("↑/↓ or k/j to move, Enter to select, Esc to cancel")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configure_select_enables_vim_mode_and_applies_starting_cursor() {
        let select = Select::new("?", vec!["a".to_string(), "b".to_string(), "c".to_string()]);
        let configured = configure_select(select, 2);

        assert!(
            configured.vim_mode,
            "vim_mode should be enabled so j/k navigate the list"
        );
        assert_eq!(configured.starting_cursor, 2);
    }

    #[test]
    fn format_option_label_uses_id_when_name_is_missing_or_matches_id() {
        let id_only = SelectOption {
            id: "java".to_string(),
            name: String::new(),
        };
        assert_eq!(format_option_label(&id_only), "java");

        let same = SelectOption {
            id: "java".to_string(),
            name: "java".to_string(),
        };
        assert_eq!(format_option_label(&same), "java");

        let different = SelectOption {
            id: "maven-project".to_string(),
            name: "Maven Project".to_string(),
        };
        assert_eq!(
            format_option_label(&different),
            "Maven Project (maven-project)"
        );
    }
}
