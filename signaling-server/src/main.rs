use std::{collections::HashMap, env};

use serde::{Deserialize, Serialize};
use firebase_rs::Firebase;
use dotenvy::dotenv;

#[derive(Serialize, Deserialize, Debug)]
struct Offer{
    sdp: String,
    sdp_type: String,
}

async fn create_offer(firebase_client: &Firebase, offer: &Offer)  -> HashMap<String, Offer> {
    firebase_client.set(&offer).await;
    let offers = firebase_client.get::<HashMap<String, Offer>>().await;
    offers.unwrap()
}

/*
async fn get_offer(firebase_client: &Firebase, offer_id: &str) -> HashMap<String, Offer> {
    
}

 */
fn string_to_offer(s: &str) -> Offer {
    serde_json::from_str(s).unwrap()
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    let database_url:String = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let firebase_client: Firebase = Firebase::new(&database_url).unwrap().at("offers");
    
    let offer = Offer {
        sdp: String::from("sdp_value_test"),
        sdp_type: String::from("sdpt_type_value_test"),
    };

    let offer = create_offer(&firebase_client, &offer).await;
    println!("Offers: {:?}", offer);
}