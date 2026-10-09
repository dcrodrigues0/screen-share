use axum::{
    routing::get,
    Router,
};
use tracing::{info, Level};
use tracing_subscriber;
use dotenvy::dotenv;
use sqlx::postgres::PgPoolOptions;


#[tokio::main]
async fn main() {
    dotenv().ok();
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
}