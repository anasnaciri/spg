use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitializrMetadata {
    #[serde(default, rename = "type")]
    pub project_type: SelectField,

    #[serde(default)]
    pub language: SelectField,

    #[serde(default, rename = "bootVersion")]
    pub boot_version: SelectField,

    #[serde(default, rename = "javaVersion")]
    pub java_version: SelectField,

    #[serde(default)]
    pub packaging: SelectField,

    #[serde(default)]
    pub dependencies: DependencyGroupField,
}

impl InitializrMetadata {
    pub fn dependency_entries(&self) -> Vec<DependencyEntry> {
        self.dependencies
            .values
            .iter()
            .flat_map(|group| {
                group.values.iter().map(|dependency| DependencyEntry {
                    id: dependency.id.clone(),
                    name: if dependency.name.is_empty() {
                        dependency.id.clone()
                    } else {
                        dependency.name.clone()
                    },
                    description: dependency.description.clone().unwrap_or_default(),
                    group: group.name.clone(),
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectField {
    pub default: Option<String>,

    #[serde(default)]
    pub values: Vec<SelectOption>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectOption {
    pub id: String,

    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyGroupField {
    #[serde(default)]
    pub values: Vec<DependencyGroup>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyGroup {
    #[serde(default)]
    pub name: String,

    #[serde(default)]
    pub values: Vec<Dependency>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    pub id: String,

    #[serde(default)]
    pub name: String,

    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub group: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_metadata_choices_and_dependency_groups() -> serde_json::Result<()> {
        let metadata: InitializrMetadata = serde_json::from_str(
            r#"
            {
              "type": {
                "default": "maven-project",
                "values": [
                  { "id": "maven-project", "name": "Maven Project" },
                  { "id": "gradle-project", "name": "Gradle Project" }
                ]
              },
              "language": {
                "default": "java",
                "values": [
                  { "id": "java", "name": "Java" },
                  { "id": "kotlin", "name": "Kotlin" }
                ]
              },
              "bootVersion": {
                "default": "3.5.0",
                "values": [
                  { "id": "3.5.0", "name": "3.5.0" }
                ]
              },
              "javaVersion": {
                "default": "21",
                "values": [
                  { "id": "17", "name": "17" },
                  { "id": "21", "name": "21" }
                ]
              },
              "packaging": {
                "default": "jar",
                "values": [
                  { "id": "jar", "name": "Jar" },
                  { "id": "war", "name": "War" }
                ]
              },
              "dependencies": {
                "values": [
                  {
                    "name": "Web",
                    "values": [
                      {
                        "id": "web",
                        "name": "Spring Web",
                        "description": "Build web applications."
                      }
                    ]
                  },
                  {
                    "name": "SQL",
                    "values": [
                      {
                        "id": "data-jpa",
                        "name": "Spring Data JPA",
                        "description": "Persist data in SQL stores."
                      }
                    ]
                  }
                ]
              }
            }
            "#,
        )?;

        assert_eq!(
            metadata.project_type.default.as_deref(),
            Some("maven-project")
        );
        assert_eq!(metadata.project_type.values[1].id, "gradle-project");
        assert_eq!(metadata.language.default.as_deref(), Some("java"));
        assert_eq!(metadata.boot_version.default.as_deref(), Some("3.5.0"));
        assert_eq!(metadata.java_version.values[1].id, "21");
        assert_eq!(metadata.packaging.values[0].id, "jar");

        let dependencies = metadata.dependency_entries();
        assert_eq!(dependencies.len(), 2);
        assert_eq!(dependencies[0].id, "web");
        assert_eq!(dependencies[0].group, "Web");
        assert_eq!(dependencies[1].id, "data-jpa");
        assert_eq!(dependencies[1].group, "SQL");

        Ok(())
    }
}
