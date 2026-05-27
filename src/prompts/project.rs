use crate::{
    cli::InitArgs,
    config::user_config::UserConfig,
    initializr::{
        generate::GenerationParams,
        metadata::{InitializrMetadata, SelectField},
    },
};
use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectPlan {
    pub generation: GenerationParams,
    pub output_dir: PathBuf,
}

impl ProjectPlan {
    pub fn from_defaults(
        args: &InitArgs,
        config: Option<&UserConfig>,
        metadata: &InitializrMetadata,
    ) -> Result<Self> {
        let project_name = args
            .project_name
            .clone()
            .unwrap_or_else(|| "demo".to_string());
        let artifact_id = args
            .artifact_id
            .clone()
            .unwrap_or_else(|| project_name.clone());
        let group_id = choose(
            args.group_id.as_ref(),
            config.and_then(|config| config.group_id.as_ref()),
        )
        .cloned()
        .unwrap_or_else(|| "com.example".to_string());
        let project_type = choose_project_type(args, config, metadata)?;
        let language = choose(
            args.language.as_ref(),
            config.and_then(|config| config.language.as_ref()),
        )
        .cloned()
        .or_else(|| metadata_default(&metadata.language))
        .context("missing language default in Spring Initializr metadata")?;
        let boot_version = args
            .boot_version
            .clone()
            .or_else(|| metadata_default(&metadata.boot_version))
            .context("missing Spring Boot version default in Spring Initializr metadata")?;
        let packaging = choose(
            args.packaging.as_ref(),
            config.and_then(|config| config.packaging.as_ref()),
        )
        .cloned()
        .or_else(|| metadata_default(&metadata.packaging))
        .context("missing packaging default in Spring Initializr metadata")?;
        let java_version = choose(
            args.java_version.as_ref(),
            config.and_then(|config| config.java_version.as_ref()),
        )
        .cloned()
        .or_else(|| metadata_default(&metadata.java_version))
        .context("missing Java version default in Spring Initializr metadata")?;
        let description = args
            .description
            .clone()
            .unwrap_or_else(|| "Demo project for Spring Boot".to_string());
        let package_name = args.package_name.clone().unwrap_or_else(|| {
            config
                .and_then(|config| config.package_name_pattern.as_deref())
                .map(|pattern| expand_package_pattern(pattern, &group_id, &artifact_id))
                .unwrap_or_else(|| default_package_name(&group_id, &artifact_id))
        });
        let dependencies = if args.dependencies.is_empty() {
            config
                .map(|config| config.dependencies.clone())
                .unwrap_or_default()
        } else {
            args.dependencies.clone()
        };
        let output_dir = args
            .output_dir
            .clone()
            .or_else(|| {
                config
                    .and_then(|config| config.output_dir.as_ref())
                    .map(PathBuf::from)
            })
            .unwrap_or_else(|| PathBuf::from("."));

        Ok(Self {
            generation: GenerationParams {
                project_type,
                language,
                boot_version,
                base_dir: project_name.clone(),
                group_id,
                artifact_id,
                name: project_name,
                description,
                package_name,
                packaging,
                java_version,
                dependencies,
            },
            output_dir,
        })
    }
}

fn choose<'a>(flag: Option<&'a String>, config: Option<&'a String>) -> Option<&'a String> {
    flag.or(config)
}

fn choose_project_type(
    args: &InitArgs,
    config: Option<&UserConfig>,
    metadata: &InitializrMetadata,
) -> Result<String> {
    if let Some(project_type) = &args.project_type {
        return Ok(project_type.clone());
    }

    if let Some(build) = args
        .build
        .as_deref()
        .or_else(|| config.and_then(|config| config.build.as_deref()))
    {
        return match build {
            "maven" => Ok("maven-project".to_string()),
            "gradle" => Ok("gradle-project".to_string()),
            other => Ok(other.to_string()),
        };
    }

    metadata_default(&metadata.project_type)
        .context("missing project type default in Spring Initializr metadata")
}

fn metadata_default(field: &SelectField) -> Option<String> {
    field
        .default
        .clone()
        .or_else(|| field.values.first().map(|option| option.id.clone()))
}

fn expand_package_pattern(pattern: &str, group_id: &str, artifact_id: &str) -> String {
    pattern
        .replace("{group_id}", group_id)
        .replace("{artifact_id}", &package_safe_artifact_id(artifact_id))
}

fn default_package_name(group_id: &str, artifact_id: &str) -> String {
    format!("{group_id}.{}", package_safe_artifact_id(artifact_id))
}

fn package_safe_artifact_id(artifact_id: &str) -> String {
    artifact_id.replace('-', "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::InitArgs,
        config::user_config::UserConfig,
        initializr::metadata::{InitializrMetadata, SelectField, SelectOption},
    };
    use std::path::PathBuf;

    #[test]
    fn default_plan_prefers_flags_over_saved_config_and_metadata() -> anyhow::Result<()> {
        let args = InitArgs {
            project_name: Some("orders-api".to_string()),
            defaults: true,
            refresh: false,
            group_id: Some("com.acme".to_string()),
            artifact_id: Some("orders".to_string()),
            description: Some("Orders service".to_string()),
            package_name: Some("com.acme.orders".to_string()),
            build: Some("maven".to_string()),
            project_type: None,
            language: Some("kotlin".to_string()),
            boot_version: Some("3.4.6".to_string()),
            java_version: Some("21".to_string()),
            packaging: Some("war".to_string()),
            dependencies: vec!["web".to_string()],
            output_dir: Some(PathBuf::from("target")),
        };
        let config = UserConfig {
            group_id: Some("com.config".to_string()),
            language: Some("java".to_string()),
            build: Some("gradle".to_string()),
            packaging: Some("jar".to_string()),
            java_version: Some("17".to_string()),
            dependencies: vec!["data-jpa".to_string()],
            package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
            output_dir: Some("projects".to_string()),
        };

        let plan = ProjectPlan::from_defaults(&args, Some(&config), &sample_metadata())?;

        assert_eq!(plan.output_dir, PathBuf::from("target"));
        assert_eq!(plan.generation.project_type, "maven-project");
        assert_eq!(plan.generation.language, "kotlin");
        assert_eq!(plan.generation.boot_version, "3.4.6");
        assert_eq!(plan.generation.group_id, "com.acme");
        assert_eq!(plan.generation.artifact_id, "orders");
        assert_eq!(plan.generation.description, "Orders service");
        assert_eq!(plan.generation.package_name, "com.acme.orders");
        assert_eq!(plan.generation.packaging, "war");
        assert_eq!(plan.generation.java_version, "21");
        assert_eq!(plan.generation.dependencies, ["web"]);

        Ok(())
    }

    #[test]
    fn default_plan_uses_saved_config_and_metadata_defaults_for_missing_values()
    -> anyhow::Result<()> {
        let args = InitArgs {
            project_name: Some("orders-api".to_string()),
            defaults: true,
            refresh: false,
            group_id: None,
            artifact_id: None,
            description: None,
            package_name: None,
            build: None,
            project_type: None,
            language: None,
            boot_version: None,
            java_version: None,
            packaging: None,
            dependencies: Vec::new(),
            output_dir: None,
        };
        let config = UserConfig {
            group_id: Some("com.example".to_string()),
            build: Some("gradle".to_string()),
            java_version: Some("21".to_string()),
            dependencies: vec!["web".to_string(), "validation".to_string()],
            package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
            output_dir: Some("projects".to_string()),
            ..UserConfig::default()
        };

        let plan = ProjectPlan::from_defaults(&args, Some(&config), &sample_metadata())?;

        assert_eq!(plan.output_dir, PathBuf::from("projects"));
        assert_eq!(plan.generation.project_type, "gradle-project");
        assert_eq!(plan.generation.language, "java");
        assert_eq!(plan.generation.boot_version, "3.5.0");
        assert_eq!(plan.generation.base_dir, "orders-api");
        assert_eq!(plan.generation.name, "orders-api");
        assert_eq!(plan.generation.artifact_id, "orders-api");
        assert_eq!(plan.generation.package_name, "com.example.orders_api");
        assert_eq!(plan.generation.packaging, "jar");
        assert_eq!(plan.generation.java_version, "21");
        assert_eq!(plan.generation.dependencies, ["web", "validation"]);

        Ok(())
    }

    #[test]
    fn project_type_flag_wins_over_build_defaults() -> anyhow::Result<()> {
        let args = InitArgs {
            project_name: Some("demo".to_string()),
            defaults: true,
            refresh: false,
            project_type: Some("maven-project".to_string()),
            build: Some("gradle".to_string()),
            ..empty_init_args()
        };

        let plan = ProjectPlan::from_defaults(&args, None, &sample_metadata())?;

        assert_eq!(plan.generation.project_type, "maven-project");

        Ok(())
    }

    fn empty_init_args() -> InitArgs {
        InitArgs {
            project_name: None,
            defaults: false,
            refresh: false,
            group_id: None,
            artifact_id: None,
            description: None,
            package_name: None,
            build: None,
            project_type: None,
            language: None,
            boot_version: None,
            java_version: None,
            packaging: None,
            dependencies: Vec::new(),
            output_dir: None,
        }
    }

    fn sample_metadata() -> InitializrMetadata {
        InitializrMetadata {
            project_type: select_field("maven-project", ["maven-project", "gradle-project"]),
            language: select_field("java", ["java", "kotlin"]),
            boot_version: select_field("3.5.0", ["3.5.0", "3.4.6"]),
            java_version: select_field("17", ["17", "21"]),
            packaging: select_field("jar", ["jar", "war"]),
            ..InitializrMetadata::default()
        }
    }

    fn select_field<const N: usize>(default: &str, ids: [&str; N]) -> SelectField {
        SelectField {
            default: Some(default.to_string()),
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
