use anyhow::{Context, Result};
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
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
