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
        .route("/register", post(handler::register))
        .route("/login", post(handler::login))
        .route("/me", get(handler::me))
}
