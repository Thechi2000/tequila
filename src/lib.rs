use std::collections::HashMap;

use url::Url;

pub use tequila_macros::*;

const TEQUILA_URL: &str = "https://tequila.epfl.ch/cgi-bin/tequila";

pub trait FromTequilaAttributes
where
    Self: Sized,
{
    fn from_tequila_attributes(attributes: HashMap<String, String>) -> Result<Self, TequilaError>;
}

fn build_hashmap(str: String) -> Result<HashMap<String, String>, TequilaError> {
    str.split('\n')
        .filter(|s| !s.is_empty())
        .map(|s| s.split_once('='))
        .fold(Ok(HashMap::new()), |map, opt| match (map, opt) {
            (Ok(mut map), Some((key, value))) => {
                map.insert(key.into(), value.into());
                Ok(map)
            }
            (Err(e), _) => Err(e),
            (_, None) => Err(TequilaError::InvalidResponse),
        })
}

#[derive(Debug)]
pub enum TequilaError {
    InvalidResponse,
    RequestError(reqwest::Error),
    MissingAttribute(String),
}

async fn send_request<R>(route: String, body: Vec<(&str, String)>) -> Result<R, TequilaError>
where
    R: FromTequilaAttributes,
{
    let response = reqwest::Client::new()
        .post(format!("{TEQUILA_URL}/{route}"))
        .header("Content-Type", "text/plain")
        .body(body.iter().fold(String::new(), |acc, (key, value)| {
            format!("{acc}\n{key}={value}")
        }))
        .send()
        .await
        .map_err(TequilaError::RequestError)?
        .text()
        .await
        .map_err(TequilaError::RequestError)?;

    R::from_tequila_attributes(build_hashmap(response)?)
}

impl FromTequilaAttributes for CreateRequestResponse {
    fn from_tequila_attributes(attributes: HashMap<String, String>) -> Result<Self, TequilaError> {
        Ok(Self {
            key: attributes
                .get("key")
                .ok_or(TequilaError::InvalidResponse)?
                .clone(),
        })
    }
}

struct CreateRequestResponse {
    key: String,
}

pub async fn create_request(
    return_url: Url,
    service_name: String,
    request_attributes: Vec<String>,
    wish_attributes: Vec<String>,
    require: String,
    allow: String,
    language: String,
) -> Result<String, TequilaError> {
    Ok(
        send_request::<CreateRequestResponse>("createrequest".into(), {
            let mut vec = vec![
                ("urlaccess", return_url.to_string()),
                ("service", service_name),
                ("mode_auth_check", "1".into()),
            ];

            if !request_attributes.is_empty() {
                vec.push(("request", request_attributes.join(",")))
            }
            if !wish_attributes.is_empty() {
                vec.push(("wish", wish_attributes.join(",")))
            }
            if !require.is_empty() {
                vec.push(("require", require))
            }
            if !allow.is_empty() {
                vec.push(("allow", allow))
            }
            if !language.is_empty() {
                vec.push(("language", language))
            }

            vec
        })
        .await?
        .key,
    )
}

pub async fn fetch_attributes<A>(key: String, auth_check: String) -> Result<A, TequilaError>
where
    A: FromTequilaAttributes,
{
    send_request(
        "fetchattributes".into(),
        vec![("key", key), ("auth_check", auth_check)],
    )
    .await
}
