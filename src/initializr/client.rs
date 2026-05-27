use crate::initializr::{generate::GenerationParams, metadata::InitializrMetadata};
use anyhow::{Context, Result, bail};
use reqwest::{
    Client, Request, Url,
    header::{ACCEPT, USER_AGENT},
};

pub const INITIALIZR_METADATA_ACCEPT: &str = "application/vnd.initializr.v2.1+json";
pub const USER_AGENT_VALUE: &str = concat!("spg/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone)]
pub struct InitializrClient {
    http: Client,
    base_url: Url,
}

impl InitializrClient {
    pub fn new_default() -> Result<Self> {
        Self::new("https://start.spring.io")
    }

    pub fn new(base_url: &str) -> Result<Self> {
        let base_url = Url::parse(base_url)
            .with_context(|| format!("invalid Spring Initializr base URL: {base_url}"))?;
        let http = Client::builder()
            .user_agent(USER_AGENT_VALUE)
            .build()
            .context("failed to build Spring Initializr HTTP client")?;

        Ok(Self { http, base_url })
    }

    pub fn metadata_request(&self) -> Result<Request> {
        self.http
            .get(self.base_url.clone())
            .header(ACCEPT, INITIALIZR_METADATA_ACCEPT)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .build()
            .context("failed to build Spring Initializr metadata request")
    }

    pub fn starter_zip_request(&self, params: &GenerationParams) -> Result<Request> {
        let url = params.starter_zip_url(&self.base_url)?;

        self.http
            .get(url)
            .header(USER_AGENT, USER_AGENT_VALUE)
            .build()
            .context("failed to build Spring Initializr generation request")
    }

    pub async fn fetch_metadata(&self) -> Result<InitializrMetadata> {
        let request = self.metadata_request()?;
        let response = self
            .http
            .execute(request)
            .await
            .context("failed to fetch Spring Initializr metadata")?;
        let status = response.status();

        if !status.is_success() {
            bail!("Spring Initializr metadata request failed with HTTP {status}");
        }

        response
            .json::<InitializrMetadata>()
            .await
            .context("failed to parse Spring Initializr metadata")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::initializr::generate::GenerationParams;
    use reqwest::header::{ACCEPT, USER_AGENT};

    #[test]
    fn metadata_request_targets_base_url_and_sets_initializr_headers() -> anyhow::Result<()> {
        let client = InitializrClient::new("https://start.example")?;
        let request = client.metadata_request()?;

        assert_eq!(request.url().as_str(), "https://start.example/");
        assert_eq!(
            request.headers().get(ACCEPT).unwrap(),
            INITIALIZR_METADATA_ACCEPT
        );
        assert_eq!(request.headers().get(USER_AGENT).unwrap(), USER_AGENT_VALUE);

        Ok(())
    }

    #[test]
    fn starter_zip_request_targets_generation_endpoint_with_query() -> anyhow::Result<()> {
        let client = InitializrClient::new("https://start.example")?;
        let request = client.starter_zip_request(&sample_generation_params())?;

        assert_eq!(request.url().scheme(), "https");
        assert_eq!(request.url().host_str(), Some("start.example"));
        assert_eq!(request.url().path(), "/starter.zip");
        assert!(
            request
                .url()
                .query()
                .unwrap()
                .contains("type=maven-project")
        );
        assert!(request.url().query().unwrap().contains("dependencies=web"));
        assert_eq!(request.headers().get(USER_AGENT).unwrap(), USER_AGENT_VALUE);

        Ok(())
    }

    fn sample_generation_params() -> GenerationParams {
        GenerationParams {
            project_type: "maven-project".to_string(),
            language: "java".to_string(),
            boot_version: "3.5.0".to_string(),
            base_dir: "orders-api".to_string(),
            group_id: "com.anas".to_string(),
            artifact_id: "orders".to_string(),
            name: "orders-api".to_string(),
            description: "Orders API".to_string(),
            package_name: "com.anas.orders".to_string(),
            packaging: "jar".to_string(),
            java_version: "21".to_string(),
            dependencies: vec!["web".to_string()],
        }
    }
}
