use std::str::FromStr;

use reqwest::Url;

#[derive(Debug)]
#[allow(dead_code)]
pub struct TequilaConfig {
    pub organization: String,
    pub server: String,
    pub domain: String,
    pub manager: String,
    pub cookies: String, // Could be a bool, but its not documented, so I'm not gonna take this risk
    pub support_certificates: String, // Same as above
    pub default_languagge: String,
    pub attributes: Vec<String>,
    pub certificate: String,
}

#[derive(Debug)]
pub enum ConfigError {
    MissingEntry(String),
    Request(reqwest::Error),
    Url(url::ParseError),
}

impl TequilaConfig {
    fn from_string(s: String) -> Result<Self, ConfigError> {
        fn extract_value(lines: &[&str], name: &str) -> Result<String, ConfigError> {
            match lines
                .iter()
                .find_map(|l| l.strip_prefix(format!("{name}:").as_str()))
            {
                Some(v) => Ok(v.trim().to_string()),
                None => Err(ConfigError::MissingEntry(name.into())),
            }
        }

        let lines = s.split('\n').collect::<Vec<_>>();
        Ok(TequilaConfig {
            organization: extract_value(&lines, "Organization")?,
            server: extract_value(&lines, "Server")?,
            domain: extract_value(&lines, "Domain")?,
            manager: extract_value(&lines, "Manager")?,
            cookies: extract_value(&lines, "Cookies")?,
            support_certificates: extract_value(&lines, "Support certificates")?,
            default_languagge: extract_value(&lines, "Default language")?,
            attributes: extract_value(&lines, "Supported user attributes ")? // Somehow there is an extra space before the colon
                .split(" ")
                .map(String::from)
                .collect(),
            certificate: extract_value(&lines, "Server certificate")?,
        })
    }

    pub fn fetch(url: String) -> Result<Self, ConfigError> {
        println!("{url}");
        Self::from_string(
            reqwest::blocking::get(
                Url::from_str(url.as_str())
                    .map_err(ConfigError::Url)?
                    .join("getconfig")
                    .map_err(ConfigError::Url)?,
            )
            .map_err(ConfigError::Request)?
            .text()
            .map_err(ConfigError::Request)?,
        )
    }
}
