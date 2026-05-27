use crate::initializr::metadata::InitializrMetadata;
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
}
