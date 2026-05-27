use crate::{
    initializr::metadata::{DependencyEntry, InitializrMetadata, SelectOption},
    prompts::ui::Prompter,
};
use anyhow::Result;
use std::collections::HashMap;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DependencySelection {
    entries: Vec<DependencyEntry>,
}

impl DependencySelection {
    pub fn add(&mut self, entry: DependencyEntry) -> bool {
        if self.entries.iter().any(|existing| existing.id == entry.id) {
            false
        } else {
            self.entries.push(entry);
            true
        }
    }

    pub fn ids(&self) -> Vec<String> {
        self.entries.iter().map(|entry| entry.id.clone()).collect()
    }

    pub fn entries(&self) -> &[DependencyEntry] {
        &self.entries
    }
}

pub fn search_dependencies(
    metadata: &InitializrMetadata,
    query: impl AsRef<str>,
) -> Vec<DependencyEntry> {
    let query = normalize_query(query.as_ref());
    let dependencies = metadata.dependency_entries();

    if query.is_empty() {
        return dependencies;
    }

    dependencies
        .into_iter()
        .filter(|dependency| dependency_matches(dependency, &query))
        .collect()
}

fn dependency_matches(dependency: &DependencyEntry, query: &str) -> bool {
    let searchable = [
        dependency.id.as_str(),
        dependency.name.as_str(),
        dependency.description.as_str(),
        dependency.group.as_str(),
    ]
    .join(" ")
    .to_ascii_lowercase();

    query
        .split_whitespace()
        .all(|term| searchable.contains(term))
}

fn normalize_query(query: &str) -> String {
    query.trim().to_ascii_lowercase()
}

pub fn pick_dependencies_interactively(
    metadata: &InitializrMetadata,
    seed: &[String],
    prompter: &mut impl Prompter,
) -> Result<Vec<String>> {
    let entries = metadata.dependency_entries();

    let mut selection = DependencySelection::default();
    let by_id: HashMap<String, DependencyEntry> = entries
        .iter()
        .map(|entry| (entry.id.clone(), entry.clone()))
        .collect();

    for id in seed {
        if let Some(entry) = by_id.get(id) {
            selection.add(entry.clone());
        }
    }

    if entries.is_empty() {
        return Ok(selection.ids());
    }

    if !prompter.confirm("Add dependencies?", true)? {
        return Ok(Vec::new());
    }

    let options: Vec<SelectOption> = entries.iter().map(dependency_option).collect();
    let selected_ids = prompter.multi_select("Select dependencies", &options, &selection.ids())?;
    let mut selected = DependencySelection::default();
    for id in selected_ids {
        if let Some(entry) = by_id.get(&id) {
            selected.add(entry.clone());
        }
    }

    Ok(selected.ids())
}

fn dependency_option(entry: &DependencyEntry) -> SelectOption {
    let label = if entry.description.is_empty() {
        format!("{} [{}]", entry.name, entry.group)
    } else {
        format!("{} [{}] - {}", entry.name, entry.group, entry.description)
    };
    SelectOption {
        id: entry.id.clone(),
        name: label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initializr::metadata::{
        Dependency, DependencyGroup, DependencyGroupField, InitializrMetadata,
    };
    use crate::prompts::ui::{
        MultiSelectFocus, MultiSelectPromptOutcome, MultiSelectPromptState, PromptKey,
    };

    #[test]
    fn searches_dependencies_by_id_name_description_and_group() {
        let metadata = sample_metadata();

        assert_eq!(ids(search_dependencies(&metadata, "web")), ["web"]);
        assert_eq!(ids(search_dependencies(&metadata, "jpa")), ["data-jpa"]);
        assert_eq!(
            ids(search_dependencies(&metadata, "restful applications")),
            ["web"]
        );
        assert_eq!(
            ids(search_dependencies(&metadata, "developer tools")),
            ["devtools"]
        );
    }

    #[test]
    fn dependency_search_is_case_insensitive_and_trims_query() {
        let metadata = sample_metadata();

        assert_eq!(
            ids(search_dependencies(&metadata, "  SPRING wEb  ")),
            ["web"]
        );
    }

    #[test]
    fn empty_dependency_search_returns_all_dependencies_in_metadata_order() {
        let metadata = sample_metadata();

        assert_eq!(
            ids(search_dependencies(&metadata, " ")),
            ["web", "data-jpa", "devtools"]
        );
    }

    #[test]
    fn selected_dependencies_preserve_order_and_ignore_duplicates() {
        let metadata = sample_metadata();
        let entries = metadata.dependency_entries();
        let mut selection = DependencySelection::default();

        assert!(selection.add(entries[0].clone()));
        assert!(selection.add(entries[1].clone()));
        assert!(!selection.add(entries[0].clone()));

        assert_eq!(selection.ids(), ["web", "data-jpa"]);
        assert_eq!(
            selection.entries(),
            &[entries[0].clone(), entries[1].clone()]
        );
    }

    fn ids(entries: Vec<DependencyEntry>) -> Vec<String> {
        entries.into_iter().map(|entry| entry.id).collect()
    }

    #[derive(Default)]
    struct ScriptedPrompter {
        text_responses: std::collections::VecDeque<String>,
        select_responses: std::collections::VecDeque<String>,
        confirm_responses: std::collections::VecDeque<bool>,
        multi_select_responses: std::collections::VecDeque<Vec<String>>,
        text_prompts: Vec<String>,
        select_prompts: Vec<String>,
        confirm_prompts: Vec<(String, bool)>,
        multi_select_prompts: Vec<String>,
        select_option_ids: Vec<Vec<String>>,
        multi_select_option_ids: Vec<Vec<String>>,
        multi_select_default_ids: Vec<Vec<String>>,
        select_defaults: Vec<Option<String>>,
    }

    impl Prompter for ScriptedPrompter {
        fn text(&mut self, message: &str, _default: Option<&str>) -> anyhow::Result<String> {
            self.text_prompts.push(message.to_string());
            self.text_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no text response scripted for: {message}"))
        }

        fn select(
            &mut self,
            message: &str,
            options: &[crate::initializr::metadata::SelectOption],
            default_id: Option<&str>,
        ) -> anyhow::Result<String> {
            self.select_prompts.push(message.to_string());
            self.select_option_ids
                .push(options.iter().map(|option| option.id.clone()).collect());
            self.select_defaults
                .push(default_id.map(ToString::to_string));
            self.select_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no select response scripted for: {message}"))
        }

        fn confirm(&mut self, message: &str, default: bool) -> anyhow::Result<bool> {
            self.confirm_prompts.push((message.to_string(), default));
            self.confirm_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no confirmation response scripted for: {message}"))
        }

        fn multi_select(
            &mut self,
            message: &str,
            options: &[crate::initializr::metadata::SelectOption],
            default_ids: &[String],
        ) -> anyhow::Result<Vec<String>> {
            self.multi_select_prompts.push(message.to_string());
            self.multi_select_option_ids
                .push(options.iter().map(|option| option.id.clone()).collect());
            self.multi_select_default_ids.push(default_ids.to_vec());
            self.multi_select_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no multi-select response scripted for: {message}"))
        }
    }

    #[test]
    fn dependency_picker_asks_before_opening_multi_select() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            confirm_responses: [true].into_iter().collect(),
            multi_select_responses: [vec!["web".to_string(), "data-jpa".to_string()]]
                .into_iter()
                .collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(&metadata, &[], &mut prompter)?;

        assert_eq!(selected, ["web", "data-jpa"]);
        assert_eq!(
            prompter.confirm_prompts,
            [("Add dependencies?".to_string(), true)]
        );
        assert_eq!(prompter.multi_select_prompts, ["Select dependencies"]);
        assert_eq!(
            prompter.multi_select_option_ids[0],
            ["web", "data-jpa", "devtools"]
        );
        Ok(())
    }

    #[test]
    fn dependency_picker_skips_dependencies_when_declined() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            confirm_responses: [false].into_iter().collect(),
            ..ScriptedPrompter::default()
        };

        let selected =
            pick_dependencies_interactively(&metadata, &["web".to_string()], &mut prompter)?;

        assert!(selected.is_empty());
        assert!(prompter.multi_select_prompts.is_empty());
        Ok(())
    }

    #[test]
    fn dependency_picker_seeds_from_saved_config_and_drops_unknown_ids() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            confirm_responses: [true].into_iter().collect(),
            multi_select_responses: [vec!["web".to_string(), "data-jpa".to_string()]]
                .into_iter()
                .collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(
            &metadata,
            &["web".to_string(), "stale".to_string()],
            &mut prompter,
        )?;

        assert_eq!(selected, ["web", "data-jpa"]);
        assert_eq!(prompter.multi_select_default_ids[0], ["web"]);
        Ok(())
    }

    #[test]
    fn dependency_picker_filters_unknown_multi_select_results() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            confirm_responses: [true].into_iter().collect(),
            multi_select_responses: [vec!["web".to_string(), "stale".to_string()]]
                .into_iter()
                .collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(&metadata, &[], &mut prompter)?;

        assert_eq!(selected, ["web"]);
        Ok(())
    }

    #[test]
    fn dependency_picker_state_keeps_j_and_k_as_search_text_until_list_is_focused() {
        let (ids, labels) = dependency_prompt_options(&sample_metadata());
        let mut state = MultiSelectPromptState::new(ids, labels, &[]);

        assert_eq!(state.focus(), MultiSelectFocus::Search);
        assert_eq!(
            state.apply_key(PromptKey::Char('j')),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.query(), "j");
        assert_eq!(
            state.apply_key(PromptKey::Backspace),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(
            state.apply_key(PromptKey::Char('k')),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.query(), "k");

        assert_eq!(
            state.apply_key(PromptKey::Backspace),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(
            state.apply_key(PromptKey::Tab),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.focus(), MultiSelectFocus::List);
        assert_eq!(
            state.apply_key(PromptKey::Char('j')),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.query(), "");
    }

    #[test]
    fn dependency_picker_state_supports_tab_shift_tab_space_and_escape_to_search() {
        let metadata = sample_metadata();
        let (ids, labels) = dependency_prompt_options(&metadata);
        let mut state = MultiSelectPromptState::new(ids, labels, &[]);

        assert_eq!(
            state.apply_key(PromptKey::Tab),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.focus(), MultiSelectFocus::List);
        assert_eq!(state.cursor_id(), Some("web"));
        assert_eq!(
            state.apply_key(PromptKey::Tab),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_id(), Some("data-jpa"));
        assert_eq!(
            state.apply_key(PromptKey::BackTab),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.cursor_id(), Some("web"));
        assert_eq!(
            state.apply_key(PromptKey::Space),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.selected_ids(), ["web"]);
        assert_eq!(
            state.rendered_options()[0],
            "■ Spring Web [Web] - Build web, including RESTful, applications using Spring MVC."
        );
        assert!(state.rendered_options()[1].starts_with("□ Spring Data JPA"));

        assert_eq!(
            state.apply_key(PromptKey::Escape),
            MultiSelectPromptOutcome::Continue
        );
        assert_eq!(state.focus(), MultiSelectFocus::Search);
        assert_eq!(state.selected_ids(), ["web"]);
    }

    fn dependency_prompt_options(metadata: &InitializrMetadata) -> (Vec<String>, Vec<String>) {
        let options: Vec<SelectOption> = metadata
            .dependency_entries()
            .iter()
            .map(dependency_option)
            .collect();
        let ids = options.iter().map(|option| option.id.clone()).collect();
        let labels = options.iter().map(|option| option.name.clone()).collect();
        (ids, labels)
    }

    #[test]
    fn dependency_picker_exits_immediately_when_metadata_has_no_dependencies() -> anyhow::Result<()>
    {
        let metadata = InitializrMetadata::default();
        let mut prompter = ScriptedPrompter::default();

        let selected = pick_dependencies_interactively(&metadata, &[], &mut prompter)?;

        assert!(selected.is_empty());
        assert!(
            prompter.text_prompts.is_empty(),
            "no metadata means no prompts"
        );
        Ok(())
    }

    fn sample_metadata() -> InitializrMetadata {
        InitializrMetadata {
            dependencies: DependencyGroupField {
                values: vec![
                    DependencyGroup {
                        name: "Web".to_string(),
                        values: vec![Dependency {
                            id: "web".to_string(),
                            name: "Spring Web".to_string(),
                            description: Some(
                                "Build web, including RESTful, applications using Spring MVC."
                                    .to_string(),
                            ),
                        }],
                    },
                    DependencyGroup {
                        name: "SQL".to_string(),
                        values: vec![Dependency {
                            id: "data-jpa".to_string(),
                            name: "Spring Data JPA".to_string(),
                            description: Some("Persist data in SQL stores with JPA.".to_string()),
                        }],
                    },
                    DependencyGroup {
                        name: "Developer Tools".to_string(),
                        values: vec![Dependency {
                            id: "devtools".to_string(),
                            name: "Spring Boot DevTools".to_string(),
                            description: Some("Fast application restarts.".to_string()),
                        }],
                    },
                ],
            },
            ..InitializrMetadata::default()
        }
    }
}
