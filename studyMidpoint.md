# studyMidpoint — 중간 응용: 게시판 · 로그인 · CRUD 서비스

> 🎯 **목표**: study0~10에서 배운 것**만으로** Axum 하나로 작은 서비스(회원가입·로그인·게시판 CRUD)를 처음부터 끝까지 조립하는 순서를 정리한다. 새로운 기술은 쓰지 않고, 기초 구간이 **한 덩어리로 맞물려 돌아가는지** 확인한다.

| 항목 | 내용 |
|---|---|
| 결과물 | [demo/](demo/) — 게시판 API 서버 (포트 3000, Swagger UI `/docs`) |
| 사용한 단계 | study0~10 전부. 그 이후(DB 교체·외부 서비스·AI·CI/CD)는 쓰지 않는다 |
| 검증 | `cargo test` (단위 7 + 통합 15) 통과 · Docker 이미지 빌드/실행/헬스체크/영속성/SIGTERM 종료 확인 (study10) |

---

## ① 목표

1. **만들 것을 한 문장으로**: "로그인한 사용자가 글을 쓰고, 누구나 읽고, 본인 글만 고치고 지우는 게시판 API."
2. 기능을 **목록으로 고정**하고, 각 기능이 어느 study에서 온 것인지 대응시킨다.
3. 폴더 구조와 구현 순서를 따라 **빈 폴더에서 배포 가능한 상태까지** 갈 수 있다.
4. 1차 배포 체크리스트로 "됐다"의 기준을 정하고, 회고로 다음 구간(study11~18)을 준비한다.

---

## ② 📖 이론 — 기능 목록과 설계 결정

### 기능 목록

| # | 기능 | 인증 | 경로 | 성공 | 대표 오류 | 근거 단계 |
|---|---|---|---|---|---|---|
| F1 | 상태 확인 | — | `GET /health` | 200 | — | 1 |
| F2 | 회원가입 | — | `POST /auth/register` | 201 | 400 검증, 409 `EMAIL_TAKEN` | 5, 7 |
| F3 | 로그인 (JWT 발급) | — | `POST /auth/login` | 200 | 401 `INVALID_CREDENTIALS` | 7 |
| F4 | 내 정보 | 필요 | `GET /auth/me` | 200 | 401 | 7 |
| F5 | 글 목록 (검색·태그·페이지) | — | `GET /posts?q=&tag=&page=&per_page=` | 200 | 400 (범위 초과) | 4, 6 |
| F6 | 글 상세 | — | `GET /posts/{id}` | 200 | 404 | 6 |
| F7 | 글 작성 | 필요 | `POST /posts` | 201 + `Location` | 400, 401 | 5, 6, 7 |
| F8 | 글 수정 (부분) | **작성자** | `PATCH /posts/{id}` | 200 | 401, **403**, 404 | 6, 7 |
| F9 | 글 삭제 | **작성자** | `DELETE /posts/{id}` | 204 | 401, 403, 404 | 6, 7 |
| F10 | 썸네일 업로드 | **작성자** | `POST /posts/{id}/thumbnail` (multipart) | 200 | 400, 403, 413 | 8 |
| F11 | API 문서 | — | `GET /docs` (Swagger UI) | 200 | — | 10 |

비기능 요구: 모든 오류는 `{ error: { code, message, fields?, requestId } }` · 요청마다 `X-Request-Id` · 응답 JSON camelCase · 느린 요청 5초 후 408 · 패닉도 500 JSON · 본문 2MB 제한 · 비밀번호는 Argon2 해시 · 설정은 환경변수.

### 설계 결정 (왜 이렇게?)

| 결정 | 이유 |
|---|---|
| 단일 크레이트(`demo/`), lib + bin | 작은 서비스에서는 가장 단순. 테스트가 같은 `app()`을 쓴다 (study8) |
| sqlx + SQLite (SQL 직접 작성) | 어떤 쿼리가 나가는지 100% 보인다. 파일 하나라 배포가 쉽다 (study6) |
| JWT Bearer 인증 | 서버가 세션을 기억하지 않는다. 프론트·앱 어디서나 쓴다 (study7) |
| 작성자는 토큰에서 | 클라이언트가 보낸 `author`를 믿지 않는다. `deny_unknown_fields` + `AuthUser` |
| 3-타입 구조 (`CreatePost` / `PostRow` / `Post`) | 입력·DB·응답의 모양이 달라서 섞지 않는다 |
| 오류는 `AppError` 하나 | `?`로 흘리고 한 곳에서 HTTP로 번역 (study5, 6) |

---

## ③ 🛠️ 실기

### 3-1. 폴더 구조

```
demo/
├─ Cargo.toml  Cargo.lock           ← 의존성·feature(sqlite/postgres)·release 프로필
├─ .env.example                      ← 환경변수 키 목록 (.env는 커밋하지 않는다)
├─ Dockerfile  .dockerignore  compose.yaml  compose.postgres.yaml
├─ migrations/        0001_create_posts.sql  0002_create_users.sql  0003_add_thumbnail.sql
├─ migrations_pg/     (PostgreSQL용, study11)
├─ src/
│  ├─ main.rs         설정 읽기 → DB 연결 → 서버 실행 (+ SIGTERM 종료)
│  ├─ lib.rs          app(): 라우터 + 미들웨어 조립  ← 테스트도 이걸 쓴다
│  ├─ config.rs       Config::from_env() / for_test() / 검증 / 비밀번호 가리기
│  ├─ db.rs           Db/DbPool 별칭, 풀 생성, 마이그레이션
│  ├─ state.rs        AppState { db, jwt_secret, upload_dir }
│  ├─ error.rs        AppError + ErrorEnvelope (스펙 v1 오류 본문)
│  ├─ extract.rs      ValidatedJson / ValidatedQuery / Path / Query
│  ├─ middleware.rs   요청 ID, 응답 시간, 패닉→500, error_envelope
│  ├─ pagination.rs   PageQuery / Page<T> (+ 단위 테스트)
│  ├─ upload.rs       multipart 이미지 저장 (경로 조작 방어)
│  ├─ openapi.rs      utoipa OpenAPI 정의
│  ├─ routes.rs       health, 데모 라우트
│  ├─ posts/          mod(router) · handler · model · repo
│  └─ auth/           mod · handler · model · repo · jwt · password · extractor
├─ tests/api.rs       oneshot 통합 테스트 15개
└─ uploads/.gitkeep
```

### 3-2. 단계별 구현 순서

> 이 순서대로 만들면 **각 단계가 끝날 때마다 실행 가능**하다. (저장소의 커밋 기록도 같은 순서를 따라간다. 커밋 메시지의 study 번호는 처음 만들 때의 번호라서 이 문서의 번호와 다르다 — README의 대응표 참고)

| 순서 | 할 일 | 만들어지는 것 | 확인 방법 | 단계 |
|---|---|---|---|---|
| 1 | 툴체인·프로젝트·`.gitignore`·`.env.example` | `cargo new demo` | `cargo run` → `Hello, Axum!` | 0, 1 |
| 2 | 라우터와 `/health`, 경로·쿼리 extractor | `routes.rs` | `curl /health` | 1, 4 |
| 3 | `AppState`, 모듈 분리 (`posts/`) | `state.rs`, `posts/mod.rs` | `nest("/posts")` | 2 |
| 4 | **메모리 CRUD** (`Arc<RwLock<Vec>>`) | `posts/handler.rs` 초안 | curl로 CRUD, 재시작하면 사라짐 | 3 |
| 5 | DTO + `validator` + `ValidatedJson` | `model.rs`, `extract.rs` | 빈 제목 → 400 | 5 |
| 6 | `AppError` + `IntoResponse`, 404/405 fallback | `error.rs` | 오류가 전부 같은 JSON | 5, 6 |
| 7 | **DB 연결 + 마이그레이션 + repo** (메모리 → SQLite) | `db.rs`, `migrations/`, `repo.rs` | 재시작해도 글이 남는다 | 6 |
| 8 | 회원 테이블, Argon2, JWT, `AuthUser` | `auth/` | 토큰 없이 쓰기 → 401 | 7 |
| 9 | 소유권 검사 (`ensure_owner`) | `posts/handler.rs` | 남의 글 수정 → 403 | 7 |
| 10 | 미들웨어 계층 (요청 ID, 로깅, 패닉, 타임아웃, 압축, CORS) | `middleware.rs`, `lib.rs` | `x-request-id` 헤더 | 8 |
| 11 | 페이지네이션·검색 (`QueryBuilder`), 썸네일 업로드 | `pagination.rs`, `upload.rs` | `?q=&page=` | 6, 8 |
| 12 | 설정 분리 (`Config`), lib+bin, 시작 시 검증 | `config.rs`, `main.rs` | 짧은 비밀키 → 부팅 거부 | 8 |
| 13 | 테스트 (단위 + `oneshot`) | `tests/api.rs` | `cargo test` 전부 통과 | 9 |
| 14 | 스펙 v1 맞춤 (오류 본문 · camelCase · `requestId`) | `error.rs`, `error_envelope` | 오류 JSON 모양 | 6 |
| 15 | OpenAPI/Swagger UI | `openapi.rs` | `/docs` | 10 |
| 16 | Dockerfile · compose · 헬스체크 · SIGTERM | `Dockerfile` | `docker run` → healthy | 10 |

**순서의 원칙**: ① 먼저 **눈에 보이는 것**(`curl`로 확인 가능한 것)을 만든다 → ② 메모리로 모양을 잡고 → ③ DB로 바꾼다 → ④ 보호(인증·인가)를 얹고 → ⑤ 구조·테스트로 굳히고 → ⑥ 패키징한다. DB를 처음부터 붙이면 오류가 "내 코드 문제인지 DB 문제인지" 구분되지 않는다.

### 3-3. 핵심 흐름이 한 번에 도는 모습 (실제 실행)

```
$ curl -X POST $B/auth/register -d '{"username":"alice","email":"alice@example.com","password":"supersecret123"}'
{"id":1,"username":"alice","email":"alice@example.com","createdAt":"2026-11-22T15:12:17.395524Z"}        201

$ curl -X POST $B/auth/login -d '{"username":"alice","password":"supersecret123"}'
{"accessToken":"eyJ0eXAiOiJKV1Q...","tokenType":"Bearer","expiresIn":3600,"user":{...}}                200

$ curl -X POST $B/posts -H "authorization: Bearer $TOKEN" -d '{"title":"Hello","content":"Rust Axum","tags":["rust","axum"]}'
{"id":1,"title":"Hello","author":"alice","authorId":1,"tags":["rust","axum"],...}                        201 + Location: /posts/1

$ curl "$B/posts?q=hello&tag=RUST"                      →  {"items":[{...}],"page":1,"perPage":10,"total":1,"totalPages":1,"hasNext":false}
$ curl -X PATCH $B/posts/1 -H "authorization: Bearer $TOKEN" -d '{"title":"Updated"}'      → 200 (content는 그대로)
$ curl -X PATCH $B/posts/1 -H "authorization: Bearer $BOB_TOKEN" -d '{"title":"탈취"}'     → 403 FORBIDDEN
$ curl -X POST $B/posts -d '{"title":"","content":""}' -H "authorization: Bearer $TOKEN"    → 400 VALIDATION_ERROR + fields
$ curl -X DELETE $B/posts/1 -H "authorization: Bearer $TOKEN"                               → 204
```

---

## ④ 🔗 연동 포인트

### DB · API 연동 구성도

```
┌──────────────┐  HTTP/JSON   ┌───────────────────────────────────────────────┐   SQL    ┌────────────┐
│ 클라이언트    │ ───────────▶ │  Axum 서버 (demo)                              │ ───────▶ │ SQLite     │
│ curl         │ ◀─────────── │                                               │ ◀─────── │ data.db    │
│ Swagger UI   │   응답        │  미들웨어: RequestId → Trace → 압축 →           │          │ (WAL)      │
│ (프론트)      │              │            error_envelope → CatchPanic →      │          └────────────┘
└──────────────┘              │            Timeout → CORS → BodyLimit         │          ┌────────────┐
                              │                                               │ 파일 쓰기 │ uploads/   │
   Authorization: Bearer ───▶ │  Router ─ extractor(AuthUser, ValidatedJson)  │ ───────▶ │ (볼륨)      │
                              │            ─ handler ─ repo ─ sqlx Pool       │          └────────────┘
                              └───────────────────────────────────────────────┘
        환경변수: DATABASE_URL · JWT_SECRET · BIND_ADDR · UPLOAD_DIR · RUST_LOG  →  Config::from_env()
```

### 프론트가 붙을 때 맞출 계약

공통 API 스펙 v1과의 차이 표는 [study6 ④](study6.md)에 있다. 이 서비스는 **오류 본문·camelCase·요청 ID**는 맞췄고, **경로 접두어(`/api`)·목록 `{data, meta}` 래핑·글 필드 `body`·댓글**은 아직 다르다. 프론트를 붙이는 순간 가장 먼저 바꿀 곳이다 (study12, 14).

### 1차 배포 체크리스트

```
□ 코드        cargo test 전부 통과 (단위 7 + 통합 15), cargo clippy 경고 확인
□ 설정        .env 미커밋 (git status), .env.example 최신, 운영 JWT_SECRET 별도 생성 (≥32자 무작위)
□ 이미지      docker build 성공, 이미지 크기 확인(약 160MB), 비root(appuser), HEALTHCHECK 있음
□ 실행        docker run -e JWT_SECRET=... -v 데이터볼륨 -p 3000:3000 → healthy
□ 시나리오    가입 → 로그인 → 글 작성 → 목록/검색 → 수정 → (남의 글 수정 403) → 삭제
□ 오류 모양   404 / 400 / 401 / 403 / 409 / 500이 모두 { error: { code, message, requestId } }
□ 영속성      컨테이너를 지우고 같은 볼륨으로 다시 띄워도 데이터가 남는다
□ 종료        docker stop 이 1초 안팎(SIGTERM), 로그에 "종료 신호(SIGTERM)"
□ 보안        비밀번호·토큰·DB 비밀번호가 로그에 없다, CORS 허용 출처 확인, HTTPS(프록시) 뒤에서만 공개
□ 문서        /docs(Swagger UI)가 열리고 보호 경로에 자물쇠가 있다
□ 롤백        직전 이미지 태그를 기록했다
```

---

## ⑤ 오류 노트

이 중간 응용을 조립하면서 **단계가 합쳐질 때** 생긴 문제들이다.

| 증상 | 원인 | 해결 |
|---|---|---|
| 각 단계 코드는 되는데 합치니 `Handler is not satisfied` | 인증 extractor(`AuthUser`)를 **본문 extractor보다 뒤에** 둠 / `.with_state()` 누락 | 본문 extractor는 마지막. `#[axum::debug_handler]` |
| 테스트에서만 `no such table` | in-memory SQLite에 풀 연결이 2개 이상 | 메모리 DB는 `max_connections(1)` |
| 로그인은 되는데 글쓰기가 401 | `Authorization: Bearer <토큰>` 형식 오류(공백/`Bearer` 누락), 토큰 만료(1시간) | 헤더 형식 확인, 새 토큰 발급 |
| 오류 JSON에 `requestId`가 안 붙음 | 미들웨어 순서 (`error_envelope`는 `SetRequestId` 안쪽, `Compression` 안쪽, `CatchPanic` 바깥) | study8의 순서표 |
| 컨테이너에서 접속 불가 | `BIND_ADDR=127.0.0.1` | `0.0.0.0:3000` |
| `docker build`가 Swagger UI 다운로드에서 실패 | `utoipa-swagger-ui`의 `build.rs`가 빌드 중 네트워크·curl 사용 | `vendored` feature (study10) |
| `docker stop`이 10초 걸림 | SIGTERM 미처리 | `shutdown_signal`에 SIGTERM 추가 (study10) |
| 데이터가 배포마다 사라짐 | 볼륨 미마운트 | `-v 이름:/app/data` |

---

## ⑥ 직접 해보기

1. **빈 폴더에서 다시 만들기**: 위 구현 순서 1~7번을 새 폴더(`my-board/`)에 직접 따라 쳐 보고, 막힌 지점마다 어느 study의 어느 절을 다시 읽었는지 기록한다. (순서 8 이후는 `demo/`를 참고해도 된다)
2. **기능 하나 추가**: "글 조회수"를 추가한다. 마이그레이션 파일을 **새로** 만들고(`0004_add_views.sql`), `GET /posts/{id}`가 조회수를 1 올리게 하고, 응답에 `views`를 넣고, 시험을 쓴다. 기존 마이그레이션을 건드리지 않았는지 확인한다.
3. **1차 배포 체크리스트를 스크립트로**: 체크리스트의 "시나리오"~"오류 모양"을 `smoke.sh`(curl)로 만들어 새 컨테이너에 돌린다. 이것이 study16 CI의 첫 걸음이다.

---

## ⑦ ☐ 체크리스트

**구현**
- ☐ 기능 목록 F1~F11을 표 없이 말하고, 각각 어느 단계에서 온 것인지 안다
- ☐ 폴더 구조를 보고 각 파일의 역할을 설명할 수 있다
- ☐ 구현 순서 16단계를 따라 **각 단계 끝에 실행 확인**을 했다
- ☐ `cargo test`가 전부 통과한다 (단위 7 + 통합 15)

**1차 배포**
- ☐ 이미지를 빌드하고 컨테이너로 실행해 `healthy`를 확인했다
- ☐ 가입→로그인→글쓰기→수정→(403)→삭제 시나리오가 컨테이너에서 돈다
- ☐ 컨테이너를 지웠다 다시 띄워도 데이터가 남는다
- ☐ `docker stop`이 1초 안팎이다
- ☐ 로그에 비밀번호·토큰이 없다

---

## 회고 (순서대로 쓴다)

회고는 **사실 → 해석 → 행동** 순서로 쓴다. 감상부터 쓰면 다음 행동이 안 나온다.

1. **목표 대비 결과**: F1~F11 중 구현된 것 / 스펙 v1과 아직 다른 것(경로 접두어·`{data, meta}`·글 `body`·댓글).
2. **숫자로 본 결과**: 테스트 22개(1.2초), 릴리스 바이너리 18.5MB, Docker 이미지 160MB, `docker stop` 0.4초, 의존성 컴파일 58초 + 내 코드 23초.
3. **잘 된 것**: 오류를 값으로 다루니 **새 갈래를 추가하면 컴파일러가 누락을 알려 줬다** / extractor로 인증·검증이 시그니처에 드러남 / `oneshot` 테스트가 빠르고 격리됨.
4. **막힌 것과 원인**: 오류의 절반은 **버전 차이**였다. 그리고 **직접 돌려 보지 않아서 늦게 발견된 문제**가 있었다 — Docker 빌드의 `curl` 의존, SIGTERM 미처리, 로그의 DB 비밀번호 노출. 모두 "로컬에서는 됐다"가 함정이었다.
5. **배운 원칙**: study10 총정리의 일곱 가지.
6. **다음 행동 (우선순위)**: ① 프론트를 붙이기 전에 스펙 v1 차이를 줄인다 ② DB를 SQLite 이상으로 확장한다 (study11) ③ 프론트·UI 연동 (study12) ④ 외부 서비스 (13) ⑤ API 설계 마무리 (14) ⑥ 성능 (15) ⑦ CI/CD (16) ⑧ 실시간 (17) ⑨ AI (18).

---

## ⑧ 다음 단계 연결

기초 구간으로 **돌아가는 서비스**가 생겼다. [study11](study11.md)부터는 심화 구간이다: 먼저 이 서비스의 **DB를 SQLite에서 PostgreSQL·MariaDB·MongoDB로 바꿔 보며** 무엇이 바뀌고 무엇이 그대로인지 확인한다.
