# Rust Board — 최종 프로젝트 (studyFinal)

`demo/`에서 배운 개념을 **구조를 바꿔서** 다시 조립한 미니 게시판 웹사이트입니다. 설명은 [../studyFinal.md](../studyFinal.md)입니다.

- **Cargo 워크스페이스** — `domain`(비즈니스 규칙) / `web`(HTTP 표현) 크레이트 분리
- **SeaORM** — `demo/`의 sqlx(쿼리 직접 작성) 대신 ORM. **DB 종류는 `DATABASE_URL`의 스킴으로** 정해진다 (SQLite · PostgreSQL · MariaDB 드라이버 포함, 실제 연결 검증)
- **HttpOnly 쿠키 세션** — `demo/`의 Bearer 헤더 대신 브라우저 친화적 인증 (Bearer 헤더도 받는다)
- **정적 프론트엔드 내장** — 빌드 도구 없이 `cargo run` 하나로 사이트 전체가 뜸
- **Docker** — 웹 + PostgreSQL을 `docker compose`로

## 실행

```powershell
cargo run -p web
# http://127.0.0.1:4000
```

환경변수는 [.env.example](.env.example) 참고. 아무것도 설정하지 않아도 개발용 기본값으로 동작합니다(개발용 `JWT_SECRET`이면 경고가 출력됩니다).

DB 바꾸기 (코드 변경 없음):

```powershell
$env:DATABASE_URL = "postgres://postgres:postgres@127.0.0.1:55432/board"   # PostgreSQL
$env:DATABASE_URL = "mysql://root:pw@127.0.0.1:53306/board"                # MariaDB
cargo run -p web
```

### Docker (웹 + PostgreSQL)

```powershell
$env:JWT_SECRET = python -c "import secrets;print(secrets.token_urlsafe(48))"
docker compose up -d --build        # http://127.0.0.1:4000  (WEB_PORT로 바꿀 수 있다)
docker compose down                 # 데이터 볼륨은 남는다 (-v를 붙이면 지운다)
```

`JWT_SECRET`이 없으면 컨테이너 시작을 거부합니다. HTTPS 프록시 뒤에서는 `SECURE_COOKIE=true`.

## 테스트

```powershell
cargo test --workspace
#   domain  서비스 8 (소유권·이메일/닉네임 충돌·검색 대소문자·페이지·정렬·댓글 cascade) + 유틸 1
#   web     HTTP 표현 20 (스펙 v1 경로·응답·오류 모양, 쿠키 인증, 429) + 단위 6
```

도메인 크레이트가 HTTP를 모르기 때문에 서버를 띄우지 않고 서비스 함수를 직접 시험합니다. web 크레이트는 `tower::ServiceExt::oneshot`으로 라우터에 요청을 직접 넣습니다.

## 구조

```
project/
├─ Cargo.toml              ← 워크스페이스 정의 + 공통 의존성 버전
├─ Dockerfile  compose.yaml  .dockerignore
├─ crates/
│  ├─ domain/              ← HTTP를 전혀 모르는 크레이트
│  │  ├─ src/
│  │  │  ├─ entity/        ← SeaORM 엔티티 (posts, users, comments)
│  │  │  ├─ migration/     ← Rust 코드로 쓴 마이그레이션 (DB 공통)
│  │  │  ├─ service/       ← 비즈니스 규칙 (소유권 검사 포함): user, post, comment
│  │  │  ├─ error.rs       ← DomainError (StatusCode 없음!)
│  │  │  └─ password.rs
│  │  └─ tests/services.rs ← 서비스 시험
│  └─ web/                 ← axum 라이브러리 + 바이너리
│     ├─ src/
│     │  ├─ api/           ← 라우터 + 핸들러 (users, posts, comments)
│     │  ├─ auth.rs        ← JWT + 쿠키 + AuthUser extractor
│     │  ├─ dto.rs         ← 요청/응답 타입 (camelCase, { data, meta })
│     │  ├─ error.rs       ← DomainError → HTTP 번역 계층 (스펙 v1 오류 본문)
│     │  ├─ middleware.rs  ← 요청 ID, 오류 본문 마무리(requestId), 패닉 처리
│     │  ├─ ratelimit.rs   ← 429 RATE_LIMITED
│     │  ├─ lib.rs         ← app() — main과 테스트가 공유
│     │  └─ main.rs
│     └─ tests/api.rs      ← HTTP 표현 시험
└─ static/                 ← vanilla JS SPA (index.html / app.js / style.css)
```

## API — 공통 API 스펙 v1

[공통 API 스펙 v1](https://app.notion.com/p/3d6d7dec5e2e8139adbfd36dd506c65e)의 §1 표와 줄 단위로 대응합니다.

| 메서드 | 경로 | 인증 | 성공 |
|---|---|---|---|
| GET | `/health` | — | 200 `{ "status": "ok" }` (접두어 없이) |
| POST | `/api/auth/signup` | — | 201 `{ data: { id, email, nickname, createdAt } }` |
| POST | `/api/auth/login` | — | 200 `{ data: { user, accessToken } }` (+ 세션 쿠키) |
| POST | `/api/auth/logout` | 🔒 | 204 (쿠키 삭제) |
| GET | `/api/auth/me` | 🔒 | 200 `{ data: user }` |
| GET | `/api/posts?q=&page=&limit=&sort=` | — | 200 `{ data: [...], meta: { page, limit, total, totalPages, q } }` |
| GET | `/api/posts/{id}` | — | 200 `{ data: post }` |
| POST | `/api/posts` | 🔒 | 201 + `Location: /api/posts/{id}` |
| PATCH | `/api/posts/{id}` | 🔒 본인 글만 | 200 |
| DELETE | `/api/posts/{id}` | 🔒 본인 글만 | 204 |
| GET | `/api/posts/{id}/comments` | — | 200 `{ data: [...] }` |
| POST | `/api/posts/{id}/comments` | 🔒 | 201 |
| DELETE | `/api/comments/{id}` | 🔒 작성자만 | 204 |

- 글: `{ id, title, body, tags, author: { id, nickname }, commentCount, createdAt, updatedAt }`
- `limit` 기본 10, **50을 넘기면 50으로 줄인다.** `sort`는 `createdAt` · `-createdAt`(기본) · `-commentCount`
- 오류: `{ error: { code, message, fields, requestId } }` — `VALIDATION_ERROR`·`BAD_REQUEST`(400), `UNAUTHORIZED`·`INVALID_CREDENTIALS`(401), `FORBIDDEN`(403), `NOT_FOUND`(404), `EMAIL_TAKEN`·`CONFLICT`(409), `RATE_LIMITED`(429), `INTERNAL`(500). `requestId`는 응답 헤더 `X-Request-Id`와 같은 값
- 인증: `Authorization: Bearer <accessToken>` 또는 같은 도메인의 HttpOnly 쿠키 `session` (스펙 §6). 응답 본문 형식은 둘 다 같다
- 호출 제한: IP당 분당 `RATE_LIMIT_PER_MINUTE`(기본 300)회. 초과하면 429 + `Retry-After`
- 옛 DB(닉네임 대신 `username`을 쓰던 시절)는 시작할 때 마이그레이션 `m0002_spec_v1`이 컬럼 이름을 바꾸고 `tags`·`comments`를 추가합니다. 데이터는 그대로 남습니다.
