use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use serde_json::json;

use super::model::{CreatePost, Post, UpdatePost};
use crate::extract::ValidatedJson;
use crate::state::AppState;

/// GET /posts — 전체 목록 (최신순)
pub async fn list(State(state): State<AppState>) -> Json<Vec<Post>> {
    let posts = state.posts().await;
    // 읽기 락을 잡은 채로 복제해서 내보낸다. 락 가드는 이 함수가 끝나며 자동 해제된다.
    let mut items = posts.clone();
    items.sort_by(|a, b| b.id.cmp(&a.id));
    Json(items)
}

/// GET /posts/{id} — 단건 조회
pub async fn get_one(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<Json<Post>, (StatusCode, Json<serde_json::Value>)> {
    let posts = state.posts().await;

    posts
        .iter()
        .find(|p| p.id == id)
        .cloned()
        .map(Json)
        .ok_or_else(|| not_found(id))
}

/// POST /posts — 생성 (201 Created + Location 헤더)
///
/// `Json<CreatePost>` 대신 `ValidatedJson<CreatePost>`를 쓴다.
/// 핸들러 본문은 하나도 안 바뀌었지만, 이제 값 검증까지 통과한 데이터만 들어온다.
pub async fn create(
    State(state): State<AppState>,
    ValidatedJson(body): ValidatedJson<CreatePost>, // 본문 extractor는 반드시 마지막 인자
) -> impl IntoResponse {
    let now = Utc::now();
    let post = Post {
        id: state.next_id(),
        title: body.title,
        content: body.content,
        author: body.author,
        tags: body.tags,
        created_at: now,
        updated_at: now,
    };

    let location = format!("/posts/{}", post.id);
    state.posts_mut().await.push(post.clone());

    (
        StatusCode::CREATED,
        [(axum::http::header::LOCATION, location)],
        Json(post),
    )
}

/// PATCH /posts/{id} — 부분 수정
pub async fn update(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    ValidatedJson(body): ValidatedJson<UpdatePost>,
) -> Result<Json<Post>, (StatusCode, Json<serde_json::Value>)> {
    let mut posts = state.posts_mut().await;

    let post = posts
        .iter_mut()
        .find(|p| p.id == id)
        .ok_or_else(|| not_found(id))?;

    // Option의 Some일 때만 덮어쓴다 = 부분 수정
    if let Some(title) = body.title {
        post.title = title;
    }
    if let Some(content) = body.content {
        post.content = content;
    }
    if let Some(tags) = body.tags {
        post.tags = tags;
    }
    post.updated_at = Utc::now();

    Ok(Json(post.clone()))
}

/// DELETE /posts/{id} — 삭제 (204 No Content)
pub async fn delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let mut posts = state.posts_mut().await;

    let before = posts.len();
    posts.retain(|p| p.id != id);

    if posts.len() == before {
        Err(not_found(id))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

/// 404 응답을 만드는 임시 헬퍼.
/// 핸들러마다 이 튜플 타입을 적는 게 번거롭다는 걸 곧 느끼게 되는데,
/// study4에서 `AppError` 하나로 정리한다.
fn not_found(id: i64) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found", "message": format!("post {id} not found") })),
    )
}
