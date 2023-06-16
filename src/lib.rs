//! This crate is an API binding to use Tequila's authentification scheme. Full (haha) reference of its API can be found [here](https://tequila.epfl.ch/download/2.0/docs/writing-clients.pdf)
//! 
//! Here is a quick reminder of Tequila's authentification flow:
//! - First, a request must be created on Tequila's servers, with the list of attributes you want to get about the user, requirements, etc, and a url where the user will be redirected. 
//! The server will return an API key, and the user must connect to `TEQUILA_URL/auth?requestkey={key}`
//! - After successful login, the user will be redirected to `{return_url}?key={key}&authcheck={auth_check}`
//! - To get the requested attributes, a second call must be made, using the request key (key) and the authentification token (auth_check)
//! 
//! There are two ways to authenticate using this crate: using a wrapper or, in a more raw approach, direct calls
//! 
//! # TequilaRequest
//! [TequilaRequest] is a wrapper over all calls that are made to Tequila's API. It enforces correcteness using typestate
//! 
//! # Direct calls
//! If you do not wish to use [TequilaRequest], you can make direct calls to the API through the functions [create_request] and [fetch_attributes].

use std::{collections::HashMap, marker::PhantomData};

use url::Url;

pub use tequila_macros::*;

pub const TEQUILA_URL: &str = "https://tequila.epfl.ch/cgi-bin/tequila";

/// This trait allow an object to be constructed from the response of the `fetch_attributes` route. It should be derived with the [FromTequilaAttributes](tequila_macros::FromTequilaAttributes) macro
pub trait FromTequilaAttributes
where
    Self: Sized,
{
    fn from_tequila_attributes(attributes: HashMap<String, String>) -> Result<Self, TequilaError>;

    fn wished_attributes() -> Vec<String>;
    fn requested_attributes() -> Vec<String>;
}

impl FromTequilaAttributes for () {
    fn from_tequila_attributes(_: HashMap<String, String>) -> Result<Self, TequilaError> {
        Ok(())
    }

    fn wished_attributes() -> Vec<String> {
        vec![]
    }

    fn requested_attributes() -> Vec<String> {
        vec![]
    }
}

/// Constructs a hashmap from a response from Tequila's API. The string is composed of key value pairs using `=` as a bind, and delimited by line feeds
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

/// Any error that may happen during a call to the API
#[derive(Debug)]
pub enum TequilaError {
    /// The response does not have the expected format
    InvalidResponse,
    /// Network error
    RequestError(reqwest::Error),
    /// The response is missing a required attribute
    MissingAttributes(Vec<String>),
}

/// Send a request to the API
/// # Parameters:
/// - route: the route to call (only the uri of the method, like `"createrequest"`, not the full url)
/// - body: a list of key value pairs
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

    fn wished_attributes() -> Vec<String> {
        vec![]
    }

    fn requested_attributes() -> Vec<String> {
        vec![]
    }
}

/// The response from the `"create_request"` route
struct CreateRequestResponse {
    key: String,
}

/// Create a request on the servers. Returns the key of the request
/// # Parameters
/// - `return_url`: The url where the user will be redirected after successful login
/// - `service_name`: A string identifying the service. It will be displayed at the top of the login window
/// - `request_attributes`: The list of attributes you want to obtain about the user
/// - `wish_attributes`: Almost the same as `request_attributes`, except that is not an absolute requirement. If one the attributes in the wish list is sensitive, the user will be asked if he want to give out the value, he can refuse, in which case the value will not be set by the server
/// - `require`: The filter you want to impose on the user's attributes. It is a parenthesized boolean expression with atomic members of the form : attr1=value1, or attr1. In the former case, attribute attr1 of the user must have the value value1 among its set of values (remember that attributes can be multi valued). In the latter form, attribute attr1 must be present and not null 
/// - `allow`: In a certain sense, this the contrary of `require`. By default, the Tequila server impose default restrictions on certain attributes values. Using allows can lift some (or all) of these restrictions
/// - `language`: Language to use in the interaction with the user (login window, errors, ...). The default server's language is set in the server's configuration files. The user will still be able to change the language
pub async fn create_request(
    return_url: Url,
    service_name: String,
    request_attributes: Vec<String>,
    wish_attributes: Vec<String>,
    require: Option<String>,
    allow: Option<String>,
    language: Option<String>,
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
            if let Some(require) = require {
                vec.push(("require", require))
            }
            if let Some(allow) = allow {
                vec.push(("allow", allow))
            }
            if let Some(language) = language {
                vec.push(("language", language))
            }

            vec
        })
        .await?
        .key,
    )
}

/// Fetches the attributes of the user which logged in using the request key. This method can only be called once on the same request. `auth_check` is the token in the url where the user was redirected
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

/// Wrapper for the whole procedure. It uses typestate to enforce that the calls are made in the right order:
/// - WaitingLogin: The request was created, but the `auth_check` was not supplied. At this point, you may use the [key](TequilaRequest::key) and [fetch_attributes](TequilaRequest::fetch_attributes) methods
/// - LoggedIn: The login was completed. The [attributes](TequilaRequest::attributes) is available to recover the attributes fetched
pub struct TequilaRequest<A, S>
where
    A: FromTequilaAttributes,
{
    key: String,
    attributes: Option<A>,
    _state: PhantomData<S>,
}

/// State of [TequilaRequest]
pub struct WaitingLogin;
/// State of [TequilaRequest]
pub struct LoggedIn;

impl TequilaRequest<(), ()> {
    /// Create a new request handler in the `WaitingLogin` state, with the given `return_url` and `service_name`
    pub async fn new<A>(
        return_url: Url,
        service_name: String,
    ) -> Result<TequilaRequest<A, WaitingLogin>, TequilaError> // TODO add allow and require
    where
        A: FromTequilaAttributes,
    {
        Ok(TequilaRequest {
            key: create_request(
                return_url,
                service_name,
                A::requested_attributes(),
                A::wished_attributes(),
                None,
                None,
                None,
            )
            .await?,
            attributes: None,
            _state: PhantomData,
        })
    }
}

impl<A> TequilaRequest<A, WaitingLogin>
where
    A: FromTequilaAttributes,
{
    /// Returns the request's key. The user must log in at `TEQUILA_URL/auth?requestkey={key}`. Must be in `WaitingLogin` state
    pub fn key(&self) -> &str {
        &self.key
    }

    /// Fetches the attributes with the auth_check provided. If it succeeds, returns a `TequilaRequest<LoggedIn>`. Must be in `WaitingLogin` state
    pub async fn fetch_attributes(
        self,
        auth_check: String,
    ) -> Result<TequilaRequest<A, LoggedIn>, TequilaError> {
        Ok(TequilaRequest {
            key: self.key.clone(),
            attributes: Some(fetch_attributes(self.key, auth_check).await?),
            _state: PhantomData::<LoggedIn>,
        })
    }
}

impl<A> TequilaRequest<A, LoggedIn>
where
    A: FromTequilaAttributes,
{
    /// Get the user's attributes. Must be in `LoggedIn` state
    pub fn attributes(&self) -> &A {
        self.attributes.as_ref().unwrap()
    }
}
