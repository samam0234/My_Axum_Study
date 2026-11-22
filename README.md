# Axum 학습 저장소

Rust 웹 프레임워크 [Axum](https://github.com/tokio-rs/axum) **하나만** 기초부터 실전 서비스까지 단계별로 익히며 기록하는 개인 학습 저장소입니다. 문서(`study*.md`)는 개념·코드·**실제로 실행해서 확인한 결과**·겪은 오류를 한 단계씩 정리하고, 코드는 세 폴더(`demo/`, `project/`, `lab/`)에 있습니다.

- 학습 목표와 로드맵의 기준: [Notion — Axum](https://app.notion.com/p/Axum-3d6d7dec5e2e814581a4e96fc36e6c71)
- 프론트·백엔드 계약의 기준: 노션 "HTTP · REST · API 설계"의 **공통 API 스펙 v1** (경로 `/api/…`, 응답 `{ data, meta }`, 오류 `{ error: { code, message, fields, requestId } }`, JSON camelCase)
- 모든 문서는 같은 8단 구성입니다: ① 목표 ② 📖 이론 ③ 🛠️ 실기(전체 코드·줄별 설명) ④ 🔗 연동 포인트 ⑤ 오류 노트 ⑥ 직접 해보기 ⑦ ☐ 체크리스트 ⑧ 다음 단계 연결

## 학습 문서 목차

### 기초 구간 (study0 ~ study10)

| 문서 | 주제 | 실행해 볼 코드 |
|---|---|---|
| [study0.md](study0.md) | **환경 설정** — rustup·cargo·버전 확인·프로젝트 생성·`cargo watch`·에디터 확장·`.gitignore`/`.env` | `hello-axum` (문서 안) |
| [study1.md](study1.md) | **Hello World와 기본 문법** — 문자열·JSON·HTML 응답, `{name}` 경로, 쿼리, 이스케이프 | [lab/examples/s01_hello.rs](lab/examples/s01_hello.rs) |
| [study2.md](study2.md) | **핵심 구조** — 핸들러 → Router → 모듈, `State`, `with_state`, 모듈 간 호출 | [s02_modules.rs](lab/examples/s02_modules.rs) |
| [study3.md](study3.md) | **데이터 다루기** — `Arc<RwLock<…>>`, 파일 저장(임시 파일 + rename), 화면 바인딩, `localStorage` | [s03_store.rs](lab/examples/s03_store.rs) |
| [study4.md](study4.md) | **라우팅과 요청 처리** — 동적/와일드카드 경로, 쿼리, 404/405, 미들웨어 흐름, CORS | [s04_routing.rs](lab/examples/s04_routing.rs) |
| [study5.md](study5.md) | **폼·입력·검증** — `validator`, `ValidatedJson`, 상태 코드별 화면, 중복 제출 방지 | [s05_form.rs](lab/examples/s05_form.rs) |
| [study6.md](study6.md) | **DB·API 연동** — sqlx CRUD, 마이그레이션, 스펙 v1 오류 본문·camelCase, 환경변수 | [demo/](demo/) |
| [study7.md](study7.md) | **인증·보안 기초** — Argon2, JWT, `AuthUser`, 401/403, CORS·XSS·CSRF, Bearer vs 쿠키 | [demo/src/auth/](demo/src/auth/) |
| [study8.md](study8.md) | **구조화·재사용** — lib+bin, 공통 모듈, `Config`, 미들웨어 순서, 개발/운영 설정 | [demo/src/lib.rs](demo/src/lib.rs) |
| [study9.md](study9.md) | **테스트·디버깅** — 단위·`oneshot` 테스트, 로그, 오류 TOP 10, mock | [demo/tests/](demo/tests/), [lab/src/mock_demo.rs](lab/src/mock_demo.rs) |
| [study10.md](study10.md) | **배포·정리** — 릴리스 빌드, 멀티 스테이지 Docker, HTTPS 개념, 배포 후 점검표, 기초 총정리 | [demo/Dockerfile](demo/Dockerfile) |

### 중간 응용

| 문서 | 주제 |
|---|---|
| [studyMidpoint.md](studyMidpoint.md) | study0~10만으로 **게시판 · 로그인 · CRUD 서비스**를 조립: 기능 목록, 폴더 구조, 구현 순서 16단계, 연동 구성도, 1차 배포 체크리스트, 회고 |

### 심화 구간 (study11 ~ study18)

| 문서 | 주제 | 실행해 볼 코드 |
|---|---|---|
| [study11.md](study11.md) | **DB 확장** — PostgreSQL(sqlx feature)·MariaDB(SeaORM)·MongoDB, ORM 설정 차이 | [demo/src/db.rs](demo/src/db.rs), [lab/examples/s11_mongo.rs](lab/examples/s11_mongo.rs) |
| [study12.md](study12.md) | **프론트·UI 연동** — 정적 서빙·SPA fallback, 프록시/CORS, 쿠키, 프론트 API 클라이언트 | [s12_front.rs](lab/examples/s12_front.rs), [lab/static/api.js](lab/static/api.js) |
| [study13.md](study13.md) | **외부 서비스** — 결제 웹훅(HMAC), 메일, 파일 업로드, 소셜 로그인(OAuth2) | [s13_external.rs](lab/examples/s13_external.rs) |
| [study14.md](study14.md) | **API 설계** — REST 규칙, OpenAPI, 버전, 오프셋/커서, 오류 규칙, ETag | [s14_api_design.rs](lab/examples/s14_api_design.rs) |
| [study15.md](study15.md) | **성능·캐시** — TTL 캐시, 작업 큐, `spawn_blocking`, 측정 | [s15_cache_jobs.rs](lab/examples/s15_cache_jobs.rs) |
| [study16.md](study16.md) | **운영·CI/CD** — Docker 체크리스트, GitHub Actions, 로그·모니터링, 백업·복원 | [.github/workflows/](.github/workflows/) |
| [study17.md](study17.md) | **실시간·확장** — WebSocket, SSE, 스트리밍 업로드 | [s17_realtime.rs](lab/examples/s17_realtime.rs) |
| [study18.md](study18.md) | **AI 연동·MCP** — AI 서버 호출, 프롬프트 설계, 스트리밍, MCP; DocuForge 연결 | [s18_ai.rs](lab/examples/s18_ai.rs) |

### 심화 응용

| 문서 | 주제 |
|---|---|
| [studyFinal.md](studyFinal.md) | study11~18 + midpoint를 합쳐 **서비스를 완성**: DB 교체, 외부 서비스, AI, Docker·자동 배포, 테스트·보안 점검, 연동 구성도, 구현 순서, 배포 체크리스트, 최종 회고 |

## 저장소 구조

```
Auxm/
├─ study0.md ~ study10.md        ← 기초 구간 문서
├─ studyMidpoint.md              ← 중간 응용
├─ study11.md ~ study18.md       ← 심화 구간 문서
├─ studyFinal.md                 ← 심화 응용
├─ demo/                         ← 게시판 API (sqlx · 단일 크레이트 · lib+bin) — study6~10, midpoint의 결과물
│  ├─ migrations/  migrations_pg/   SQLite / PostgreSQL 마이그레이션
│  ├─ src/                         lib.rs(앱 조립) + main.rs + 기능 모듈(posts, auth)
│  ├─ tests/api.rs                 oneshot 통합 테스트 15개
│  └─ Dockerfile  compose.yaml  compose.postgres.yaml
├─ project/                      ← 최종 서비스 (Cargo 워크스페이스 domain/web · SeaORM · 쿠키 세션 · SPA) — studyFinal의 결과물
│  ├─ crates/domain/               HTTP를 모르는 도메인 (+ 서비스 테스트 5개)
│  ├─ crates/web/                  axum 표현 계층
│  ├─ static/                      vanilla JS SPA
│  └─ Dockerfile  compose.yaml     (웹 + PostgreSQL)
├─ lab/                          ← 단계별 미니 예제 (한 파일 = 한 주제)
│  ├─ examples/s01 ~ s18_*.rs      `cargo run --example <이름>`
│  ├─ src/mock_demo.rs             study9 mock 예제 (+ 테스트)
│  └─ static/                      study12 프론트 (api.js + node 테스트)
├─ .github/workflows/            ← CI(테스트)와 이미지 게시(태그)
└─ .agent/ .claude/ .codex/ .grok/   ← 에이전트 공통·개별 가이드
```

## 사전 준비

Rust 툴체인이 필요합니다. Windows에서는 **MSVC 빌드 도구 + Windows SDK**(Visual Studio의 "C++를 사용한 데스크톱 개발")도 함께 필요합니다. 자세한 설치와 버전 확인은 [study0.md](study0.md)입니다.

```powershell
winget install Rustlang.Rustup
rustc --version      # rustc 1.98.1
cargo --version      # cargo 1.98.1
```

선택: Docker(study10·11·16), Node 20+(`lab/static`의 프론트 테스트), 로컬 Ollama(study18의 실제 모델 시험).

## 기술 스택

| 폴더 | 스택 |
|---|---|
| `demo/` | Rust 2024, axum 0.8.9, tokio, **sqlx 0.9** (SQLite 기본 / PostgreSQL feature), `validator`, `thiserror`/`anyhow`, `jsonwebtoken`(JWT), `argon2`, tower/tower-http, `tracing`, `utoipa` + Swagger UI(`vendored`) |
| `project/` | Cargo 워크스페이스, **SeaORM 1.1** (SQLite·PostgreSQL·MariaDB 드라이버), HttpOnly 쿠키 세션, 정적 SPA |
| `lab/` | axum(`ws`, `multipart`), `reqwest 0.13`, `hmac`/`sha2`, `tokio-stream`, `mongodb`, `validator` |

## 실행

### demo — 게시판 API

```powershell
cd demo
copy .env.example .env      # 선택: 값을 바꾸고 싶을 때
cargo run                   # http://127.0.0.1:3000
```

- API 문서(Swagger UI): `http://127.0.0.1:3000/docs`, OpenAPI JSON: `/api-docs/openapi.json`
- PostgreSQL로 실행: [study11.md](study11.md) — `docker compose -f compose.postgres.yaml up -d` 후 `cargo run --no-default-features --features postgres`
- Docker로 실행: [study10.md](study10.md) — `docker build -t axum-demo . ; docker run -e JWT_SECRET=... -p 3000:3000 axum-demo`

### project — 최종 서비스

```powershell
cd project
cargo run -p web            # http://127.0.0.1:4000  (API와 웹사이트가 함께 뜬다)
# 웹 + PostgreSQL 컨테이너: $env:JWT_SECRET="..."; docker compose up -d --build
```

### lab — 단계별 예제

```powershell
cd lab
cargo run --example s01_hello          # study1, 포트 3001
cargo run --example s05_form           # study5, 포트 3005
cargo run --example s18_ai             # study18, 포트 3018 (가짜 AI 서버 내장)
```

| 예제 | 포트 | 문서 | | 예제 | 포트 | 문서 |
|---|---|---|---|---|---|---|
| `s01_hello` | 3001 | study1 | | `s12_front` | 3012 | study12 |
| `s02_modules` | 3002 | study2 | | `s13_external` | 3013 | study13 |
| `s03_store` | 3003 | study3 | | `s14_api_design` | 3014 | study14 |
| `s04_routing` | 3004 | study4 | | `s15_cache_jobs` | 3015 | study15 |
| `s05_form` | 3005 | study5 | | `s17_realtime` | 3017 | study17 |
| `s11_mongo` | 3011 | study11 | | `s18_ai` | 3018 | study18 |

## 테스트

```powershell
cd demo    ; cargo test                       # 단위 7 + 통합 15
cd project ; cargo test --workspace           # 도메인 서비스 5 + 유틸 1
cd lab     ; cargo test --lib --examples      # mock 3 + 웹훅 서명 3 + 프롬프트/스트림 3
cd lab     ; node --test static/api.test.mjs  # 프론트 API 클라이언트 9
```

같은 명령을 [.github/workflows/ci.yml](.github/workflows/ci.yml)이 push/PR마다 실행합니다.

## 커밋 기록과 문서 번호

이 저장소는 한 번 **커리큘럼을 재편**했습니다. 옛 커밋 메시지의 `study1~10`은 처음 만들 때의 번호(라우터 → 인메모리 → 검증 → 오류 → sqlx → JWT → 미들웨어 → 페이지네이션 → 테스트 → 배포)이고, 지금의 문서 번호는 위 목차입니다.

| 옛 커밋(`feat: studyN …`) | 내용 | 지금 문서 |
|---|---|---|
| study1 | Cargo 프로젝트, 라우터/extractor | study0, 1, 4 |
| study2 | `Arc<RwLock>` 인메모리 CRUD | study2, 3 |
| study3 | `validator`, `ValidatedJson` | study5 |
| study4 | `AppError` + `IntoResponse` | study5, 6 |
| study5 | sqlx + SQLite | study6 |
| study6 | Argon2 + JWT + `AuthUser` | study7 |
| study7 | tower 미들웨어 계층 | study8 |
| study8 | 페이지네이션, 업로드 | study6, 13, 17 |
| study9 | lib+bin, `oneshot` 테스트, 설정 | study8, 9 |
| study10 | OpenAPI, Docker | study10, 14 |
| studyFinal | 워크스페이스 + SeaORM + SPA | studyFinal |

## 문서·커밋 규칙

공통 규칙은 [.agent/AGENTS.md](.agent/AGENTS.md), 커밋 규칙은 [.agent/COMMIT_GUIDE.md](.agent/COMMIT_GUIDE.md)를 따릅니다. 문서와 커밋 설명은 한글, 커밋 타입·코드·식별자·파일명은 영어입니다.
