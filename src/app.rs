use crate::{
    archive::unzip::extract_zip_archive,
    cli::{CacheCommand, Cli, Commands, ConfigCommand, InitArgs},
    config::{cache, paths::AppPaths, user_config, user_config::UserConfig},
    initializr::{
        client::InitializrClient,
        generate::GenerationParams,
        metadata::{InitializrMetadata, SelectOption},
    },
    prompts::{
        dependencies,
        project::ProjectPlan,
        ui::{InquirePrompter, Prompter},
    },
};
use anyhow::{Context, Result, bail};
use inquire::Confirm;
use std::{
    fs,
    future::Future,
    io::{self, Cursor, Write},
    path::{Path, PathBuf},
    pin::Pin,
    time::SystemTime,
};

pub trait Confirmation {
    fn confirm(&mut self, message: &str) -> Result<bool>;
}

pub trait MetadataProvider {
    fn fetch_metadata<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<InitializrMetadata>> + 'a>>;
}

pub trait StarterZipProvider {
    fn download_starter_zip<'a>(
        &'a mut self,
        params: &'a GenerationParams,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + 'a>>;
}

pub struct InquireConfirmation;

pub struct CachedMetadataProvider<'a, P> {
    cache_file: PathBuf,
    upstream: &'a mut P,
    now: SystemTime,
    ttl: std::time::Duration,
}

impl<'a, P> CachedMetadataProvider<'a, P> {
    pub fn new(cache_file: PathBuf, upstream: &'a mut P) -> Self {
        Self {
            cache_file,
            upstream,
            now: SystemTime::now(),
            ttl: cache::METADATA_CACHE_TTL,
        }
    }

    #[cfg(test)]
    fn with_settings(
        cache_file: PathBuf,
        upstream: &'a mut P,
        now: SystemTime,
        ttl: std::time::Duration,
    ) -> Self {
        Self {
            cache_file,
            upstream,
            now,
            ttl,
        }
    }
}

impl Confirmation for InquireConfirmation {
    fn confirm(&mut self, message: &str) -> Result<bool> {
        Confirm::new(message)
            .with_default(false)
            .prompt()
            .context("failed to read confirmation prompt")
    }
}

impl MetadataProvider for InitializrClient {
    fn fetch_metadata<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<InitializrMetadata>> + 'a>> {
        Box::pin(async move { InitializrClient::fetch_metadata(self).await })
    }
}

impl StarterZipProvider for InitializrClient {
    fn download_starter_zip<'a>(
        &'a mut self,
        params: &'a GenerationParams,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>>> + 'a>> {
        Box::pin(async move { InitializrClient::download_starter_zip(self, params).await })
    }
}

impl<P> MetadataProvider for CachedMetadataProvider<'_, P>
where
    P: MetadataProvider,
{
    fn fetch_metadata<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<InitializrMetadata>> + 'a>> {
        Box::pin(async move {
            if let Some(metadata) =
                cache::load_fresh_metadata_cache(&self.cache_file, self.now, self.ttl)?
            {
                return Ok(metadata);
            }

            let metadata = self.upstream.fetch_metadata().await?;
            cache::save_metadata_cache(&self.cache_file, &metadata, self.now)?;
            Ok(metadata)
        })
    }
}

pub async fn run(cli: Cli) -> Result<()> {
    let paths = AppPaths::discover()?;
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = stdout.lock();
    let mut stderr = stderr.lock();
    let mut confirmation = InquireConfirmation;
    let mut prompter = InquirePrompter;

    run_with_paths(
        cli,
        &paths,
        &mut stdout,
        &mut stderr,
        &mut confirmation,
        &mut prompter,
    )
    .await
}

pub async fn run_with_paths(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
    prompter: &mut impl Prompter,
) -> Result<()> {
    let mut metadata_client = InitializrClient::new_default()?;
    let mut starter_zip_client = metadata_client.clone();
    let mut metadata_provider =
        CachedMetadataProvider::new(paths.metadata_cache_file.clone(), &mut metadata_client);
    run_with_services(
        cli,
        paths,
        stdout,
        stderr,
        confirmation,
        prompter,
        &mut metadata_provider,
        &mut starter_zip_client,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn run_with_services(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
    prompter: &mut impl Prompter,
    metadata_provider: &mut impl MetadataProvider,
    starter_zip_provider: &mut impl StarterZipProvider,
) -> Result<()> {
    match cli.command {
        Commands::Init(args) => {
            run_init(
                *args,
                paths,
                stdout,
                confirmation,
                prompter,
                metadata_provider,
                starter_zip_provider,
            )
            .await?;
        }
        Commands::Deps(args) => {
            let metadata = metadata_provider.fetch_metadata().await?;
            print_dependencies(&metadata, args.query.as_deref(), stdout)?;
        }
        Commands::Config(ConfigCommand::Show) => {
            show_config(paths, stdout)?;
        }
        Commands::Config(ConfigCommand::Reset) => {
            reset_config(paths, stdout, confirmation)?;
        }
        Commands::Cache(CacheCommand::Clear) => {
            clear_cache(paths, stdout)?;
        }
    }

    let _ = stderr;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SavedConfigStrategy {
    Reuse,
    Edit,
    Fresh,
}

#[allow(clippy::too_many_arguments)]
async fn run_init(
    args: InitArgs,
    paths: &AppPaths,
    stdout: &mut impl Write,
    confirmation: &mut impl Confirmation,
    prompter: &mut impl Prompter,
    metadata_provider: &mut impl MetadataProvider,
    starter_zip_provider: &mut impl StarterZipProvider,
) -> Result<()> {
    if args.refresh {
        cache::clear_metadata_cache(&paths.metadata_cache_file)?;
    }

    let saved_config = user_config::load(&paths.user_config_file)?;
    let metadata = metadata_provider.fetch_metadata().await?;
    let (plan, offer_save) = build_plan(&args, saved_config.as_ref(), &metadata, prompter)?;
    plan.generation.validate(&metadata)?;

    prepare_output_dir(&plan.output_dir)?;

    let project_dir = plan.output_dir.join(&plan.generation.base_dir);
    if project_dir.exists() {
        if args.defaults {
            bail!(
                "{} already exists. Remove it or pass --output-dir before retrying.",
                project_dir.display()
            );
        }
        if !confirmation.confirm(&format!(
            "{} already exists. Overwrite?",
            project_dir.display()
        ))? {
            bail!(
                "aborted: existing project at {} was not overwritten",
                project_dir.display()
            );
        }
    }

    let archive = starter_zip_provider
        .download_starter_zip(&plan.generation)
        .await?;
    extract_zip_archive(Cursor::new(archive), &plan.output_dir)?;

    print_success(stdout, &plan, &project_dir)?;

    if offer_save && confirmation.confirm("Save these choices as your spg defaults?")? {
        let new_config = plan.to_user_config();
        user_config::save(&paths.user_config_file, &new_config)?;
        writeln!(
            stdout,
            "Saved spg defaults to {}",
            paths.user_config_file.display()
        )?;
    }

    Ok(())
}

fn build_plan(
    args: &InitArgs,
    saved_config: Option<&UserConfig>,
    metadata: &InitializrMetadata,
    prompter: &mut impl Prompter,
) -> Result<(ProjectPlan, bool)> {
    if args.defaults {
        let plan = ProjectPlan::from_defaults(args, saved_config, metadata)?;
        return Ok((plan, false));
    }

    if let Some(config) = saved_config {
        match prompt_for_saved_config_strategy(prompter)? {
            SavedConfigStrategy::Reuse => {
                let plan = ProjectPlan::from_defaults(args, Some(config), metadata)?;
                Ok((plan, false))
            }
            SavedConfigStrategy::Edit => {
                let plan = ProjectPlan::from_prompts(args, Some(config), metadata, prompter)?;
                Ok((plan, true))
            }
            SavedConfigStrategy::Fresh => {
                let plan = ProjectPlan::from_prompts(args, None, metadata, prompter)?;
                Ok((plan, true))
            }
        }
    } else {
        let plan = ProjectPlan::from_prompts(args, None, metadata, prompter)?;
        Ok((plan, true))
    }
}

fn prompt_for_saved_config_strategy(prompter: &mut impl Prompter) -> Result<SavedConfigStrategy> {
    let options = [
        SelectOption {
            id: "edit".to_string(),
            name: "Edit saved defaults".to_string(),
        },
        SelectOption {
            id: "reuse".to_string(),
            name: "Use saved defaults as-is".to_string(),
        },
        SelectOption {
            id: "fresh".to_string(),
            name: "Start fresh (ignore saved defaults)".to_string(),
        },
    ];
    let selected = prompter.select(
        "Found saved spg defaults. How would you like to proceed?",
        &options,
        Some("edit"),
    )?;
    match selected.as_str() {
        "reuse" => Ok(SavedConfigStrategy::Reuse),
        "edit" => Ok(SavedConfigStrategy::Edit),
        "fresh" => Ok(SavedConfigStrategy::Fresh),
        other => bail!("unexpected saved config strategy '{other}'"),
    }
}

fn prepare_output_dir(output_dir: &Path) -> Result<()> {
    fs::create_dir_all(output_dir).with_context(|| {
        format!(
            "failed to create output directory at {}",
            output_dir.display()
        )
    })?;

    let probe = output_dir.join(".spg-write-check");
    fs::write(&probe, b"").with_context(|| {
        format!(
            "output directory is not writable at {}",
            output_dir.display()
        )
    })?;
    let _ = fs::remove_file(&probe);
    Ok(())
}

fn print_success(stdout: &mut impl Write, plan: &ProjectPlan, project_dir: &Path) -> Result<()> {
    writeln!(
        stdout,
        "Created Spring Boot project at {}",
        project_dir.display()
    )?;
    writeln!(stdout)?;
    writeln!(stdout, "Next steps:")?;
    writeln!(stdout, "  cd {}", plan.generation.base_dir)?;
    let project_type = plan.generation.project_type.as_str();
    if project_type.contains("gradle") {
        writeln!(stdout, "  ./gradlew bootRun")?;
    } else if project_type.contains("maven") {
        writeln!(stdout, "  ./mvnw spring-boot:run")?;
    }
    Ok(())
}

fn print_dependencies(
    metadata: &InitializrMetadata,
    query: Option<&str>,
    stdout: &mut impl Write,
) -> Result<()> {
    let dependencies = dependencies::search_dependencies(metadata, query.unwrap_or_default());

    if dependencies.is_empty() {
        writeln!(stdout, "No Spring Initializr dependencies found")?;
        return Ok(());
    }

    for dependency in dependencies {
        writeln!(
            stdout,
            "{}\t{}\t{}",
            dependency.id, dependency.name, dependency.group
        )?;
    }

    Ok(())
}

fn show_config(paths: &AppPaths, stdout: &mut impl Write) -> Result<()> {
    match user_config::load(&paths.user_config_file)? {
        Some(config) => {
            let raw = user_config::to_toml(&config)?;
            write!(stdout, "{raw}")?;
        }
        None => {
            writeln!(
                stdout,
                "No saved spg config found at {}",
                paths.user_config_file.display()
            )?;
        }
    }

    Ok(())
}

fn reset_config(
    paths: &AppPaths,
    stdout: &mut impl Write,
    confirmation: &mut impl Confirmation,
) -> Result<()> {
    if user_config::load(&paths.user_config_file)?.is_none() {
        writeln!(
            stdout,
            "No saved spg config found at {}",
            paths.user_config_file.display()
        )?;
        return Ok(());
    }

    if confirmation.confirm("Reset saved spg config?")? {
        user_config::remove(&paths.user_config_file)?;
        writeln!(stdout, "Reset saved spg config")?;
    } else {
        writeln!(stdout, "Kept saved spg config")?;
    }

    Ok(())
}

fn clear_cache(paths: &AppPaths, stdout: &mut impl Write) -> Result<()> {
    if cache::clear_metadata_cache(&paths.metadata_cache_file)? {
        writeln!(stdout, "Cleared Spring Initializr metadata cache")?;
    } else {
        writeln!(
            stdout,
            "No Spring Initializr metadata cache found at {}",
            paths.metadata_cache_file.display()
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cli::Cli,
        config::{paths::AppPaths, user_config::UserConfig},
        initializr::metadata::InitializrMetadata,
    };
    use clap::Parser;
    use std::{
        fs,
        future::Future,
        io::Cursor,
        path::{Path, PathBuf},
        pin::Pin,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[derive(Default)]
    struct StaticConfirmation {
        responses: Vec<bool>,
    }

    impl StaticConfirmation {
        fn yes() -> Self {
            Self {
                responses: vec![true],
            }
        }

        fn no() -> Self {
            Self {
                responses: vec![false],
            }
        }
    }

    impl Confirmation for StaticConfirmation {
        fn confirm(&mut self, _message: &str) -> anyhow::Result<bool> {
            Ok(self.responses.remove(0))
        }
    }

    struct UnusedPrompter;

    impl Prompter for UnusedPrompter {
        fn text(&mut self, message: &str, _default: Option<&str>) -> anyhow::Result<String> {
            Err(anyhow::anyhow!(
                "Prompter::text should not be called in this test (message: {message})"
            ))
        }

        fn select(
            &mut self,
            message: &str,
            _options: &[crate::initializr::metadata::SelectOption],
            _default_id: Option<&str>,
        ) -> anyhow::Result<String> {
            Err(anyhow::anyhow!(
                "Prompter::select should not be called in this test (message: {message})"
            ))
        }
    }

    #[derive(Default)]
    struct ScriptedPrompter {
        text_responses: std::collections::VecDeque<String>,
        select_responses: std::collections::VecDeque<String>,
        text_messages: Vec<String>,
        select_messages: Vec<String>,
    }

    impl Prompter for ScriptedPrompter {
        fn text(&mut self, message: &str, _default: Option<&str>) -> anyhow::Result<String> {
            self.text_messages.push(message.to_string());
            self.text_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted text response for {message:?}"))
        }

        fn select(
            &mut self,
            message: &str,
            _options: &[crate::initializr::metadata::SelectOption],
            _default_id: Option<&str>,
        ) -> anyhow::Result<String> {
            self.select_messages.push(message.to_string());
            self.select_responses
                .pop_front()
                .ok_or_else(|| anyhow::anyhow!("no scripted select response for {message:?}"))
        }
    }

    struct StaticMetadataProvider {
        metadata: InitializrMetadata,
    }

    impl MetadataProvider for StaticMetadataProvider {
        fn fetch_metadata<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<InitializrMetadata>> + 'a>> {
            let metadata = self.metadata.clone();
            Box::pin(async move { Ok(metadata) })
        }
    }

    struct CountingMetadataProvider {
        metadata: InitializrMetadata,
        calls: usize,
    }

    impl MetadataProvider for CountingMetadataProvider {
        fn fetch_metadata<'a>(
            &'a mut self,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<InitializrMetadata>> + 'a>> {
            self.calls += 1;
            let metadata = self.metadata.clone();
            Box::pin(async move { Ok(metadata) })
        }
    }

    struct UnusedStarterZipProvider;

    impl StarterZipProvider for UnusedStarterZipProvider {
        fn download_starter_zip<'a>(
            &'a mut self,
            _params: &'a GenerationParams,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<u8>>> + 'a>> {
            Box::pin(async move {
                Err(anyhow::anyhow!(
                    "StarterZipProvider should not be called in this test"
                ))
            })
        }
    }

    struct StaticStarterZipProvider {
        bytes: Vec<u8>,
        captured: Vec<GenerationParams>,
    }

    impl StarterZipProvider for StaticStarterZipProvider {
        fn download_starter_zip<'a>(
            &'a mut self,
            params: &'a GenerationParams,
        ) -> Pin<Box<dyn Future<Output = anyhow::Result<Vec<u8>>> + 'a>> {
            self.captured.push(params.clone());
            let bytes = self.bytes.clone();
            Box::pin(async move { Ok(bytes) })
        }
    }

    #[tokio::test]
    async fn config_show_prints_saved_toml() -> anyhow::Result<()> {
        let paths = temp_paths("config-show");
        let config = UserConfig {
            group_id: Some("com.example".to_string()),
            language: Some("java".to_string()),
            build: Some("maven".to_string()),
            packaging: Some("jar".to_string()),
            java_version: Some("21".to_string()),
            dependencies: vec!["web".to_string(), "validation".to_string()],
            package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
            output_dir: Some("~/projects".to_string()),
        };
        crate::config::user_config::save(&paths.user_config_file, &config)?;

        let cli = Cli::parse_from(["spg", "config", "show"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;

        run_with_paths(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("group_id = \"com.example\""));
        assert!(output.contains("dependencies = [\"web\", \"validation\"]"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_show_reports_when_config_is_missing() -> anyhow::Result<()> {
        let paths = temp_paths("config-show-missing");
        let cli = Cli::parse_from(["spg", "config", "show"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;

        run_with_paths(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("No saved spg config found"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_reset_deletes_config_after_confirmation() -> anyhow::Result<()> {
        let paths = temp_paths("config-reset");
        crate::config::user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.example".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from(["spg", "config", "reset"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::yes();
        let mut prompter = UnusedPrompter;

        run_with_paths(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
        )
        .await?;

        assert!(!paths.user_config_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Reset saved spg config"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn config_reset_preserves_config_when_declined() -> anyhow::Result<()> {
        let paths = temp_paths("config-reset-declined");
        crate::config::user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.example".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from(["spg", "config", "reset"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::no();
        let mut prompter = UnusedPrompter;

        run_with_paths(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
        )
        .await?;

        assert!(paths.user_config_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Kept saved spg config"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn cache_clear_deletes_metadata_cache() -> anyhow::Result<()> {
        let paths = temp_paths("cache-clear");
        fs::create_dir_all(paths.metadata_cache_file.parent().unwrap())?;
        fs::write(&paths.metadata_cache_file, "{}")?;

        let cli = Cli::parse_from(["spg", "cache", "clear"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;

        run_with_paths(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
        )
        .await?;

        assert!(!paths.metadata_cache_file.exists());
        assert!(String::from_utf8(stdout)?.contains("Cleared Spring Initializr metadata cache"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn deps_prints_dependency_catalog_from_metadata_provider() -> anyhow::Result<()> {
        let paths = temp_paths("deps");
        let cli = Cli::parse_from(["spg", "deps"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: sample_metadata()?,
        };
        let mut starter_zip = UnusedStarterZipProvider;

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("web\tSpring Web\tWeb"));
        assert!(output.contains("data-jpa\tSpring Data JPA\tSQL"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn deps_filters_dependency_catalog_with_query() -> anyhow::Result<()> {
        let paths = temp_paths("deps-query");
        let cli = Cli::parse_from(["spg", "deps", "jpa"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: sample_metadata()?,
        };
        let mut starter_zip = UnusedStarterZipProvider;

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(!output.contains("web\tSpring Web\tWeb"));
        assert!(output.contains("data-jpa\tSpring Data JPA\tSQL"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn deps_uses_fresh_cached_metadata_without_fetching_upstream() -> anyhow::Result<()> {
        let paths = temp_paths("deps-cache-hit");
        crate::config::cache::save_metadata_cache(
            &paths.metadata_cache_file,
            &sample_metadata()?,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000),
        )?;

        let cli = Cli::parse_from(["spg", "deps"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut upstream = CountingMetadataProvider {
            metadata: InitializrMetadata::default(),
            calls: 0,
        };
        let mut cached = CachedMetadataProvider::with_settings(
            paths.metadata_cache_file.clone(),
            &mut upstream,
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_100),
            std::time::Duration::from_secs(24 * 60 * 60),
        );
        let mut starter_zip = UnusedStarterZipProvider;

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut cached,
            &mut starter_zip,
        )
        .await?;

        drop(cached);
        assert_eq!(upstream.calls, 0);
        assert!(String::from_utf8(stdout)?.contains("web\tSpring Web\tWeb"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn deps_fetches_and_saves_metadata_when_cache_is_missing() -> anyhow::Result<()> {
        let paths = temp_paths("deps-cache-miss");
        let cli = Cli::parse_from(["spg", "deps"]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut upstream = CountingMetadataProvider {
            metadata: sample_metadata()?,
            calls: 0,
        };
        let now = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let mut cached = CachedMetadataProvider::with_settings(
            paths.metadata_cache_file.clone(),
            &mut upstream,
            now,
            std::time::Duration::from_secs(24 * 60 * 60),
        );
        let mut starter_zip = UnusedStarterZipProvider;

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut cached,
            &mut starter_zip,
        )
        .await?;

        drop(cached);
        assert_eq!(upstream.calls, 1);
        assert!(paths.metadata_cache_file.exists());
        assert!(
            crate::config::cache::load_fresh_metadata_cache(
                &paths.metadata_cache_file,
                now,
                std::time::Duration::from_secs(24 * 60 * 60)
            )?
            .is_some()
        );
        assert!(String::from_utf8(stdout)?.contains("web\tSpring Web\tWeb"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_defaults_extracts_starter_archive_and_prints_next_steps()
    -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-defaults");
        let output_dir = root.join("projects");

        let cli = Cli::parse_from([
            "spg",
            "init",
            "orders-api",
            "--defaults",
            "--group-id",
            "com.acme",
            "--package-name",
            "com.acme.orders",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("orders-api")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let project_dir = output_dir.join("orders-api");
        assert!(project_dir.join("pom.xml").exists());
        assert_eq!(starter_zip.captured.len(), 1);
        let params = &starter_zip.captured[0];
        assert_eq!(params.base_dir, "orders-api");
        assert_eq!(params.artifact_id, "orders-api");
        assert_eq!(params.group_id, "com.acme");
        assert_eq!(params.package_name, "com.acme.orders");
        assert_eq!(params.project_type, "maven-project");

        let output = String::from_utf8(stdout)?;
        assert!(output.contains(
            format!("Created Spring Boot project at {}", project_dir.display()).as_str()
        ));
        assert!(output.contains("cd orders-api"));
        assert!(output.contains("./mvnw spring-boot:run"));
        assert!(stderr.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_gradle_build_prints_gradle_runner_hint() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-gradle-hint");
        let output_dir = root.join("projects");

        let cli = Cli::parse_from([
            "spg",
            "init",
            "demo",
            "--defaults",
            "--build",
            "gradle",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let output = String::from_utf8(stdout)?;
        assert!(output.contains("./gradlew bootRun"));
        assert!(!output.contains("./mvnw"));
        assert_eq!(starter_zip.captured[0].project_type, "gradle-project");

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_defaults_bails_when_target_directory_already_exists() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-defaults-existing-target");
        let output_dir = root.join("projects");
        let existing = output_dir.join("demo");
        fs::create_dir_all(&existing)?;
        fs::write(existing.join("marker.txt"), b"keep")?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "demo",
            "--defaults",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("demo")?,
            captured: Vec::new(),
        };

        let error = run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await
        .expect_err("--defaults must not prompt when target exists; should bail");

        let message = error.to_string();
        assert!(message.contains("already exists"), "got: {message}");
        assert!(message.contains("--output-dir"), "got: {message}");
        assert!(existing.join("marker.txt").exists());
        assert!(starter_zip.captured.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_interactive_aborts_with_error_when_user_declines_overwrite() -> anyhow::Result<()>
    {
        let (paths, root) = temp_paths_with_root("init-overwrite-decline-interactive");
        let output_dir = root.join("projects");
        let existing = output_dir.join("demo");
        fs::create_dir_all(&existing)?;
        fs::write(existing.join("marker.txt"), b"keep")?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "demo",
            "--group-id",
            "com.example",
            "--artifact-id",
            "demo",
            "--description",
            "demo",
            "--package-name",
            "com.example.demo",
            "--type",
            "maven-project",
            "--language",
            "java",
            "--boot-version",
            "3.5.0",
            "--java-version",
            "17",
            "--packaging",
            "jar",
            "--dependency",
            "web",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::no();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("demo")?,
            captured: Vec::new(),
        };

        let error = run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await
        .expect_err("declined overwrite must propagate as an error");

        let message = error.to_string();
        assert!(message.contains("aborted"), "got: {message}");
        assert!(existing.join("marker.txt").exists());
        assert!(starter_zip.captured.is_empty());

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_refresh_clears_cached_metadata_before_fetching() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-refresh");
        let output_dir = root.join("projects");
        crate::config::cache::save_metadata_cache(
            &paths.metadata_cache_file,
            &full_sample_metadata()?,
            UNIX_EPOCH + Duration::from_secs(1_000),
        )?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "demo",
            "--defaults",
            "--refresh",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut upstream = CountingMetadataProvider {
            metadata: full_sample_metadata()?,
            calls: 0,
        };
        let mut cached = CachedMetadataProvider::with_settings(
            paths.metadata_cache_file.clone(),
            &mut upstream,
            UNIX_EPOCH + Duration::from_secs(1_100),
            Duration::from_secs(24 * 60 * 60),
        );
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut cached,
            &mut starter_zip,
        )
        .await?;

        drop(cached);
        assert_eq!(upstream.calls, 1);
        assert_eq!(starter_zip.captured.len(), 1);

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_loads_saved_user_config_for_missing_flags() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-uses-saved-config");
        let output_dir = root.join("projects");
        crate::config::user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.saved".to_string()),
                build: Some("gradle".to_string()),
                java_version: Some("21".to_string()),
                dependencies: vec!["web".to_string()],
                package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "saved-demo",
            "--defaults",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("saved-demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let params = &starter_zip.captured[0];
        assert_eq!(params.group_id, "com.saved");
        assert_eq!(params.project_type, "gradle-project");
        assert_eq!(params.java_version, "21");
        assert_eq!(params.dependencies, ["web"]);
        assert_eq!(params.package_name, "com.saved.saved_demo");

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_without_defaults_drives_interactive_prompts() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-interactive");
        let output_dir = root.join("projects");

        let cli = Cli::parse_from([
            "spg",
            "init",
            "interactive-demo",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::no();
        let mut prompter = ScriptedPrompter {
            text_responses: [
                "com.acme",
                "interactive-demo",
                "Interactive demo",
                "com.acme.demo",
                "web",
                "",
            ]
            .into_iter()
            .map(String::from)
            .collect(),
            select_responses: ["maven-project", "java", "3.5.0", "17", "jar", "web"]
                .into_iter()
                .map(String::from)
                .collect(),
            ..ScriptedPrompter::default()
        };
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("interactive-demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        assert_eq!(
            prompter.text_messages[..4],
            ["Group ID?", "Artifact ID?", "Description?", "Package name?",]
        );
        assert!(
            prompter.text_messages[4].starts_with("Search dependencies"),
            "interactive picker should run after the standard text fields"
        );
        assert_eq!(
            prompter.select_messages,
            [
                "Project type?",
                "Language?",
                "Spring Boot version?",
                "Java version?",
                "Packaging?",
                "Add which dependency?",
            ]
        );
        let params = &starter_zip.captured[0];
        assert_eq!(params.group_id, "com.acme");
        assert_eq!(params.artifact_id, "interactive-demo");
        assert_eq!(params.description, "Interactive demo");
        assert_eq!(params.package_name, "com.acme.demo");
        assert_eq!(params.project_type, "maven-project");
        assert_eq!(params.language, "java");
        assert_eq!(params.java_version, "17");
        assert_eq!(params.packaging, "jar");
        assert_eq!(params.dependencies, ["web"]);
        assert!(
            !paths.user_config_file.exists(),
            "save prompt was declined; no config should be written"
        );

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_interactive_saves_defaults_when_user_confirms() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-save-defaults");
        let output_dir = root.join("projects");

        let cli = Cli::parse_from([
            "spg",
            "init",
            "saved-demo",
            "--group-id",
            "com.acme",
            "--artifact-id",
            "saved-demo",
            "--description",
            "Saved demo",
            "--package-name",
            "com.acme.demo",
            "--type",
            "gradle-project",
            "--language",
            "java",
            "--boot-version",
            "3.5.0",
            "--java-version",
            "21",
            "--packaging",
            "jar",
            "--dependency",
            "web",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::yes();
        let mut prompter = UnusedPrompter;
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("saved-demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        assert!(paths.user_config_file.exists());
        let saved = user_config::load(&paths.user_config_file)?.expect("config saved");
        assert_eq!(saved.group_id.as_deref(), Some("com.acme"));
        assert_eq!(saved.build.as_deref(), Some("gradle"));
        assert_eq!(saved.java_version.as_deref(), Some("21"));
        assert_eq!(saved.dependencies, ["web"]);
        assert!(
            String::from_utf8(stdout)?.contains("Saved spg defaults"),
            "success path should mention persisted defaults"
        );

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_saved_config_reuse_strategy_skips_field_prompts() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-reuse-saved");
        let output_dir = root.join("projects");
        user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.saved".to_string()),
                build: Some("gradle".to_string()),
                java_version: Some("21".to_string()),
                dependencies: vec!["web".to_string()],
                package_name_pattern: Some("{group_id}.{artifact_id}".to_string()),
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "reuse-demo",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::default();
        let mut prompter = ScriptedPrompter {
            select_responses: ["reuse"].into_iter().map(String::from).collect(),
            ..ScriptedPrompter::default()
        };
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("reuse-demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        assert_eq!(prompter.text_messages.len(), 0, "no field prompts");
        assert_eq!(prompter.select_messages.len(), 1);
        assert!(prompter.select_messages[0].contains("saved spg defaults"));
        let params = &starter_zip.captured[0];
        assert_eq!(params.group_id, "com.saved");
        assert_eq!(params.project_type, "gradle-project");
        assert_eq!(params.java_version, "21");
        assert_eq!(params.dependencies, ["web"]);
        assert_eq!(params.package_name, "com.saved.reuse_demo");

        cleanup(&paths);
        Ok(())
    }

    #[tokio::test]
    async fn init_with_saved_config_fresh_strategy_ignores_saved_defaults() -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-fresh-strategy");
        let output_dir = root.join("projects");
        user_config::save(
            &paths.user_config_file,
            &UserConfig {
                group_id: Some("com.saved".to_string()),
                build: Some("gradle".to_string()),
                java_version: Some("21".to_string()),
                dependencies: vec!["web".to_string()],
                ..UserConfig::default()
            },
        )?;

        let cli = Cli::parse_from([
            "spg",
            "init",
            "fresh-demo",
            "--artifact-id",
            "fresh-demo",
            "--description",
            "Fresh",
            "--package-name",
            "com.example.fresh",
            "--dependency",
            "web",
            "--output-dir",
            output_dir.to_str().unwrap(),
        ]);
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut confirmation = StaticConfirmation::no();
        let mut prompter = ScriptedPrompter {
            text_responses: ["com.example"].into_iter().map(String::from).collect(),
            select_responses: ["fresh", "maven-project", "java", "3.5.0", "17", "jar"]
                .into_iter()
                .map(String::from)
                .collect(),
            ..ScriptedPrompter::default()
        };
        let mut metadata = StaticMetadataProvider {
            metadata: full_sample_metadata()?,
        };
        let mut starter_zip = StaticStarterZipProvider {
            bytes: make_demo_zip("fresh-demo")?,
            captured: Vec::new(),
        };

        run_with_services(
            cli,
            &paths,
            &mut stdout,
            &mut stderr,
            &mut confirmation,
            &mut prompter,
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        let params = &starter_zip.captured[0];
        assert_eq!(params.group_id, "com.example", "saved group id was ignored");
        assert_eq!(
            params.project_type, "maven-project",
            "saved build (gradle) was ignored in favor of metadata default"
        );
        assert_eq!(params.java_version, "17", "saved Java 21 was ignored");

        cleanup(&paths);
        Ok(())
    }

    fn full_sample_metadata() -> serde_json::Result<InitializrMetadata> {
        serde_json::from_str(
            r#"
            {
              "type": {
                "default": "maven-project",
                "values": [
                  { "id": "maven-project", "name": "Maven" },
                  { "id": "gradle-project", "name": "Gradle" }
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
                "default": "17",
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
                      { "id": "web", "name": "Spring Web" }
                    ]
                  }
                ]
              }
            }
            "#,
        )
    }

    fn make_demo_zip(base_dir: &str) -> anyhow::Result<Vec<u8>> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer.add_directory(format!("{base_dir}/"), options)?;
        writer.start_file(format!("{base_dir}/pom.xml"), options)?;
        writer.write_all(b"<project />")?;
        Ok(writer.finish()?.into_inner())
    }

    fn temp_paths_with_root(test_name: &str) -> (AppPaths, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("spg-{test_name}-{unique}"));
        let paths = AppPaths::from_dirs(root.join("config"), root.join("cache"));
        (paths, root)
    }

    fn sample_metadata() -> serde_json::Result<InitializrMetadata> {
        serde_json::from_str(
            r#"
            {
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
        )
    }

    fn temp_paths(test_name: &str) -> AppPaths {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("spg-{test_name}-{unique}"));
        AppPaths::from_dirs(root.join("config"), root.join("cache"))
    }

    fn cleanup(paths: &AppPaths) {
        remove_ancestor(&paths.user_config_file);
        remove_ancestor(&paths.metadata_cache_file);
    }

    fn remove_ancestor(path: &Path) {
        if let Some(root) = path
            .ancestors()
            .find(|ancestor| ancestor.file_name().is_some_and(|name| name == "config"))
            .and_then(Path::parent)
            .map(PathBuf::from)
        {
            let _ = fs::remove_dir_all(root);
        }
    }
}
