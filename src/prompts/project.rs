use crate::{
    cli::InitArgs,
    config::user_config::UserConfig,
    initializr::{
        generate::GenerationParams,
        metadata::{InitializrMetadata, SelectField},
    },
    prompts::ui::Prompter,
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

    pub fn from_prompts(
        args: &InitArgs,
        config: Option<&UserConfig>,
        metadata: &InitializrMetadata,
        prompter: &mut impl Prompter,
    ) -> Result<Self> {
        let project_name = match args.project_name.clone() {
            Some(name) => name,
            None => prompter.text("Project name?", Some("demo"))?,
        };

        let group_id = match args.group_id.clone() {
            Some(group_id) => group_id,
            None => {
                let default = config
                    .and_then(|config| config.group_id.as_deref())
                    .unwrap_or("com.example");
                prompter.text("Group ID?", Some(default))?
            }
        };

        let artifact_id = match args.artifact_id.clone() {
            Some(artifact_id) => artifact_id,
            None => prompter.text("Artifact ID?", Some(&project_name))?,
        };

        let project_type = if let Some(project_type) = args.project_type.clone() {
            project_type
        } else if let Some(build) = args
            .build
            .as_deref()
            .or_else(|| config.and_then(|config| config.build.as_deref()))
        {
            build_to_project_type(build)
        } else {
            let default = metadata.project_type.default.as_deref();
            prompter.select("Project type?", &metadata.project_type.values, default)?
        };

        let language = match args.language.clone() {
            Some(language) => language,
            None => {
                let default = config
                    .and_then(|config| config.language.as_deref())
                    .or(metadata.language.default.as_deref());
                prompter.select("Language?", &metadata.language.values, default)?
            }
        };

        let boot_version = match args.boot_version.clone() {
            Some(boot_version) => boot_version,
            None => {
                let default = metadata.boot_version.default.as_deref();
                prompter.select(
                    "Spring Boot version?",
                    &metadata.boot_version.values,
                    default,
                )?
            }
        };

        let java_version = match args.java_version.clone() {
            Some(java_version) => java_version,
            None => {
                let default = config
                    .and_then(|config| config.java_version.as_deref())
                    .or(metadata.java_version.default.as_deref());
                prompter.select("Java version?", &metadata.java_version.values, default)?
            }
        };

        let packaging = match args.packaging.clone() {
            Some(packaging) => packaging,
            None => {
                let default = config
                    .and_then(|config| config.packaging.as_deref())
                    .or(metadata.packaging.default.as_deref());
                prompter.select("Packaging?", &metadata.packaging.values, default)?
            }
        };

        let description = match args.description.clone() {
            Some(description) => description,
            None => prompter.text("Description?", Some("Demo project for Spring Boot"))?,
        };

        let package_name = match args.package_name.clone() {
            Some(package_name) => package_name,
            None => {
                let default = config
                    .and_then(|config| config.package_name_pattern.as_deref())
                    .map(|pattern| expand_package_pattern(pattern, &group_id, &artifact_id))
                    .unwrap_or_else(|| default_package_name(&group_id, &artifact_id));
                prompter.text("Package name?", Some(&default))?
            }
        };

        let output_dir = match args.output_dir.clone() {
            Some(output_dir) => output_dir,
            None => {
                let default = config
                    .and_then(|config| config.output_dir.as_deref())
                    .unwrap_or(".");
                PathBuf::from(prompter.text("Output directory?", Some(default))?)
            }
        };

        let dependencies = if args.dependencies.is_empty() {
            config
                .map(|config| config.dependencies.clone())
                .unwrap_or_default()
        } else {
            args.dependencies.clone()
        };

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

fn build_to_project_type(build: &str) -> String {
    match build {
        "maven" => "maven-project".to_string(),
        "gradle" => "gradle-project".to_string(),
        other => other.to_string(),
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
        return Ok(build_to_project_type(build));
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

    #[derive(Default)]
    struct StaticPrompter {
        text_answers: Vec<(&'static str, &'static str, String)>,
        select_answers: Vec<(&'static str, &'static str, String)>,
        text_calls: Vec<(String, Option<String>)>,
        select_calls: Vec<(String, Option<String>, Vec<String>)>,
    }

    impl StaticPrompter {
        fn with_text(mut self, message: &'static str, default: &'static str, answer: &str) -> Self {
            self.text_answers
                .push((message, default, answer.to_string()));
            self
        }

        fn with_select(
            mut self,
            message: &'static str,
            default_id: &'static str,
            answer: &str,
        ) -> Self {
            self.select_answers
                .push((message, default_id, answer.to_string()));
            self
        }
    }

    impl Prompter for StaticPrompter {
        fn text(&mut self, message: &str, default: Option<&str>) -> anyhow::Result<String> {
            self.text_calls
                .push((message.to_string(), default.map(str::to_string)));
            let position = self
                .text_answers
                .iter()
                .position(|(prompt, _, _)| *prompt == message)
                .with_context(|| {
                    format!("no scripted text answer for prompt {message:?}, default={default:?}")
                })?;
            let (_, expected_default, answer) = self.text_answers.remove(position);
            assert_eq!(
                default,
                Some(expected_default),
                "unexpected default for prompt {message:?}"
            );
            Ok(answer)
        }

        fn select(
            &mut self,
            message: &str,
            options: &[SelectOption],
            default_id: Option<&str>,
        ) -> anyhow::Result<String> {
            self.select_calls.push((
                message.to_string(),
                default_id.map(str::to_string),
                options.iter().map(|option| option.id.clone()).collect(),
            ));
            let position = self
                .select_answers
                .iter()
                .position(|(prompt, _, _)| *prompt == message)
                .with_context(|| {
                    format!(
                        "no scripted select answer for prompt {message:?}, default={default_id:?}"
                    )
                })?;
            let (_, expected_default, answer) = self.select_answers.remove(position);
            assert_eq!(
                default_id,
                Some(expected_default),
                "unexpected default for prompt {message:?}"
            );
            assert!(
                options.iter().any(|option| option.id == answer),
                "scripted answer {answer:?} is not in option list for {message:?}"
            );
            Ok(answer)
        }
    }

    #[test]
    fn interactive_plan_prompts_only_for_missing_values() -> anyhow::Result<()> {
        let args = InitArgs {
            project_name: Some("orders-api".to_string()),
            artifact_id: Some("orders".to_string()),
            language: Some("kotlin".to_string()),
            packaging: Some("war".to_string()),
            ..empty_init_args()
        };
        let mut prompter = StaticPrompter::default()
            .with_text("Group ID?", "com.example", "com.acme")
            .with_select("Project type?", "maven-project", "maven-project")
            .with_select("Spring Boot version?", "3.5.0", "3.5.0")
            .with_select("Java version?", "17", "21")
            .with_text(
                "Description?",
                "Demo project for Spring Boot",
                "Orders service",
            )
            .with_text("Package name?", "com.acme.orders", "com.acme.orders")
            .with_text("Output directory?", ".", ".");

        let plan = ProjectPlan::from_prompts(&args, None, &sample_metadata(), &mut prompter)?;

        assert_eq!(plan.generation.name, "orders-api");
        assert_eq!(plan.generation.artifact_id, "orders");
        assert_eq!(plan.generation.language, "kotlin");
        assert_eq!(plan.generation.packaging, "war");
        assert_eq!(plan.generation.group_id, "com.acme");
        assert_eq!(plan.generation.project_type, "maven-project");
        assert_eq!(plan.generation.boot_version, "3.5.0");
        assert_eq!(plan.generation.java_version, "21");
        assert_eq!(plan.generation.description, "Orders service");
        assert_eq!(plan.generation.package_name, "com.acme.orders");
        assert_eq!(plan.output_dir, PathBuf::from("."));
        assert!(
            prompter
                .text_calls
                .iter()
                .all(|(message, _)| message != "Project name?" && message != "Artifact ID?"),
            "should not prompt for values provided via flags"
        );
        Ok(())
    }

    #[test]
    fn interactive_plan_prefills_defaults_from_saved_config_then_metadata() -> anyhow::Result<()> {
        let args = empty_init_args();
        let config = UserConfig {
            group_id: Some("com.saved".to_string()),
            language: Some("kotlin".to_string()),
            java_version: Some("21".to_string()),
            packaging: Some("war".to_string()),
            output_dir: Some("projects".to_string()),
            package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
            ..UserConfig::default()
        };
        let mut prompter = StaticPrompter::default()
            .with_text("Project name?", "demo", "demo-api")
            .with_text("Group ID?", "com.saved", "com.saved")
            .with_text("Artifact ID?", "demo-api", "demo-api")
            .with_select("Project type?", "maven-project", "maven-project")
            .with_select("Language?", "kotlin", "kotlin")
            .with_select("Spring Boot version?", "3.5.0", "3.5.0")
            .with_select("Java version?", "21", "21")
            .with_select("Packaging?", "war", "war")
            .with_text("Description?", "Demo project for Spring Boot", "demo")
            .with_text("Package name?", "com.saved.demo_api", "com.saved.demo_api")
            .with_text("Output directory?", "projects", "projects");

        let plan =
            ProjectPlan::from_prompts(&args, Some(&config), &sample_metadata(), &mut prompter)?;

        assert_eq!(plan.generation.base_dir, "demo-api");
        assert_eq!(plan.generation.group_id, "com.saved");
        assert_eq!(plan.generation.language, "kotlin");
        assert_eq!(plan.generation.java_version, "21");
        assert_eq!(plan.generation.packaging, "war");
        assert_eq!(plan.generation.package_name, "com.saved.demo_api");
        assert_eq!(plan.output_dir, PathBuf::from("projects"));
        Ok(())
    }

    #[test]
    fn interactive_plan_uses_build_flag_to_skip_project_type_prompt() -> anyhow::Result<()> {
        let args = InitArgs {
            project_name: Some("demo".to_string()),
            artifact_id: Some("demo".to_string()),
            language: Some("java".to_string()),
            boot_version: Some("3.5.0".to_string()),
            java_version: Some("17".to_string()),
            packaging: Some("jar".to_string()),
            description: Some("d".to_string()),
            package_name: Some("com.example.demo".to_string()),
            group_id: Some("com.example".to_string()),
            output_dir: Some(PathBuf::from(".")),
            build: Some("gradle".to_string()),
            ..empty_init_args()
        };
        let mut prompter = StaticPrompter::default();

        let plan = ProjectPlan::from_prompts(&args, None, &sample_metadata(), &mut prompter)?;

        assert_eq!(plan.generation.project_type, "gradle-project");
        assert!(
            prompter.select_calls.is_empty(),
            "all selects should have been skipped"
        );
        assert!(
            prompter.text_calls.is_empty(),
            "all text prompts should have been skipped"
        );
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
