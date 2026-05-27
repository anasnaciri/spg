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

    fn contains(&self, id: &str) -> bool {
        self.entries.iter().any(|entry| entry.id == id)
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

    let mut last_no_match: Option<String> = None;
    loop {
        let prompt = format!(
            "Search dependencies (blank to finish; {})",
            render_picker_status(&selection, last_no_match.as_deref())
        );
        let query = prompter.text(&prompt, Some(""))?;
        let trimmed = query.trim();
        if trimmed.is_empty() {
            break;
        }

        let matches = search_dependencies(metadata, trimmed);
        if matches.is_empty() {
            last_no_match = Some(trimmed.to_string());
            continue;
        }
        last_no_match = None;

        let options: Vec<SelectOption> = matches.iter().map(dependency_option).collect();
        let default_id = dependency_select_default(&matches, &selection);
        let chosen_id =
            prompter.select("Add which dependency?", &options, default_id.as_deref())?;
        if let Some(entry) = by_id.get(&chosen_id) {
            selection.add(entry.clone());
        }
    }

    Ok(selection.ids())
}

fn dependency_select_default(
    matches: &[DependencyEntry],
    selection: &DependencySelection,
) -> Option<String> {
    matches
        .iter()
        .find(|entry| !selection.contains(&entry.id))
        .or_else(|| matches.first())
        .map(|entry| entry.id.clone())
}

fn render_picker_status(selection: &DependencySelection, last_no_match: Option<&str>) -> String {
    let selected = if selection.entries().is_empty() {
        "no dependencies selected".to_string()
    } else {
        format!("selected: {}", selection.ids().join(", "))
    };
    match last_no_match {
        Some(query) => format!("no matches for '{query}'; {selected}"),
        None => selected,
    }
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
        text_prompts: Vec<String>,
        select_prompts: Vec<String>,
        select_option_ids: Vec<Vec<String>>,
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
    }

    #[test]
    fn dependency_picker_loops_until_user_enters_blank_query() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            text_responses: ["web", "jpa", ""].into_iter().map(String::from).collect(),
            select_responses: ["web", "data-jpa"].into_iter().map(String::from).collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(&metadata, &[], &mut prompter)?;

        assert_eq!(selected, ["web", "data-jpa"]);
        assert_eq!(prompter.text_prompts.len(), 3);
        assert!(prompter.text_prompts[0].contains("no dependencies selected"));
        assert!(prompter.text_prompts[1].contains("selected: web"));
        assert_eq!(prompter.select_option_ids[1], ["data-jpa"]);
        Ok(())
    }

    #[test]
    fn dependency_picker_defaults_select_cursor_to_first_unselected_match() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            text_responses: ["spring", ""].into_iter().map(String::from).collect(),
            select_responses: ["data-jpa"].into_iter().map(String::from).collect(),
            ..ScriptedPrompter::default()
        };

        let selected =
            pick_dependencies_interactively(&metadata, &["web".to_string()], &mut prompter)?;

        assert_eq!(selected, ["web", "data-jpa"]);
        assert_eq!(
            prompter.select_option_ids[0],
            ["web", "data-jpa", "devtools"]
        );
        assert_eq!(prompter.select_defaults[0], Some("data-jpa".to_string()));
        Ok(())
    }

    #[test]
    fn dependency_picker_seeds_from_saved_config_and_drops_unknown_ids() -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            text_responses: [""].into_iter().map(String::from).collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(
            &metadata,
            &["web".to_string(), "stale".to_string()],
            &mut prompter,
        )?;

        assert_eq!(selected, ["web"]);
        assert!(prompter.text_prompts[0].contains("selected: web"));
        Ok(())
    }

    #[test]
    fn dependency_picker_reports_no_matches_in_next_prompt_and_ignores_duplicates()
    -> anyhow::Result<()> {
        let metadata = sample_metadata();
        let mut prompter = ScriptedPrompter {
            text_responses: ["nothing", "web", "web", ""]
                .into_iter()
                .map(String::from)
                .collect(),
            select_responses: ["web", "web"].into_iter().map(String::from).collect(),
            ..ScriptedPrompter::default()
        };

        let selected = pick_dependencies_interactively(&metadata, &[], &mut prompter)?;

        assert_eq!(selected, ["web"], "duplicate selection must be ignored");
        assert!(
            prompter.text_prompts[1].contains("no matches for 'nothing'"),
            "next prompt should surface the missed query"
        );
        Ok(())
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
