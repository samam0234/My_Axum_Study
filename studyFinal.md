# studyFinal — 심화 응용: 서비스 완성

> 🎯 **목표**: study11~18에서 만든 부품과 [studyMidpoint](studyMidpoint.md)의 게시판을 합쳐 **Axum 하나로 서비스를 완성**한다: DB 교체, 외부 서비스 연동, AI 기능 추가, Docker·자동 배포, 테스트·보안 점검. 이 문서는 **무엇이 이미 합쳐져 있고, 무엇이 검증된 부품으로만 존재하며, 무엇이 남았는지**를 정직하게 구분하고, 남은 것을 합치는 순서를 정한다.

| 항목 | 내용 |
|---|---|
| 최종 서비스 | [project/](project/) — **Rust Board**: Cargo 워크스페이스(`domain` / `web`) + SeaORM + HttpOnly 쿠키 세션 + 정적 SPA. 포트 4000 |
| 부품 창고 | [demo/](demo/) (스펙 v1 API, 인증, 테스트, Docker) · [lab/](lab/) (외부 서비스, 실시간, AI, 캐시, 프론트 클라이언트) |
| 이 단계에서 `project/`에 더한 것 | PostgreSQL·MariaDB 드라이버(+실검증), 검색 대소문자·UNIQUE 위반 수정, **공통 API 스펙 v1 정렬**(경로·`{data, meta}`·오류 본문·이메일 로그인·`tags`·댓글·`limit`/`sort`·429), **도메인 서비스 시험 8개 + web HTTP 시험 20개**, `Dockerfile` + `compose.yaml`(웹 + PostgreSQL), SIGTERM 정상 종료, 개발용 시크릿 경고, DB 비밀번호 로그 마스킹 |
| 검증 | `cargo test --workspace` 통과(domain 9 + web 26) · SQLite(옛 DB 업그레이드)·PostgreSQL 17·MariaDB 11에서 같은 스펙 시나리오 · `docker compose up`으로 웹+PostgreSQL 실제 기동 후 가입→로그인→글쓰기→댓글→검색→`docker stop` 확인 |

---

## ① 목표

1. 완성된 서비스의 **기능 목록**과 각 기능의 **구현 상태**(합쳐짐 / 검증된 부품 / 미구현)를 표로 고정한다.
2. 부품을 합치는 **구조 규칙**(어느 크레이트에 무엇을 두는가)을 정한다.
3. 구현 순서와, 각 단계의 **끝을 확인하는 방법**을 정한다.
4. 배포 체크리스트와 테스트·보안 점검표로 "완성"의 기준을 만든다.
5. 최종 회고로 커리큘럼 전체를 정리한다.

---

## ② 📖 이론 — 기능 목록과 구조 규칙

### 기능 목록과 구현 상태

범례: ✅ **`project/`에 합쳐져 있고 실행·시험했다** · 🧩 **부품이 `demo/`·`lab/`에서 실행 검증됨 — `project/`에는 아직 합치지 않음** · ⬜ 미구현

| # | 기능 | 상태 | 어디에 / 근거 |
|---|---|---|---|
| F1 | 회원가입·로그인(이메일)·로그아웃·내 정보 (HttpOnly 쿠키 + Bearer 둘 다) | ✅ | `crates/web/src/auth.rs`, `api/users.rs` · 도메인 시험(중복 이메일/닉네임, 로그인 실패 동일) · web 시험(쿠키 인증) |
| F2 | 글 목록(검색·정렬·페이지)·상세·작성·수정·삭제·**태그**·**댓글**, **소유권 검사는 서비스에서** | ✅ | `crates/domain/src/service/{post,comment}.rs` · 도메인 시험(남의 글/댓글 403, 검색 대소문자, 페이지네이션, 댓글 수 정렬, cascade) |
| F3 | **DB 교체** (SQLite ↔ PostgreSQL ↔ MariaDB, URL만으로) | ✅ | `DATABASE_URL` 스킴. 세 DB 모두 실제 컨테이너로 CRUD·중복 409·검색 확인 (study11) |
| F4 | 웹사이트(SPA) + API를 한 서버가 같은 출처로 서빙 | ✅ | `static/`, `ServeDir.fallback` (컨테이너에서 딥링크 200 확인) |
| F5 | **Docker**: 멀티 스테이지 이미지 + compose(웹 + PostgreSQL) | ✅ | `project/Dockerfile`, `compose.yaml` (이미지 151MB, 비root, healthy, `docker stop` 0.4초) |
| F6 | **CI**: push/PR마다 테스트 + **태그 시 이미지 게시** | ✅ | `.github/workflows/ci.yml`, `docker-publish.yml` (GitHub 실행 결과는 study16 ③-4) |
| F7 | 보안 점검: 비밀번호 해시, 사용자 열거 방지, 쿠키 속성, 시작 시 비밀키 검증·경고, 로그 비밀번호 마스킹 | ✅ | 아래 4-4 점검표 |
| F8 | 공통 API 스펙 v1 정렬 (경로 `/api`·`/health`, 오류 본문 `{ error: { code, … } }`, `requestId`, camelCase, `{data, meta}`, `signup`/이메일 로그인, `tags`, 댓글, `limit`≤50·`sort`) | ✅ | `crates/web/{api,dto,error,middleware}` · web 시험 20 · 세 DB + 컨테이너에서 같은 시나리오 (study6 ④) |
| F9 | **외부 서비스**: 결제 웹훅(HMAC·멱등), 메일(백그라운드), 소셜 로그인(OAuth2 + `state`) | 🧩 | `lab/examples/s13_external.rs` (단위 3 + 흐름 전체 실행) |
| F10 | 파일 업로드(썸네일·대용량 스트리밍) | 🧩 | `demo/src/upload.rs`, `lab/examples/s17_realtime.rs` |
| F11 | **AI 기능**: 문서 요약·근거 기반 질의응답·스트리밍(SSE)·MCP | 🧩 | `lab/examples/s18_ai.rs` (가짜 AI + **실제 Ollama `gemma4:12b`** 검증) |
| F12 | 실시간 알림 (SSE / WebSocket) | 🧩 | `lab/examples/s17_realtime.rs` |
| F13 | 성능: TTL 캐시, 작업 큐, `spawn_blocking` | 🧩 | `lab/examples/s15_cache_jobs.rs` (311ms→0ms, 1.26s→1ms 실측) |
| F14 | 프론트 API 클라이언트 (`ApiError`, 필드 오류, 취소) | 🧩 | `lab/static/api.js` (테스트 9) |
| F15 | 호출 제한(429, IP당 분당 N회) | ✅ | `crates/web/src/ratelimit.rs` · web 시험 (429 + `Retry-After`, `/health`는 제외) |
| F16 | 모니터링 메트릭, JSON 로그, 자동 백업 | ⬜ | study15·16의 "도입 체크리스트" |

**"완성"의 기준**: ✅가 서비스의 **뼈대**(F1~F8, F15)다. 🧩는 합치기만 하면 되는 **검증된 부품**이고, ⬜는 운영 단계에서 더하는 것이다. 🧩를 합치는 순서는 ④의 4-2에 있다.

### 구조 규칙 — 부품을 어디에 꽂나

```
crates/
├─ domain/            HTTP도, 외부 SDK 세부도 모른다. "서비스가 무엇을 하는가"만.
│   ├─ service/       user, post, comment (+ document: AI 기능의 저장·검색 규칙)
│   ├─ entity/        users, posts, comments  (migration m0001 → m0002 스펙 v1)
│   └─ error.rs       DomainError  (StatusCode 없음)
├─ integrations/      (새 크레이트) 바깥 세계와의 경계: ai 클라이언트, 메일, OAuth, 웹훅 검증
│   └─ trait 정의 + reqwest 구현  ← study9의 trait 경계. 테스트에서는 가짜로 교체
└─ web/               axum. 요청을 풀고 → 서비스/통합을 부르고 → 응답으로 옮긴다
    ├─ api/           users, posts, comments, (+ documents, ai, webhooks, oauth)
    ├─ dto.rs         요청/응답 타입 — JSON 모양(camelCase, `{ data, meta }`)은 여기서 정한다. 도메인 타입엔 Serialize가 없다
    ├─ error.rs       DomainError → HTTP 번역 (스펙 v1 모양)
    ├─ middleware.rs  요청 ID · error_envelope(requestId) · 패닉 → 500
    ├─ ratelimit.rs   429
    ├─ lib.rs         app() — main과 통합 시험이 공유
    └─ extract.rs     ValidatedJson, AuthUser …
```

- **의존 방향은 `web → integrations → domain` 한 방향**(또는 `web → domain`, `integrations → domain`). 도메인은 아무도 모른다. 실수로 도메인 코드에 `StatusCode`나 `reqwest`가 섞여 들어가는 일이 **빌드 단계에서** 불가능하다.
- 외부 서비스는 항상 **trait 뒤에** 둔다 → 시험은 가짜로, 운영은 진짜로 (study9).
- 설정은 `web`의 `main`이 한 번 읽어 각 부품에 **값으로** 넘긴다 (부품이 `std::env`를 직접 읽지 않는다).

---

## ③ 🛠️ 실기 — 이미 합쳐진 것을 확인한다

### 3-1. 도메인 서비스 시험 — [project/crates/domain/tests/services.rs](project/crates/domain/tests/services.rs)

HTTP가 없어서 **서버를 띄우지 않고** 규칙을 시험한다. DB는 이름 있는 in-memory SQLite(`cache=shared`)로 시험마다 격리한다.

```rust
async fn test_db() -> DatabaseConnection {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    connect(&format!("sqlite:file:svc_test_{n}?mode=memory&cache=shared")).await.expect("테스트 DB 연결 실패")
}

#[tokio::test]
async fn 남의_글은_수정도_삭제도_못_한다() {
    let db = test_db().await;
    let alice = make_user(&db, "alice").await;
    let bob = make_user(&db, "bob").await;
    let created = post::create(&db, alice, "앨리스의 글", "본문", &[]).await.unwrap();

    let edit = post::update(&db, created.id, bob, Some("탈취"), None, None).await;
    assert!(matches!(edit, Err(DomainError::Forbidden(_))), "{edit:?}");
    let del = post::delete(&db, created.id, bob).await;
    assert!(matches!(del, Err(DomainError::Forbidden(_))), "{del:?}");

    assert_eq!(post::find(&db, created.id).await.unwrap().title, "앨리스의 글");     // 글은 그대로
    let edited = post::update(&db, created.id, alice, Some("수정됨"), None, None).await.unwrap();
    assert_eq!((edited.title.as_str(), edited.body.as_str()), ("수정됨", "본문"));      // 안 보낸 필드는 유지
}

#[tokio::test]
async fn 검색은_대소문자를_구분하지_않는다() {                   // study11에서 PostgreSQL이 깨졌던 것의 회귀 시험
    /* "SeaORM 가이드"를 만들고 q = seaorm / SEAORM / SeaORM / migration 모두 total 1, 없는 단어는 0 */
}
```

| 시험 | 고정하는 약속 |
|---|---|
| `같은_이메일이나_닉네임으로_다시_가입하면_충돌이다` | 이메일 중복 → `EmailTaken`(대소문자만 달라도), 닉네임 중복 → `Conflict` — 스펙의 두 409를 구분 |
| `없는_이메일과_틀린_비밀번호는_같은_오류다` | 둘 다 `InvalidCredentials` — 사용자 열거 방지 (이메일 대소문자는 무시) |
| `남의_글은_수정도_삭제도_못_한다` | **소유권 검사가 서비스 안에 있다** → HTTP 계층이 빠뜨릴 수 없다 |
| `글에는_태그와_작성자_닉네임이_붙는다` | `tags` JSON 저장·교체·유지, JOIN으로 닉네임 |
| `검색은_대소문자를_구분하지_않는다` | DB를 바꿔도 같은 결과 (회귀 시험) |
| `페이지네이션은_1부터_세고_전체_수를_준다` | `limit`/`offset` 계산과 정렬 방향 |
| `댓글_수와_댓글_많은_순_정렬` | 서브쿼리 `commentCount`, `-commentCount` 정렬 |
| `댓글은_작성자만_지우고_글을_지우면_함께_사라진다` | 댓글 소유권, 없는 글은 `NotFound`(빈 목록과 구분), FK cascade |

```
$ cargo test --workspace
domain  tests/services.rs  → 8 passed (0.9s)   + 유틸 1 passed
web     tests/api.rs        → 20 passed (1.4s)   + 단위 6 passed   (경로·응답 모양·오류 코드·쿠키·429를 oneshot으로)
```

### 3-2. DB 교체 — URL 한 줄 (study11 결과)

```
DATABASE_URL=sqlite://board.db?mode=rwc                           # 기본
DATABASE_URL=postgres://board:***@db:5432/board                    # compose의 PostgreSQL
DATABASE_URL=mysql://root:pw@127.0.0.1:53306/board                 # MariaDB
```

엔티티·서비스·마이그레이션은 그대로. 이 과정에서 고친 두 가지(검색 `LOWER`, UNIQUE는 `sql_err()`)가 위 서비스 시험으로 고정되었다. 스펙 v1용 `m0002`(컬럼 이름 변경·`tags`·`comments`)도 세 DB에서 그대로 돌고, **옛 `board.db`(글 2개·회원 2명)** 에 적용해도 데이터가 그대로 남았다.

### 3-3. Docker — [project/Dockerfile](project/Dockerfile) · [project/compose.yaml](project/compose.yaml)

워크스페이스는 크레이트마다 "가짜 소스"를 만들어 의존성 레이어를 캐시한다.

```dockerfile
COPY Cargo.toml Cargo.lock ./
COPY crates/domain/Cargo.toml crates/domain/Cargo.toml
COPY crates/web/Cargo.toml crates/web/Cargo.toml
RUN mkdir -p crates/domain/src crates/web/src \
    && echo '' > crates/domain/src/lib.rs && echo 'fn main() {}' > crates/web/src/main.rs \
    && cargo build --release -p web && rm -rf crates/domain/src crates/web/src      # 의존성만 컴파일 → 캐시

COPY crates ./crates
RUN touch crates/domain/src/lib.rs crates/web/src/main.rs && cargo build --release -p web
```

```yaml
services:
  db:
    image: postgres:17
    healthcheck: { test: ["CMD-SHELL", "pg_isready -U board -d board"], interval: 5s, retries: 10 }
  web:
    build: .
    depends_on: { db: { condition: service_healthy } }        # DB가 준비된 뒤에 앱을 띄운다
    ports: ["${WEB_PORT:-4000}:4000"]
    environment:
      DATABASE_URL: postgres://board:${POSTGRES_PASSWORD:-board-dev-password}@db:5432/board     # 호스트 이름 db = 서비스 이름
      JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}                                  # 없으면 시작 거부
      SECURE_COOKIE: ${SECURE_COOKIE:-false}                                                     # HTTPS 프록시 뒤에서는 true
```

실측 (`docker compose up -d --build`, 웹 포트 4300 — 스펙 v1 API 기준):

```
GET /health                           → {"status":"ok"}          GET /api/health → 404 NOT_FOUND (예전의 /api/health는 스펙에 맞춰 /health로 옮겼다)
GET /                                  → 200 text/html   (SPA)        GET /some/route → 200 (딥링크)
POST /api/auth/signup                  → 201 {"data":{"id":1,"email":"frank@example.com","nickname":"frank","createdAt":"..."}}
signup (같은 이메일)                    → 409 EMAIL_TAKEN              ← PostgreSQL에서도
signup (같은 닉네임)                    → 409 CONFLICT
POST /api/auth/login                   → 200  set-cookie: session=eyJ0eXAi...; HttpOnly; SameSite=Lax; Path=/; Max-Age=43200
POST /api/posts (쿠키 없음)             → 401 {"error":{"code":"UNAUTHORIZED","message":"로그인이 필요합니다","requestId":"993dc193-..."}}
POST /api/posts (쿠키)                  → 201 {"data":{"id":1,"title":"Docker post","body":"in compose","tags":["docker"],"author":{"id":1,"nickname":"frank"},"commentCount":0,...}}
댓글 1개 작성 후 GET /api/posts?q=DOCKER → meta {'page':1,'limit':10,'total':1,'totalPages':1,'q':'DOCKER'}, commentCount 1   ← 대소문자 무시
docker inspect → healthy              id → uid=1000(appuser)         이미지 약 152MB
로그: database ready database_url=postgres://board:***@db:5432/board        ← 비밀번호 마스킹
psql: posts 1 / users 1 / comments 1 / seaql_migrations 2                      ← 컨테이너의 PostgreSQL에 실제로 저장 (m0001, m0002)
스펙 스모크 시나리오(가입·중복·로그인·글·태그·댓글·검색·정렬·limit 보정·400/404·SPA·logout·cascade) 전부 통과
docker stop → 0.4초, 로그 "종료 신호(SIGTERM)를 받았습니다"
```

### 3-4. CI/CD — [.github/workflows/](.github/workflows/)

- `ci.yml`: push/PR마다 `demo`(SQLite 테스트 + PostgreSQL feature 컴파일) · `project`(`cargo test --workspace`) · `lab`(`cargo test --lib --examples`) 세 job 병렬.
- `docker-publish.yml`: `v*` 태그 푸시 시 **`demo`와 `project` 두 이미지**를 matrix로 GHCR에 게시 (`GITHUB_TOKEN`, 최소 권한). 서버 배포 job은 시크릿 3개를 등록하면 켤 수 있도록 주석으로 준비.

---

## ④ 🔗 연동 포인트 — 구성도 · 구현 순서 · 배포 체크리스트 · 점검표

### 4-1. 연동 구성도 (완성형)

```
┌─────────────┐   HTTPS      ┌──────────────┐  http   ┌─────────────────────────────────────────────────────────────────┐
│ 브라우저(SPA)│ ───────────▶ │ 리버스 프록시  │ ──────▶ │ web  (axum)                                                      │
│ api.js      │ ◀─────────── │ (TLS 종료)    │ ◀────── │  미들웨어: RequestId · Trace · 압축 · error_envelope · Panic ·    │
│ EventSource │   쿠키/SSE    └──────────────┘         │            Timeout · BodyLimit                                   │
└─────────────┘                                        │  api/: users · posts · documents · ai(SSE) · webhooks · oauth    │
                                                       │  extract: AuthUser(쿠키/Bearer) · ValidatedJson                  │
                                                       └───────┬─────────────────────────┬──────────────────────┬─────────┘
                                                               │ 서비스 호출               │ trait 경계            │ SeaORM
                                                       ┌───────▼────────┐     ┌──────────▼────────────┐   ┌─────▼──────────────────┐
                                                       │ domain          │     │ integrations           │   │ DB (URL로 선택)         │
                                                       │ service::user   │     │  ai: reqwest → AI 서버  │   │ SQLite / PostgreSQL /  │
                                                       │ service::post   │     │  mail, oauth, webhook   │   │ MariaDB                │
                                                       │ service::document│    └───┬─────────┬───────────┘   │ users · posts ·        │
                                                       └─────────────────┘        │         │               │ documents · qa(jsonb)  │
                                                                           ┌──────▼───┐ ┌───▼─────────┐     └────────────────────────┘
                                                                           │ AI 서버   │ │ 결제·메일·   │
                                                                           │ Ollama 등 │ │ OAuth 제공자  │ ← 웹훅(서명 검증)이 web으로 들어온다
                                                                           └──────────┘ └─────────────┘
환경변수: DATABASE_URL · JWT_SECRET · SECURE_COOKIE · BIND_ADDR · STATIC_DIR · AI_BASE_URL · AI_MODEL · WEBHOOK_SECRET · OAUTH_* · MAIL_API_*
```

### 4-2. 구현 순서

| 순서 | 할 일 | 상태 | 끝을 확인하는 방법 |
|---|---|---|---|
| 1 | 구조 전환: 단일 크레이트 → `domain` / `web` 워크스페이스, 오류를 `DomainError` + 번역 계층으로 분리 | ✅ | `domain`의 `Cargo.toml`에 axum이 **없다** |
| 2 | ORM 전환(SeaORM): 엔티티·관계(`find_also_related`)·집계 서브쿼리·코드 마이그레이션 | ✅ | 서비스 시험 8개 |
| 3 | 인증: 쿠키 세션 + Bearer, `AuthUser`, 소유권은 서비스 함수의 `actor_id` 필수 인자 | ✅ | 남의 글 수정 403 시험 |
| 4 | SPA 내장 서빙 + `fallback` | ✅ | 딥링크 200 |
| 5 | **DB 교체**: `sqlx-postgres`/`sqlx-mysql`, 검색 `LOWER`, UNIQUE는 `sql_err()` | ✅ | 세 DB 실연결 + 회귀 시험 |
| 6 | **테스트·보안 점검** 기반: 서비스 시험, 비밀키 검증/경고, 로그 마스킹, SIGTERM | ✅ | 4-4 점검표 |
| 7 | **Docker + compose + CI/CD** | ✅ | compose 기동, `docker stop` 0.4s, Actions |
| 8 | **스펙 v1 정렬**: `web/error.rs`를 `{ error: { code, message, fields, requestId } }`로(코드는 study6 표), `error_envelope`·`SetRequestId` 이식, 응답 camelCase·`{ data, meta }`·`limit`≤50·`sort`, 이메일 로그인·`nickname`·`tags`·댓글(마이그레이션 `m0002`), 429, 프론트 `static/app.js`를 새 모양으로 | ✅ | `demo/`의 시나리오를 `web/tests/api.rs`(20개)로 이식, 세 DB·컨테이너 스모크. **화면은 브라우저로 직접 확인이 필요하다** (JS 문법 검사와 API 계약 시험까지만 했다) |
| 9 | **외부 서비스** 합류: `integrations`에 `MailSender`·`OAuthClient`·웹훅 검증을 **trait + reqwest 구현**으로, `web/api/webhooks.rs`·`oauth.rs`. 멱등 키는 DB UNIQUE | 🧩 | 웹훅 서명 시험 3개 이식 + 가짜 제공자로 OAuth 흐름 |
| 10 | **AI 기능** 합류: `domain::service::document` (문서·질문 저장, 발췌 선택 규칙), `integrations::ai`(타임아웃·오류 번역), `web/api/ai.rs`(SSE 중계, `AuthUser` 필수·속도 제한) | 🧩 | 가짜 AI 서버 시험 + 실제 Ollama 1회 |
| 11 | 실시간·성능 합류: 완료 알림 SSE, 요약 캐시(문서 해시 키), 오래 걸리는 요약은 작업 큐 + 202 | 🧩 | 알림이 도착, 같은 문서 재요약 시 `hit` |
| 12 | 운영 보강: JSON 로그, 메트릭, 백업 스크립트(`pg_dump`/`VACUUM INTO`)와 복원 연습 (속도 제한 429는 8번에서 끝) | ⬜ | 복원 리허설 |
| 13 | 배포: 태그 → 이미지 게시 → 서버 `compose pull && up -d` → 점검표 | ✅/⬜ | 4-3 체크리스트 |

> **8번이 가장 먼저**였다. 이후 모든 합류(9~11)가 새 오류·응답 모양을 쓰기 때문이다. 순서를 거꾸로 하면 같은 코드를 두 번 고친다. 8번을 하며 **도메인 타입에서 `Serialize`를 뺀** 것도 같은 이유다 — JSON 모양을 바꿔도 `domain`은 건드리지 않는다.

#### 9~11번 합류의 모양 (검증된 부품 → `project/`)

```rust
// integrations: 경계는 trait. 구현은 reqwest.  (lab/s18_ai.rs의 ai_post, build_prompt, drain_lines를 옮긴다)
pub trait AiClient: Clone + Send + Sync + 'static {
    fn summarize(&self, text: &str) -> impl Future<Output = Result<String, AiError>> + Send;
    fn ask(&self, text: &str, question: &str) -> impl Future<Output = Result<Answer, AiError>> + Send;
}
// AiError → web/error.rs: Rejected(status) → 그 4xx 그대로 / Unavailable, Timeout, BadResponse → 502 (+ code AI_*)

// web/api/ai.rs: 인증은 인자에 적는 것으로, 비용이 드는 호출이라 AuthUser 필수
pub async fn ask_question(State(state): State<AppState>, user: AuthUser, Path(id): Path<i32>, ValidatedJson(body): ValidatedJson<AskRequest>) -> ApiResult<..> {
    let doc = document_service::find_owned(&state.db, id, user.id).await?;          // 소유권 검사는 서비스 안에서
    let answer = state.ai.ask(&doc.text, &body.question).await?;                    // trait 경계 → 시험에서는 가짜
    let qa = document_service::save_question(&state.db, id, &body.question, &answer).await?;
    Ok((StatusCode::CREATED, Json(qa)))
}
```

---

### 4-3. 배포 체크리스트

```
코드·시험
□ cargo test --workspace (project) · cargo test (demo) · cargo test --lib --examples (lab) 전부 통과
□ CI(Actions) 초록
□ 새로 합친 외부 연동은 가짜로 시험되어 있다 (실제 결제·메일·AI 호출 없이 CI가 돈다)

설정·비밀
□ JWT_SECRET: 운영 전용 ≥32자 무작위, 이미지·저장소·로그에 없다 (없으면 compose가 시작 거부)
□ DATABASE_URL·POSTGRES_PASSWORD·WEBHOOK_SECRET·OAUTH_CLIENT_SECRET·MAIL_API_KEY·AI 키는 시크릿 저장소에서 주입
□ SECURE_COOKIE=true (HTTPS 뒤), CORS는 운영 출처만(분리 배포일 때), 개발용 기본값 경고(WARN)가 없다

이미지·실행
□ docker compose up -d --build → web·db 모두 healthy
□ 비root(uid 1000), 볼륨(pg-data)이 있다, restart: unless-stopped
□ docker stop 이 1초 안팎 (SIGTERM), 로그에 "종료 신호(SIGTERM)"
□ 이미지 태그가 버전(v0.x.y)이다 — 롤백할 직전 태그를 기록했다

기능 시나리오 (curl 또는 브라우저)
□ 가입 → 로그인(Set-Cookie) → 글 작성 → 목록/검색(대소문자 무시) → 수정 → 남의 글 수정 403 → 삭제
□ 오류 모양: 401/403/404/409/429/500이 모두 같은 형식, 응답 헤더 X-Request-Id = 본문 requestId
□ (합류 후) 웹훅: 잘못된 서명 401, 같은 이벤트 두 번 → 한 번만 처리
□ (합류 후) AI: 근거 있는 질문 / 근거 없는 질문("찾을 수 없습니다") / AI 서버 중지 시 502

데이터
□ 컨테이너를 지웠다 다시 띄워도 데이터가 남는다
□ 백업(pg_dump)을 새 DB에 복원해 앱을 붙여 봤다 (RTO를 쟀다)

운영
□ 로그에 비밀번호·토큰·DB 비밀번호가 없다 (database_url=postgres://board:***@db…)
□ 외부에서 /health를 1분마다 확인하는 감시가 있다, 알림 수신자가 정해져 있다
□ 인증서 만료·디스크 용량 알림
```

---

### 4-4. 테스트 · 보안 점검표

| 영역 | 점검 | 상태 | 근거 |
|---|---|---|---|
| 비밀번호 | Argon2 해시 + `spawn_blocking`, 평문 저장/로그 없음 | ✅ | `domain/password.rs` |
| 사용자 열거 | 없는 아이디/틀린 비밀번호 같은 응답 | ✅ | 서비스 시험 |
| 세션 쿠키 | `HttpOnly; SameSite=Lax` (+ `Secure` 설정) | ✅ | `web/auth.rs` (study7) |
| 인가 | 소유권을 **서비스**에서 검사 (`actor_id` 필수) | ✅ | 서비스 시험 |
| 입력 | `validator`, `deny_unknown_fields`, `limit` 상한 50(보정), `sort` 허용 목록 | ✅ | `web/dto.rs` |
| SQL 인젝션 | ORM 바인딩 / sqlx `$n`, `format!` 금지(`SqlSafeStr`) | ✅ | 구조상 + `demo` 인젝션 시험 |
| 시크릿 | 짧은 키 부팅 거부, 기본 키 경고, compose는 미설정 시 거부 | ✅ | `main.rs`, `compose.yaml` |
| 로그 | DB 비밀번호 마스킹 | ✅ | `domain::redact_database_url` + 단위 시험 |
| 종료 | SIGTERM 정상 종료 | ✅ | 컨테이너 실측 0.4초 |
| XSS | SPA가 `textContent`/이스케이프로 렌더링 | ✅/확인 | `static/app.js`의 `esc()` 사용 — 새 화면에서도 규칙 유지 |
| CSRF | `SameSite=Lax` 쿠키. 상태 변경은 `POST/PATCH/DELETE`만 | ✅ | study7 |
| CORS | 같은 출처 서빙이라 불필요. 분리 배포 시 정확한 출처 + `credentials` | ✅ | study12 |
| 속도 제한 | IP당 분당 N회 (`RATE_LIMIT_PER_MINUTE`), 429 + `Retry-After` | ✅ | `web/ratelimit.rs` · 시험. **로그인·AI 호출·웹훅에 더 낮은 한도**는 ⬜ (AI는 비용이 직결). 서버가 여러 대면 공유 저장소 필요 |
| 의존성 | 취약점 점검 (`cargo audit`), 이미지 베이스 갱신 | ⬜ | CI에 `cargo audit` 추가 후보 |
| 업로드 | 형식 허용 목록·UUID 이름·크기 제한·정리 | 🧩 | `demo`/`lab` (합류 시) |
| 웹훅 | 서명(HMAC, 상수 시간) + 멱등 | 🧩 | `lab/s13` |
| AI | 프롬프트 구분자 정화·길이 예산·권한 최소화·타임아웃·비밀(프롬프트) 로그 금지 | 🧩 | `lab/s18` |
| 시험 | 서비스 시험 8, HTTP 계약 시험 20(project) · 31(demo), 외부는 가짜 | ✅/🧩 | CI |

---

## ⑤ 오류 노트 (합치면서 생기는 것)

| 증상 | 원인 | 해결 |
|---|---|---|
| 워크스페이스 Docker 빌드가 매번 의존성 전체를 다시 컴파일 | 크레이트별 `Cargo.toml`을 먼저 복사하지 않았거나 `Cargo.lock` 누락 | 루트 + 각 크레이트의 `Cargo.toml`/`Cargo.lock`만 먼저 → 가짜 소스로 빌드 → 진짜 소스 |
| compose에서 web이 `connection refused`로 재시작 반복 | DB가 준비되기 전에 앱이 시작 | `depends_on: condition: service_healthy` + DB `healthcheck` |
| web 컨테이너에서 `localhost:5432` 연결 실패 | 컨테이너의 `localhost`는 자기 자신 | 서비스 이름 `db` |
| PostgreSQL에서만 중복 가입이 500 / 검색이 비어 있음 | UNIQUE를 문자열로 판별 / `LIKE` 대소문자 | `sql_err()` / `LOWER` (study11) — 서비스 시험이 회귀를 잡는다 |
| 서비스 시험이 `no such table` 또는 서로 간섭 | in-memory DB가 연결마다 따로 / 시험끼리 같은 DB | `cache=shared` + **시험마다 다른 이름** |
| 스펙 정렬 후 SPA 화면이 깨짐 | 프론트가 옛 응답 모양(`items`, `per_page`, `author_name`, `details`, `username`)을 직접 읽음 | 실제로 `static/app.js`의 `api()`(오류는 `error.fields`), 목록(`data`/`meta`), 상세(`author.nickname`), 로그인 화면(이메일·닉네임)을 고쳐야 했다. API 호출을 **한 곳**(`api()`)으로 모아 두었기 때문에 오류 처리는 한 곳만 고치면 됐다 |
| AI 합류 후 CI가 외부 호출로 실패/지연 | 시험이 실제 AI 서버를 부름 | `AiClient` trait + 가짜 구현 |
| 도메인 크레이트에 `reqwest`/`axum`이 들어옴 | 편해서 경계를 넘음 | **의존성 목록이 곧 아키텍처 규칙.** 새 의존성을 넣을 때 어느 크레이트인지 먼저 묻는다 |
| 쿠키가 운영(HTTPS)에서 안 붙음 / 개발(http)에서 안 붙음 | `Secure` 속성 불일치 | `SECURE_COOKIE` 환경변수로 환경별 |
| 이미지 게시 워크플로가 `project`만 실패 | Dockerfile이 컨텍스트에 `static/`이 없다고 판단 / 컨텍스트 경로 오류 | `context: project`, `.dockerignore`가 `static/`을 제외하지 않는지 확인 |

---

## ⑥ 직접 해보기

1. **브라우저로 화면 확인 + 한 곳 고치기**: `cargo run -p web` 후 `http://127.0.0.1:4000`에서 가입(이메일+닉네임) → 로그인 → 글쓰기(태그) → 댓글 → 검색을 눌러 보고, 개발자도구 Network 탭에서 응답이 `{ data }`/`{ data, meta }`인지 본다. 일부러 제목을 비워 보내 `error.fields`가 폼 아래에 표시되는지, `requestId`를 `toast`에 덧붙이려면 `api()` 한 곳만 고치면 되는지 확인한다. (이 화면은 JS 문법 검사와 API 시험까지만 자동으로 확인했다.)
2. **AI 기능 합류 (순서 10)**: `AiClient` trait를 정의하고, `lab/s18_ai.rs`의 `ai_post`/`build_prompt`를 `integrations`로 옮긴다. 가짜 구현으로 "AI 서버가 422를 주면 422, 죽으면 502" 서비스 시험을 쓴다. 마지막에 `AI_BASE_URL`을 실제 Ollama 쪽 어댑터로 바꿔 한 번 실행한다.
3. **복원 리허설 + 롤백 리허설**: `compose`의 PostgreSQL을 `pg_dump` → 새 볼륨에 `pg_restore` → 앱을 붙여 가입·조회. 그리고 이미지 태그를 한 단계 전으로 돌려 `compose up -d`로 롤백하는 데 걸리는 시간을 잰다.

---

## ⑦ ☐ 체크리스트

- ☐ 기능 목록 F1~F15를 ✅/🧩/⬜로 구분해 말할 수 있고, 합쳐진 것과 부품만 있는 것을 헷갈리지 않는다
- ☐ `domain` ← `integrations` ← `web`의 의존 방향과, 외부 서비스는 trait 뒤에 둔다는 규칙을 설명할 수 있다
- ☐ `cargo test --workspace`가 통과하고, 서비스 시험 8개와 HTTP 시험 20개가 각각 무엇을 고정하는지 안다
- ☐ `domain`의 타입에 `Serialize`가 없고 JSON 모양은 `web/dto.rs`가 정한다는 것, 그래서 스펙이 바뀌어도 `domain`이 안 바뀐다는 것을 설명할 수 있다
- ☐ `DATABASE_URL`만 바꿔 SQLite/PostgreSQL/MariaDB에서 같은 시나리오가 돌아가는 것을 확인했다
- ☐ `docker compose up -d --build`로 웹 + PostgreSQL을 띄우고 시나리오와 `docker stop` 시간을 확인했다
- ☐ 태그 → 이미지 게시(`docker-publish.yml`)의 흐름과 필요한 권한을 설명할 수 있다
- ☐ 배포 체크리스트와 테스트·보안 점검표에서 ⬜ 항목이 무엇이고 왜 남았는지 말할 수 있다
- ☐ 구현 순서에서 "스펙 정렬(8)"이 외부 서비스·AI 합류보다 먼저인 이유를 설명할 수 있고, 옛 DB에 새 마이그레이션을 쌓는 방식(`m0002`)을 설명할 수 있다
- ☐ 최종 회고(아래)를 자기 말로 다시 쓸 수 있다

---

## 최종 회고 (순서대로)

1. **목표 대비 결과** — 학습 목표 여섯 가지(라우터·extractor·상태 / JSON·검증 / DB / 오류 타입 / JWT / Docker)는 모두 도달했다. 그 위에 DB 교체(3종), 프론트 계약, 외부 연동, API 설계, 성능, 운영, 실시간, AI를 **실행 가능한 코드와 측정값**으로 정리했다. 최종 서비스(`project/`)는 뼈대(F1~F8, F15)가 합쳐져 컨테이너로 돌고 공통 API 스펙 v1을 따르며, 검증된 부품(F9~F14)은 합류 순서가 정해져 있다.
2. **숫자** — 테스트: demo 43(단위 12 + 통합 31) · project 35(domain 9 + web 26) · lab 9 + 프론트 9. 캐시 311ms→0ms. 블로킹 격리 시 `/health` 1.26s→1ms. `docker stop` 10.4s→0.4s. 이미지 160MB(demo) / 151MB(project). 응답 압축 4,246B→94B. `LIKE` 한 줄과 UNIQUE 판별 한 줄이 PostgreSQL에서 검색과 가입을 깼다.
3. **잘 된 것** — ① 오류를 값으로 다루니 **새 갈래의 매핑 누락을 컴파일러가 알려 줬다.** ② 크레이트 경계가 **의존성 목록**으로 강제되어 계층이 무너지지 않았다. ③ trait 경계 + 가짜 구현으로 외부 서비스를 **서버 없이** 시험했다. ④ 모든 단계가 `curl`로 재현 가능한 결과를 남겼다.
4. **막힌 것과 원인** — (a) **버전 차이**(axum 0.8 `{id}`, jsonwebtoken 11 `rust_crypto`, sqlx 0.9 `SqlSafeStr`, argon2 0.6, reqwest 0.13 `form`, SeaORM `fetch_page` 0-based). (b) **"로컬에서는 됐다"**: Docker 빌드의 `curl` 의존, SIGTERM 미처리, 로그의 DB 비밀번호, `/api/없는경로`가 SPA로 새는 것, 호스트 PostgreSQL과의 포트 충돌 — **직접 돌려 보기 전에는 보이지 않았다.** (c) **가짜와 진짜의 차이**: 가짜 AI 서버로는 안 보이던 thinking 토큰, SQLite로만 시험하면 안 보이는 PostgreSQL의 `LIKE`·UNIQUE 메시지.
5. **배운 원칙** — study10 총정리의 일곱 가지 + ⑧ **경계는 한 곳에**(오류 번역, 설정, 프론트 API 호출, DB 종류) ⑨ **조용히 실패하는 것이 가장 위험하다**(백업의 `.db`만 복사, 무시된 `RUST_LOG`, 깨진 JSON을 빈 목록으로, 빈 토큰) ⑩ **가장 비싼 호출 앞에 가장 싼 검증을.**
6. **남은 일 (우선순위)** — ① (완료) 스펙 v1 정렬 — 단, **브라우저에서 SPA 화면을 직접 확인**하는 일이 남았다 ② 외부 서비스·AI 합류(9, 10) — DocuForge의 오케스트레이터 자리에서 실제 FastAPI 스키마 대조 ③ 로그인·AI 호출의 낮은 한도·`cargo audit`·JSON 로그·메트릭 ④ 서버 자동 배포 job 활성화와 롤백 리허설 ⑤ 노션의 AI 미니 프로젝트에서 인증이 가장 약한 곳(로그인 없음)에 이 서비스의 JWT/쿠키를 붙이기.

---

## ⑧ 다음 단계 연결

이 저장소의 커리큘럼은 여기서 끝난다. 이어서 할 일은 위 **남은 일** 목록이다. 스펙 v1 차이 표([study6 ④](study6.md))는 지웠으니 다음 계약은 외부 서비스·AI 합류(순서 9, 10)다. 문서·코드·측정값의 전체 지도는 [README.md](README.md)에 있다.
