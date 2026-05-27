use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "spg",
    version,
    about = "Interactive Spring Initializr project generator",
    subcommand_required = true,
    arg_required_else_help = true,
    disable_help_subcommand = true,
    disable_version_flag = true
)]
pub struct Cli {
    #[arg(
        short = 'v',
        long = "version",
        action = clap::ArgAction::Version,
        help = "Print version"
    )]
    version: Option<bool>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Create a Spring Boot project.
    Init(Box<InitArgs>),

    /// Search Spring Initializr dependencies.
    Deps(DepsArgs),

    /// Manage saved user defaults.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Manage cached Spring Initializr metadata.
    #[command(subcommand)]
    Cache(CacheCommand),
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project name to generate.
    pub project_name: Option<String>,

    /// Use saved defaults where possible.
    #[arg(long)]
    pub defaults: bool,

    /// Force a fresh Spring Initializr metadata fetch.
    #[arg(long)]
    pub refresh: bool,

    /// Java group id, such as com.example.
    #[arg(long)]
    pub group_id: Option<String>,

    /// Java artifact id.
    #[arg(long)]
    pub artifact_id: Option<String>,

    /// Project description.
    #[arg(long)]
    pub description: Option<String>,

    /// Java package name.
    #[arg(long)]
    pub package_name: Option<String>,

    /// Build tool preference from Initializr metadata.
    #[arg(long)]
    pub build: Option<String>,

    /// Initializr project type, such as maven-project.
    #[arg(long = "type")]
    pub project_type: Option<String>,

    /// Project language from Initializr metadata.
    #[arg(long)]
    pub language: Option<String>,

    /// Spring Boot version from Initializr metadata.
    #[arg(long)]
    pub boot_version: Option<String>,

    /// Java version from Initializr metadata.
    #[arg(long)]
    pub java_version: Option<String>,

    /// Packaging option from Initializr metadata.
    #[arg(long)]
    pub packaging: Option<String>,

    /// Dependency id from Initializr metadata. Can be repeated.
    #[arg(short = 'd', long = "dependency")]
    pub dependencies: Vec<String>,

    /// Directory where the project folder should be written.
    #[arg(long)]
    pub output_dir: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct DepsArgs {
    /// Optional search query for filtering dependencies.
    pub query: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show saved user defaults.
    Show,

    /// Reset saved user defaults.
    Reset,
}

#[derive(Debug, Subcommand)]
pub enum CacheCommand {
    /// Clear cached Spring Initializr metadata.
    Clear,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, Parser, error::ErrorKind};
    use std::path::PathBuf;

    #[test]
    fn parses_init_with_project_name_and_partial_flags() {
        let cli = Cli::parse_from([
            "spg",
            "init",
            "my-api",
            "--defaults",
            "--refresh",
            "--group-id",
            "com.anas",
            "--artifact-id",
            "orders",
            "--description",
            "Orders API",
            "--package-name",
            "com.anas.orders",
            "--build",
            "maven",
            "--type",
            "maven-project",
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
            "-d",
            "validation",
            "--output-dir",
            "projects",
        ]);

        let args = match cli.command {
            Commands::Init(args) => args,
            other => panic!("expected init command, got {other:?}"),
        };

        assert_eq!(args.project_name.as_deref(), Some("my-api"));
        assert!(args.defaults);
        assert!(args.refresh);
        assert_eq!(args.group_id.as_deref(), Some("com.anas"));
        assert_eq!(args.artifact_id.as_deref(), Some("orders"));
        assert_eq!(args.description.as_deref(), Some("Orders API"));
        assert_eq!(args.package_name.as_deref(), Some("com.anas.orders"));
        assert_eq!(args.build.as_deref(), Some("maven"));
        assert_eq!(args.project_type.as_deref(), Some("maven-project"));
        assert_eq!(args.language.as_deref(), Some("java"));
        assert_eq!(args.boot_version.as_deref(), Some("3.5.0"));
        assert_eq!(args.java_version.as_deref(), Some("21"));
        assert_eq!(args.packaging.as_deref(), Some("jar"));
        assert_eq!(args.dependencies, ["web", "validation"]);
        assert_eq!(args.output_dir, Some(PathBuf::from("projects")));
    }

    #[test]
    fn parses_top_level_commands() {
        let deps = Cli::parse_from(["spg", "deps"]);
        assert!(matches!(
            deps.command,
            Commands::Deps(DepsArgs { query: None })
        ));

        let deps_query = Cli::parse_from(["spg", "deps", "web"]);
        assert!(matches!(
            deps_query.command,
            Commands::Deps(DepsArgs {
                query: Some(query)
            }) if query == "web"
        ));

        let config_show = Cli::parse_from(["spg", "config", "show"]);
        assert!(matches!(
            config_show.command,
            Commands::Config(ConfigCommand::Show)
        ));

        let cache_clear = Cli::parse_from(["spg", "cache", "clear"]);
        assert!(matches!(
            cache_clear.command,
            Commands::Cache(CacheCommand::Clear)
        ));
    }

    #[test]
    fn help_flags_display_help_without_subcommand() {
        for flag in ["--help", "-h"] {
            let err = Cli::command()
                .try_get_matches_from(["spg", flag])
                .expect_err("help flag should exit during clap parsing");

            assert_eq!(err.kind(), ErrorKind::DisplayHelp);
        }
    }

    #[test]
    fn version_flags_display_version_without_subcommand() {
        for flag in ["--version", "-v"] {
            let err = Cli::command()
                .try_get_matches_from(["spg", flag])
                .expect_err("version flag should exit during clap parsing");

            assert_eq!(err.kind(), ErrorKind::DisplayVersion);
        }
    }
}
