//! 댓글 API. **공통 API 스펙 v1 §1**

use axum::{
    Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::IntoResponse,
};
use domain::service::comment as comment_service;

use crate::auth::AuthUser;
use crate::dto::{CommentResponse, CreateCommentRequest, Data};
use crate::error::ApiResult;
use crate::extract::ValidatedJson;
use crate::state::AppState;

/// GET /api/posts/{id}/comments — 로그인 없이 볼 수 있다. 없는 글이면 404.
pub async fn list(
    State(state): State<AppState>,
    Path(post_id): Path<i32>,
) -> ApiResult<Json<Data<Vec<CommentResponse>>>> {
    let comments = comment_service::list(&state.db, post_id).await?;
    Ok(Json(Data::new(
        comments.into_iter().map(CommentResponse::from).collect(),
    )))
}

/// POST /api/posts/{id}/comments — 로그인 필요. 201
pub async fn create(
    State(state): State<AppState>,
    user: AuthUser,
    Path(post_id): Path<i32>,
    ValidatedJson(body): ValidatedJson<CreateCommentRequest>,
) -> ApiResult<impl IntoResponse> {
    let created = comment_service::create(&state.db, post_id, user.id, &body.body).await?;

    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/posts/{post_id}/comments"))],
        Json(Data::new(CommentResponse::from(created))),
    ))
}

/// DELETE /api/comments/{id} — 작성자만. 204
pub async fn delete(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<i32>,
) -> ApiResult<StatusCode> {
    comment_service::delete(&state.db, id, user.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
