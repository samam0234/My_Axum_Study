# lab — 단계별 미니 예제

학습 문서(`../study*.md`)가 **전체 코드와 줄별 설명**을 싣는 예제들입니다. 한 파일이 한 주제이고, 실행해서 `curl`/브라우저로 확인한 결과가 문서에 들어 있습니다.

```powershell
cargo run --example s01_hello       # 포트는 3001 → 3001, 3002 …
```

| 예제 | 포트 | 문서 | 확인하는 것 |
|---|---|---|---|
| [s01_hello.rs](examples/s01_hello.rs) | 3001 | study1 | 문자열/JSON/HTML 응답, `{name}` 경로, 쿼리, HTML 이스케이프 |
| [s02_modules.rs](examples/s02_modules.rs) | 3002 | study2 | `mod`로 나눈 라우터, `nest`, `State`, 모듈 간 호출 |
| [s03_store.rs](examples/s03_store.rs) | 3003 | study3 | `Arc<RwLock<Vec>>`, JSON 파일 저장(임시 파일 + rename), 화면 바인딩 |
| [s04_routing.rs](examples/s04_routing.rs) | 3004 | study4 | 와일드카드 경로, 리다이렉트(308), 404/405 JSON, 로깅 미들웨어, CORS |
| [s05_form.rs](examples/s05_form.rs) | 3005 | study5 | `validator`, `ValidatedJson`, 상태 코드별 화면, `Idempotency-Key` |
| [s06_db.rs](examples/s06_db.rs) | 3006 | study6 | sqlx + SQLite, 마이그레이션(`s06_migrations/`), 3-타입, `RETURNING`·`COALESCE`·`QueryBuilder`, 오류를 값으로 |
| [s07_auth.rs](examples/s07_auth.rs) | 3007 | study7 | Argon2 + `spawn_blocking`, JWT, `AuthUser` extractor, 401 vs 403, 사용자 열거 방지 |
| [s08_structure.rs](examples/s08_structure.rs) | 3008 | study8 | 인라인 모듈 경계, `Config` 시작 시 검증, **미들웨어 순서 버그 재현**(`LAYER_ORDER=wrong`) |
| [s09_testing.rs](examples/s09_testing.rs) | 3009 | study9 | 단위 시험, `oneshot` 요청 시험, 시험마다 새 상태, `test_app_with` |
| [s10_deploy.rs](examples/s10_deploy.rs) | 3010 | study10 | `BIND_ADDR`, 짧은 비밀키 거부, DB 비밀번호 마스킹, **SIGTERM 정상 종료**(진행 중 요청 완료) |
| [s11_mongo.rs](examples/s11_mongo.rs) | 3011 | study11 | MongoDB CRUD (`MONGODB_URI`, Docker `mongo:7`) |
| [s12_front.rs](examples/s12_front.rs) | 3012 | study12 | SPA fallback, CORS + 쿠키, `api.js` 클라이언트 |
| [s13_external.rs](examples/s13_external.rs) | 3013 | study13 | 웹훅 HMAC 검증·멱등, 메일 백그라운드, OAuth2(가짜 제공자 내장) |
| [s14_api_design.rs](examples/s14_api_design.rs) | 3014 | study14 | v1/v2, 오프셋/커서, `Deprecation`/`Sunset`, ETag/304 |
| [s15_cache_jobs.rs](examples/s15_cache_jobs.rs) | 3015 | study15 | TTL 캐시, 작업 큐, `spawn_blocking` 대비 |
| [s16_ops.rs](examples/s16_ops.rs) | 3016 | study16 | JSON 로그, `/health` vs `/ready`, `/metrics`, **WAL 백업 함정과 `VACUUM INTO`** (`-- backup out.db`) |
| [s17_realtime.rs](examples/s17_realtime.rs) | 3017 | study17 | WebSocket 채팅, SSE, 스트리밍 업로드 |
| [s18_ai.rs](examples/s18_ai.rs) | 3018 | study18 | AI 서버 호출·오류 번역, 프롬프트, SSE 중계, MCP (가짜 AI 서버 내장 / 실제 Ollama) |
| [src/mock_demo.rs](src/mock_demo.rs) | — | study9 | trait 경계 + 가짜 구현 (`cargo test`) |

```powershell
cargo test --lib --examples        # 예제별 시험 + mock (s06 3 · s07 4 · s08 3 · s09 6 · s10 3 · s13 3 · s16 3 · s18 3, lib 3)
node --test static/api.test.mjs    # 프론트 API 클라이언트 9 (서버 불필요, Node 20+)
```

- 환경변수: `s03`(`TODO_FILE`), `s06`(`DATABASE_URL`), `s07`(`JWT_SECRET`), `s08`(`LAYER_ORDER`, `JWT_SECRET`, `REQUEST_TIMEOUT_SECS`), `s10`(`BIND_ADDR`, `JWT_SECRET`, `DATABASE_URL`, `SHUTDOWN_AFTER_MS`), `s11`(`MONGODB_URI`), `s12`(`CORS_ORIGIN`), `s16`(`DATABASE_URL`, `LOG_FORMAT`), `s13`(`WEBHOOK_SECRET`, `MAIL_API_URL`, `OAUTH_*`), `s17`(`UPLOAD_DIR`), `s18`(`AI_BASE_URL`, `AI_GENERATE_URL`, `AI_MODEL`)
- 실행 중 만들어지는 파일(`todos.json`, `uploads/*`)은 `.gitignore`에 있습니다.
- 포트 `3000`은 `demo/`, `4000`은 `project/`가 씁니다.
