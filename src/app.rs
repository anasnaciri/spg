use crate::{
    archive::unzip::extract_zip_archive,
    cli::{CacheCommand, Cli, Commands, ConfigCommand, InitArgs},
    config::{cache, paths::AppPaths, user_config},
    initializr::{
        client::InitializrClient, generate::GenerationParams, metadata::InitializrMetadata,
    },
    prompts::{dependencies, project::ProjectPlan},
};
use anyhow::{Context, Result};
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

    run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await
}

pub async fn run_with_paths(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
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
        &mut metadata_provider,
        &mut starter_zip_client,
    )
    .await
}

pub async fn run_with_services(
    cli: Cli,
    paths: &AppPaths,
    stdout: &mut impl Write,
    stderr: &mut impl Write,
    confirmation: &mut impl Confirmation,
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

async fn run_init(
    args: InitArgs,
    paths: &AppPaths,
    stdout: &mut impl Write,
    confirmation: &mut impl Confirmation,
    metadata_provider: &mut impl MetadataProvider,
    starter_zip_provider: &mut impl StarterZipProvider,
) -> Result<()> {
    if args.refresh {
        cache::clear_metadata_cache(&paths.metadata_cache_file)?;
    }

    let config = user_config::load(&paths.user_config_file)?;
    let metadata = metadata_provider.fetch_metadata().await?;
    let plan = ProjectPlan::from_defaults(&args, config.as_ref(), &metadata)?;
    plan.generation.validate(&metadata)?;

    prepare_output_dir(&plan.output_dir)?;

    let project_dir = plan.output_dir.join(&plan.generation.base_dir);
    if project_dir.exists()
        && !confirmation.confirm(&format!(
            "{} already exists. Overwrite?",
            project_dir.display()
        ))?
    {
        writeln!(
            stdout,
            "Aborted: kept existing project at {}",
            project_dir.display()
        )?;
        return Ok(());
    }

    let archive = starter_zip_provider
        .download_starter_zip(&plan.generation)
        .await?;
    extract_zip_archive(Cursor::new(archive), &plan.output_dir)?;

    print_success(stdout, &plan, &project_dir)?;
    Ok(())
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

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

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

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

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

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

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

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

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

        run_with_paths(cli, &paths, &mut stdout, &mut stderr, &mut confirmation).await?;

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
    async fn init_aborts_when_user_declines_overwrite_of_existing_project_folder()
    -> anyhow::Result<()> {
        let (paths, root) = temp_paths_with_root("init-overwrite-declined");
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
        let mut confirmation = StaticConfirmation::no();
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
            &mut metadata,
            &mut starter_zip,
        )
        .await?;

        assert!(existing.join("marker.txt").exists());
        assert!(starter_zip.captured.is_empty());
        assert!(String::from_utf8(stdout)?.contains("Aborted"));

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
