use clap::Parser;
use tequila::FromTequilaAttributes;
use url::Url;

#[derive(Parser, Debug)]
enum Args {
    CreateRequest { return_url: String },
    FetchAttributes { key: String, auth_check: String },
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
                String::new(),
                String::new(),
                "en".into(),
            )
            .await
            .expect("Unable to fetch request key");
            println!("Your request key is: {key}");
            println!("https://tequila.epfl.ch/cgi-bin/tequila/auth?requestkey={key}")
        },
        Args::FetchAttributes {key, auth_check} => {
            println!("{:#?}", tequila::fetch_attributes::<Attributes>(key,auth_check).await.expect("Could not fetch attributes"))
        }
    }
}
