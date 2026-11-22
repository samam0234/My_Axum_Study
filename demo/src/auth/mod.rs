pub mod extractor;
pub mod handler;
pub mod jwt;
pub mod model;
pub mod password;
pub mod repo;

use axum::{Router, routing::{get, post}};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/signup", post(handler::signup))
        .route("/login", post(handler::login))
        .route("/logout", post(handler::logout))
        .route("/me", get(handler::me))
}
