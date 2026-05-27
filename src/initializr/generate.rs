use crate::initializr::metadata::{InitializrMetadata, SelectField};
use anyhow::{Context, Result, bail};
use reqwest::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationParams {
    pub project_type: String,
    pub language: String,
    pub boot_version: String,
    pub base_dir: String,
    pub group_id: String,
    pub artifact_id: String,
    pub name: String,
    pub description: String,
    pub package_name: String,
    pub packaging: String,
    pub java_version: String,
    pub dependencies: Vec<String>,
}

impl GenerationParams {
    pub fn query_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = vec![
            ("type", self.project_type.clone()),
            ("language", self.language.clone()),
            ("bootVersion", self.boot_version.clone()),
            ("baseDir", self.base_dir.clone()),
            ("groupId", self.group_id.clone()),
            ("artifactId", self.artifact_id.clone()),
            ("name", self.name.clone()),
            ("description", self.description.clone()),
            ("packageName", self.package_name.clone()),
            ("packaging", self.packaging.clone()),
            ("javaVersion", self.java_version.clone()),
        ];

        if !self.dependencies.is_empty() {
            pairs.push(("dependencies", self.dependencies.join(",")));
        }

        pairs
    }

    pub fn starter_zip_url(&self, base_url: &Url) -> Result<Url> {
        let mut url = base_url
            .join("starter.zip")
            .with_context(|| format!("failed to build starter.zip URL from {base_url}"))?;

        url.query_pairs_mut().extend_pairs(self.query_pairs());
        Ok(url)
    }

    pub fn validate(&self, metadata: &InitializrMetadata) -> Result<()> {
        validate_artifact_id(&self.artifact_id)?;
        validate_package_name(&self.package_name)?;

        validate_metadata_value("project type", &self.project_type, &metadata.project_type)?;
        validate_metadata_value("language", &self.language, &metadata.language)?;
        validate_metadata_value(
            "Spring Boot version",
            &self.boot_version,
            &metadata.boot_version,
        )?;
        validate_metadata_value("Java version", &self.java_version, &metadata.java_version)?;
        validate_metadata_value("packaging", &self.packaging, &metadata.packaging)?;

        let dependencies = metadata.dependency_entries();
        for dependency in &self.dependencies {
            if !dependencies.iter().any(|entry| entry.id == *dependency) {
                bail!("unsupported dependency '{dependency}'");
            }
        }

        Ok(())
    }
}

fn validate_metadata_value(label: &str, value: &str, field: &SelectField) -> Result<()> {
    if field.values.iter().any(|option| option.id == value) {
        Ok(())
    } else {
        bail!("unsupported {label} '{value}'")
    }
}

fn validate_artifact_id(artifact_id: &str) -> Result<()> {
    if artifact_id.is_empty() {
        bail!("artifact id must not be empty");
    }

    if artifact_id.starts_with(['-', '_']) || artifact_id.ends_with(['-', '_']) {
        bail!("artifact id must not start or end with '-' or '_'");
    }

    if artifact_id
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_')
    {
        Ok(())
    } else {
        bail!("artifact id must contain only ASCII letters, numbers, hyphens, and underscores")
    }
}

fn validate_package_name(package_name: &str) -> Result<()> {
    if package_name.is_empty() {
        bail!("package name must not be empty");
    }

    for segment in package_name.split('.') {
        if !is_valid_package_segment(segment) {
            bail!("package name contains invalid segment '{segment}'");
        }
    }

    Ok(())
}

fn is_valid_package_segment(segment: &str) -> bool {
    let mut chars = segment.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    if is_java_keyword(segment) {
        return false;
    }

    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn is_java_keyword(segment: &str) -> bool {
    matches!(
        segment,
        "abstract"
            | "assert"
            | "boolean"
            | "break"
            | "byte"
            | "case"
            | "catch"
            | "char"
            | "class"
            | "const"
            | "continue"
            | "default"
            | "do"
            | "double"
            | "else"
            | "enum"
            | "extends"
            | "final"
            | "finally"
            | "float"
            | "for"
            | "goto"
            | "if"
            | "implements"
            | "import"
            | "instanceof"
            | "int"
            | "interface"
            | "long"
            | "native"
            | "new"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "return"
            | "short"
            | "static"
            | "strictfp"
            | "super"
            | "switch"
            | "synchronized"
            | "this"
            | "throw"
            | "throws"
            | "transient"
            | "try"
            | "void"
            | "volatile"
            | "while"
            | "true"
            | "false"
            | "null"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initializr::metadata::{
        Dependency, DependencyGroup, DependencyGroupField, InitializrMetadata, SelectField,
        SelectOption,
    };
    use reqwest::Url;

    #[test]
    fn generation_params_build_initializr_query_pairs() {
        let params = GenerationParams {
            project_type: "maven-project".to_string(),
            language: "java".to_string(),
            boot_version: "3.5.0".to_string(),
            base_dir: "orders-api".to_string(),
            group_id: "com.example".to_string(),
            artifact_id: "orders".to_string(),
            name: "orders-api".to_string(),
            description: "Orders API".to_string(),
            package_name: "com.example.orders".to_string(),
            packaging: "jar".to_string(),
            java_version: "21".to_string(),
            dependencies: vec!["web".to_string(), "validation".to_string()],
        };

        assert_eq!(
            params.query_pairs(),
            vec![
                ("type", "maven-project".to_string()),
                ("language", "java".to_string()),
                ("bootVersion", "3.5.0".to_string()),
                ("baseDir", "orders-api".to_string()),
                ("groupId", "com.example".to_string()),
                ("artifactId", "orders".to_string()),
                ("name", "orders-api".to_string()),
                ("description", "Orders API".to_string()),
                ("packageName", "com.example.orders".to_string()),
                ("packaging", "jar".to_string()),
                ("javaVersion", "21".to_string()),
                ("dependencies", "web,validation".to_string()),
            ]
        );
    }

    #[test]
    fn generation_params_omit_empty_dependencies() {
        let params = GenerationParams {
            dependencies: Vec::new(),
            ..sample_params()
        };

        assert!(
            params
                .query_pairs()
                .iter()
                .all(|(name, _)| *name != "dependencies")
        );
    }

    #[test]
    fn generation_params_build_starter_zip_url() -> anyhow::Result<()> {
        let params = sample_params();
        let base_url = Url::parse("https://start.example")?;

        let url = params.starter_zip_url(&base_url)?;

        assert_eq!(url.scheme(), "https");
        assert_eq!(url.host_str(), Some("start.example"));
        assert_eq!(url.path(), "/starter.zip");
        assert!(url.query().unwrap().contains("type=maven-project"));
        assert!(url.query().unwrap().contains("dependencies=web"));

        Ok(())
    }

    #[test]
    fn validates_generation_params_against_metadata() -> anyhow::Result<()> {
        sample_params().validate(&sample_metadata())
    }

    #[test]
    fn rejects_invalid_artifact_ids() {
        let params = GenerationParams {
            artifact_id: "orders api".to_string(),
            ..sample_params()
        };

        let error = params
            .validate(&sample_metadata())
            .expect_err("artifact id with spaces should be invalid");

        assert!(error.to_string().contains("artifact id"));
    }

    #[test]
    fn rejects_invalid_java_package_names() {
        let params = GenerationParams {
            package_name: "com.example.class".to_string(),
            ..sample_params()
        };

        let error = params
            .validate(&sample_metadata())
            .expect_err("package segment using Java keyword should be invalid");

        assert!(error.to_string().contains("package name"));
    }

    #[test]
    fn rejects_values_not_available_in_metadata() {
        let params = GenerationParams {
            java_version: "99".to_string(),
            ..sample_params()
        };

        let error = params
            .validate(&sample_metadata())
            .expect_err("unsupported Java version should fail");

        assert!(error.to_string().contains("unsupported Java version '99'"));
    }

    #[test]
    fn rejects_dependencies_not_available_in_metadata() {
        let params = GenerationParams {
            dependencies: vec!["web".to_string(), "missing".to_string()],
            ..sample_params()
        };

        let error = params
            .validate(&sample_metadata())
            .expect_err("unknown dependency should fail");

        assert!(
            error
                .to_string()
                .contains("unsupported dependency 'missing'")
        );
    }

    fn sample_params() -> GenerationParams {
        GenerationParams {
            project_type: "maven-project".to_string(),
            language: "java".to_string(),
            boot_version: "3.5.0".to_string(),
            base_dir: "orders-api".to_string(),
            group_id: "com.example".to_string(),
            artifact_id: "orders".to_string(),
            name: "orders-api".to_string(),
            description: "Orders API".to_string(),
            package_name: "com.example.orders".to_string(),
            packaging: "jar".to_string(),
            java_version: "21".to_string(),
            dependencies: vec!["web".to_string()],
        }
    }

    fn sample_metadata() -> InitializrMetadata {
        InitializrMetadata {
            project_type: select_field(["maven-project", "gradle-project"]),
            language: select_field(["java", "kotlin"]),
            boot_version: select_field(["3.5.0", "3.4.6"]),
            java_version: select_field(["17", "21"]),
            packaging: select_field(["jar", "war"]),
            dependencies: DependencyGroupField {
                values: vec![DependencyGroup {
                    name: "Web".to_string(),
                    values: vec![
                        Dependency {
                            id: "web".to_string(),
                            name: "Spring Web".to_string(),
                            description: None,
                        },
                        Dependency {
                            id: "validation".to_string(),
                            name: "Validation".to_string(),
                            description: None,
                        },
                    ],
                }],
            },
        }
    }

    fn select_field<const N: usize>(ids: [&str; N]) -> SelectField {
        SelectField {
            default: ids.first().map(|id| (*id).to_string()),
            values: ids
                .into_iter()
                .map(|id| SelectOption {
                    id: id.to_string(),
                    name: id.to_string(),
                })
                .collect(),
        }
    }
}
