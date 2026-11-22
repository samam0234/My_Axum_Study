//! study2 — 핵심 구조: 모듈 · 라우터 · 공유 상태.
//!
//! 실행: `cargo run --example s02_modules`  →  http://127.0.0.1:3002
//!
//! Axum의 "기본 단위"는 클래스도 컨트롤러도 아니다.
//!   핸들러(async fn)  →  Router(경로 묶음)  →  모듈(Router를 돌려주는 함수)
//! 이 세 겹이 전부다. 한 파일 안에 `mod`로 나눠서 경계가 어떻게 생기는지 본다.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::Serialize;

/// 모든 모듈이 공유하는 상태. `Clone`이 반드시 필요하다 (요청마다 복제되어 핸들러에 들어간다).
#[derive(Clone)]
struct AppState {
    app_name: Arc<String>,
}

#[tokio::main]
async fn main() {
    let state = AppState {
        app_name: Arc::new("lab-s02".to_string()),
    };

    let app = Router::new()
        .route("/", get(|| async { "모듈 구조 예제: /users/1, /posts/1" }))
        // 각 모듈이 자기 Router를 만들어 돌려주고, 여기서 접두어를 붙여 합친다.
        .nest("/users", users::router())
        .nest("/posts", posts::router())
        // 상태는 가장 바깥에서 한 번만 주입한다. Router<AppState> → Router<()>
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3002").await.unwrap();
    println!("http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

/// users 모듈 — 사용자 관련 라우트와 **다른 모듈이 부를 수 있는 함수**를 가진다.
mod users {
    use super::*;

    #[derive(Serialize)]
    pub struct User {
        pub id: u32,
        pub name: &'static str,
    }

    /// 모듈 간 호출은 그냥 함수 호출이다. HTTP를 거치지 않는다.
    /// 다른 모듈의 기능을 쓰려면 `users::find(1)`처럼 부르면 끝이다. 주입 설정이나 컨테이너가 없다.
    pub fn find(id: u32) -> Option<User> {
        match id {
            1 => Some(User { id: 1, name: "mina" }),
            2 => Some(User { id: 2, name: "joon" }),
            _ => None,
        }
    }

    /// 반환 타입이 `Router<AppState>`: "상태를 아직 못 받은 라우터"라는 뜻이다.
    pub fn router() -> Router<AppState> {
        Router::new().route("/{id}", get(get_user))
    }

    async fn get_user(
        State(state): State<AppState>,
        Path(id): Path<u32>,
    ) -> Result<Json<User>, StatusCode> {
        println!("[{}] GET /users/{id}", state.app_name);
        find(id).map(Json).ok_or(StatusCode::NOT_FOUND)
    }
}

/// posts 모듈 — users 모듈의 함수를 불러서 작성자 이름을 붙인다.
mod posts {
    use super::*;

    #[derive(Serialize)]
    struct PostView {
        id: u32,
        title: &'static str,
        /// 다른 모듈이 준 값을 응답에 합친다.
        author: Option<&'static str>,
    }

    pub fn router() -> Router<AppState> {
        Router::new().route("/{id}", get(get_post))
    }

    async fn get_post(Path(id): Path<u32>) -> Result<Json<PostView>, StatusCode> {
        // 글 1번의 작성자는 사용자 1번이라고 가정한다.
        let (title, author_id) = match id {
            1 => ("첫 글", 1),
            2 => ("두 번째 글", 2),
            _ => return Err(StatusCode::NOT_FOUND),
        };

        Ok(Json(PostView {
            id,
            title,
            // props 대신 "함수 인자와 반환값"으로 데이터를 주고받는다.
            author: users::find(author_id).map(|u| u.name),
        }))
    }
}
