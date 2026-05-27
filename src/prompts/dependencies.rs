use crate::initializr::metadata::{DependencyEntry, InitializrMetadata};

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
