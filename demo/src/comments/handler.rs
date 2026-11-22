use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};

use super::model::{Comment, CreateComment};
use super::repo;
use crate::auth::extractor::AuthUser;
use crate::error::{AppError, AppResult};
use crate::extract::{Path, ValidatedJson};
use crate::posts::repo as posts_repo;
use crate::response::Data;
use crate::state::AppState;

/// GET /api/posts/{id}/comments — 댓글 목록. 인증 불필요.
#[utoipa::path(
    get,
    path = "/api/posts/{id}/comments",
    tag = "comments",
    params(("id" = i64, Path, description = "게시글 id")),
    responses(
        (status = 200, description = "댓글 목록 (오래된 순)", body = Data<Vec<Comment>>),
        (status = 404, description = "없는 게시글 (NOT_FOUND)"),
    )
)]
pub async fn list(
    State(state): State<AppState>,
    Path(post_id): Path<i64>,
) -> AppResult<Json<Data<Vec<Comment>>>> {
    // 글이 없으면 "댓글 0개"가 아니라 404여야 한다. 클라이언트가 둘을 구분할 수 있어야 하기 때문이다.
    ensure_post_exists(&state, post_id).await?;

    let rows = repo::list_by_post(&state.db, post_id).await?;
    Ok(Json(Data::new(rows.into_iter().map(Comment::from).collect())))
}

/// POST /api/posts/{id}/comments — 댓글 작성. **인증 필요**
#[utoipa::path(
    post,
    path = "/api/posts/{id}/comments",
    tag = "comments",
    params(("id" = i64, Path, description = "게시글 id")),
    request_body = CreateComment,
    security(("bearer_auth" = [])),
    responses(
        (status = 201, description = "작성됨 (Location은 글의 댓글 목록)", body = Data<Comment>),
        (status = 401, description = "인증 필요 (UNAUTHORIZED)"),
        (status = 404, description = "없는 게시글 (NOT_FOUND)"),
        (status = 400, description = "검증 실패 (VALIDATION_ERROR)"),
    )
)]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(post_id): Path<i64>,
    ValidatedJson(body): ValidatedJson<CreateComment>, // 본문 extractor는 마지막
) -> AppResult<impl IntoResponse> {
    ensure_post_exists(&state, post_id).await?;

    let row = repo::insert(&state.db, post_id, auth.id, &body.body).await?;

    Ok((
        StatusCode::CREATED,
        [(
            axum::http::header::LOCATION,
            format!("/api/posts/{post_id}/comments"),
        )],
        Json(Data::new(Comment::from(row))),
    ))
}

/// DELETE /api/comments/{id} — 댓글 삭제. **작성자만**
#[utoipa::path(
    delete,
    path = "/api/comments/{id}",
    tag = "comments",
    params(("id" = i64, Path, description = "댓글 id")),
    security(("bearer_auth" = [])),
    responses(
        (status = 204, description = "삭제됨"),
        (status = 403, description = "본인 댓글이 아님 (FORBIDDEN)"),
        (status = 404, description = "없는 댓글 (NOT_FOUND)"),
    )
)]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> AppResult<StatusCode> {
    let row = repo::find(&state.db, id)
        .await?
        .ok_or_else(|| AppError::not_found("comment", id))?;

    // 인증(누구인가)과 인가(이 댓글의 주인인가)는 다른 문제다.
    if row.author_id != auth.id {
        return Err(AppError::Forbidden(
            "본인이 작성한 댓글만 삭제할 수 있습니다".into(),
        ));
    }

    repo::delete(&state.db, id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn ensure_post_exists(state: &AppState, post_id: i64) -> AppResult<()> {
    if posts_repo::exists(&state.db, post_id).await? {
        Ok(())
    } else {
        Err(AppError::not_found("post", post_id))
    }
}
