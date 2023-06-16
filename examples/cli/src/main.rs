use clap::Parser;
use tequila::{FromTequilaAttributes, TequilaRequest, TEQUILA_URL};
use url::Url;

#[derive(Parser, Debug)]
enum Args {
    CreateRequest { return_url: String },
    FetchAttributes { key: String, auth_check: String },
    Login { return_url: String },
}

#[derive(FromTequilaAttributes, Debug)]
struct Attributes {
    #[tequila("uniqueid")]
    sciper: String,
    username: String,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    match args {
        Args::CreateRequest { return_url } => {
            println!("Creating request to {return_url}");
            let key = tequila::create_request(
                Url::parse(&return_url).expect("Invalid url"),
                "Tequila CLI example".into(),
                vec!["uniqueid".into(), "username".into()],
                Vec::new(),
                None,
                None,
                None,
            )
            .await
            .expect("Unable to fetch request key");
            println!("Your request key is: {key}");
            println!("{TEQUILA_URL}/auth?requestkey={key}")
        }
        Args::FetchAttributes { key, auth_check } => {
            println!(
                "{:#?}",
                tequila::fetch_attributes::<Attributes>(key, auth_check)
                    .await
                    .expect("Could not fetch attributes")
            )
        }
        Args::Login { return_url } => {
            let req = TequilaRequest::new::<Attributes>(
                Url::parse(&return_url).expect("Invalid url"),
                "Tequila CLI example".into(),
            )
            .await
            .expect("Could not create request");
            println!(
                "Login to {TEQUILA_URL}/auth?requestkey={} and input the auth_check",
                req.key()
            );

            let mut auth_check = String::new();
            std::io::stdin()
                .read_line(&mut auth_check)
                .expect("Could not read from stdin");

            let req = req
                .fetch_attributes(auth_check)
                .await
                .expect("Could not fetch attributes");

            println!(
                "Hi, {} ({})",
                req.attributes().username,
                req.attributes().sciper
            )
        }
    }
}
