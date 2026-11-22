# 에이전트 공통 가이드라인 (인수인계 문서)

이 문서는 이 저장소에서 작업하는 모든 AI 에이전트(Claude, Grok, Codex 등)가 **공통으로** 지켜야 할 규칙을 정리한 인수인계 문서입니다.
`.claude/`, `.grok/`, `.codex/` 폴더의 에이전트별 문서는 이 문서를 기본으로 삼고, 도구별 특이사항만 추가로 기술합니다.

## 1. 저장소 개요

- Rust 기반 웹 프레임워크 [Axum](https://github.com/tokio-rs/axum) 학습 기록 저장소입니다. Axum은 tokio 팀이 만든 프레임워크로, `tower`의 `Service` 추상화 위에 라우터와 extractor를 얹은 구조입니다.
- **Axum 하나만** 다룹니다. 문서에서 다른 프레임워크와 비교하거나 통합하지 않습니다. (노션 학습 페이지의 "비교해 볼 대안" 항목은 이 저장소 문서의 범위가 아닙니다)
- 핵심 특징: **DI 컨테이너가 없고, 소유권/생명주기를 타입으로 직접 다룹니다.** `State<AppState>`를 명시적으로 주입하고, 런타임 예외 대신 `Result<T, AppError>`로 오류를 값으로 다룹니다.
- 공통 스펙: **게시판(Posts) CRUD + 회원/인증**을 처음부터 끝까지 사용합니다. 프론트·백엔드 계약은 노션 "HTTP · REST · API 설계"의 **공통 API 스펙 v1**입니다.
- 학습 목표와 전체 로드맵은 [Notion: Axum 학습 목표](https://app.notion.com/p/Axum-3d6d7dec5e2e814581a4e96fc36e6c71) 페이지를 기준으로 삼습니다.

### 문서 구성 (루트의 모든 학습 문서는 파일명이 `study`로 시작한다)

| 파일 | 구간 | 내용 |
|---|---|---|
| `study0.md` ~ `study10.md` | 기초 | 환경 설정 → Hello World → 핵심 구조 → 데이터 → 라우팅 → 폼·검증 → DB·API → 인증·보안 → 구조화 → 테스트·디버깅 → 배포·정리 |
| `studyMidpoint.md` | 중간 응용 | study0~10만으로 게시판·로그인·CRUD 서비스 조립 |
| `study11.md` ~ `study18.md` | 심화 | DB 확장 → 프론트·UI → 외부 서비스 → API 설계 → 성능·캐시 → 운영·CI/CD → 실시간 → AI·MCP |
| `studyFinal.md` | 심화 응용 | study11~18 + midpoint를 합쳐 서비스 완성 (구현 상태 표 포함) |

- **모든 문서가 같은 8단 구성**입니다: ① 목표 ② 📖 이론 ③ 🛠️ 실기(전체 코드·줄별 설명) ④ 🔗 연동 포인트 ⑤ 오류 노트 ⑥ 직접 해보기 ⑦ ☐ 체크리스트 ⑧ 다음 단계 연결. (midpoint/final은 요청된 목차 순서를 이 틀 안에 담았습니다)
- 문서의 실행 결과·수치는 **실제로 돌려서 얻은 것**만 적습니다. 돌려 보지 못한 것은 "미검증/스케치"라고 밝힙니다.
- 노션 [FW] 페이지의 "공통 API 스펙 v1 맞춤" 체크 항목은 해당 단계의 ☐ 체크리스트에 **원문 그대로** 들어 있습니다 (study6 ⑦ 세 항목, study11 ⑦ PostgreSQL 전환).

### 코드 폴더

- `demo/`: 게시판 API (sqlx · 단일 크레이트 · lib+bin). study6~10과 midpoint의 결과물. SQLite 기본, `--no-default-features --features postgres`로 PostgreSQL.
- `project/`: 최종 서비스 (Cargo 워크스페이스 `domain`/`web` + SeaORM + 쿠키 세션 + SPA). studyFinal의 결과물. DB 종류는 `DATABASE_URL`의 스킴.
- `lab/`: 단계별 미니 예제 (`examples/s01~s18_*.rs`, `src/mock_demo.rs`, `static/api.js`). 문서의 코드는 여기서 컴파일·실행으로 검증합니다.
- `.github/workflows/`: `ci.yml`(테스트), `docker-publish.yml`(태그 시 이미지 게시).
- 옛 커밋 메시지의 `studyN`은 커리큘럼 재편 이전 번호입니다. 대응표는 [README.md](../README.md)에 있습니다.

## 2. 작업 전 확인 사항

- 새 작업을 시작하기 전에 `study0.md` ~ `study10.md`, `studyMidpoint.md`, `study11.md` ~ `study18.md`, `studyFinal.md` 순서로 확인하여 지금까지의 학습 진도와 맥락을 파악합니다.
- `demo/`, `project/`, `lab/` 코드가 study 문서 설명과 어긋나지 않는지 확인합니다. 코드를 바꿨다면 해당 문서의 코드·결과·오류 노트를 함께 고칩니다.
- 변경 후에는 `demo`(`cargo test`), `project`(`cargo test --workspace`), `lab`(`cargo test --lib --examples`)를 확인합니다. CI(`ci.yml`)가 같은 명령을 실행합니다.
- 새 studyN.md 문서를 추가/수정할 때는 이전 study 문서와 이어지는 흐름을 유지합니다.
- Rust 프로젝트는 `target/` 빌드 산출물이 매우 크므로 반드시 `.gitignore`에 포함되어 있어야 합니다. (SQLite DB 파일, `.env`도 마찬가지)
- 코드를 고쳤으면 **커밋 전에 최소한 `cargo check`를 통과**시킵니다. 가능하면 `cargo run`으로 서버를 띄워 `curl`로 실제 응답을 확인한 뒤 커밋합니다.
- Rust는 컴파일 시간이 깁니다. 첫 빌드는 수 분이 걸릴 수 있으니 백그라운드로 돌리고, 의존성을 추가할 때는 한 번에 모아서 추가하는 편이 효율적입니다.
- `cargo run`을 백그라운드로 띄운 채로는 `target/` 안의 실행 파일이 잠겨 `cargo build`가 실패합니다(Windows 파일 잠금). 재빌드 전에 서버 프로세스를 먼저 종료합니다.

## 3. 커밋 규칙

- 커밋 관련 규칙은 [COMMIT_GUIDE.md](./COMMIT_GUIDE.md) 문서를 따릅니다.

## 4. 언어 규칙

- 문서 내용, 커밋 설명: 한글
- 커밋 타입(prefix), 코드, 식별자, 파일명: 영어

## 5. 에이전트별 문서

- Claude: [.claude/CLAUDE.md](../.claude/CLAUDE.md)
- Grok: [.grok/GROK.md](../.grok/GROK.md)
- Codex: [.codex/AGENTS.md](../.codex/AGENTS.md)

## 6. 인수인계 원칙

- 작업을 마칠 때는 다음 에이전트/세션이 이어받을 수 있도록 변경 내용과 다음 단계 후보를 간단히 남깁니다.
- 불확실한 결정은 추측하지 말고 study 문서 내용과 [Notion 학습 목표 페이지](https://app.notion.com/p/Axum-3d6d7dec5e2e814581a4e96fc36e6c71)를 근거로 삼습니다.
- 이 문서나 커밋 가이드에 없는 새로운 컨벤션이 필요해지면, 추측해서 각자 다르게 처리하지 말고 이 문서를 먼저 갱신합니다.
