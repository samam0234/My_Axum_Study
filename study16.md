# study16 — 운영 · CI/CD

> 🎯 **목표**: 서비스를 **계속 안전하게 굴리는** 체크리스트를 갖춘다: Docker/compose 운영 구성, GitHub Actions로 테스트·이미지 빌드 자동화, 로그·모니터링, 백업과 **복원 검증**. 설명보다 **체크리스트와 직접 돌려 본 결과** 중심이다.

| 항목 | 내용 |
|---|---|
| 관련 파일 | [.github/workflows/ci.yml](.github/workflows/ci.yml), [.github/workflows/docker-publish.yml](.github/workflows/docker-publish.yml), [demo/Dockerfile](demo/Dockerfile), [demo/compose.yaml](demo/compose.yaml), [demo/compose.postgres.yaml](demo/compose.postgres.yaml) |
| 로컬 검증 | 워크플로 YAML 문법 검증 + **워크플로가 실행하는 것과 같은 명령**(`cargo test`, `cargo check --no-default-features --features postgres`, `cargo test --workspace`, `cargo test --lib --examples`)을 로컬에서 실행해 통과 확인 · 백업/복원 실측(PostgreSQL, SQLite) |
| GitHub에서의 실제 실행 | 푸시 후 Actions 결과로 확인한다 (아래 ③-3 끝의 "실행 결과"에 기록) |

---

## ① 목표

1. 컨테이너 운영 설정(재시작·헬스체크·볼륨·비밀)을 체크리스트로 점검한다.
2. push/PR마다 **자동으로 테스트**하고, 태그를 푸시하면 **이미지를 빌드해 레지스트리에 올리는** 파이프라인을 만든다.
3. 로그와 최소한의 모니터링(헬스·알림)을 구성한다.
4. 백업을 **받는 것**이 아니라 **복원해 보는 것**까지 한다.

---

## ② 📖 이론

### CI와 CD

| | 하는 일 | 이 저장소 |
|---|---|---|
| **CI** (지속적 통합) | 변경이 올라올 때마다 **빌드·테스트를 자동 실행** → 깨지면 바로 알림 | `ci.yml` |
| **CD** (지속적 배포) | 통과한 것을 **이미지로 만들어 배포 가능한 상태로** (또는 서버에 자동 배포) | `docker-publish.yml` (이미지 게시까지. 서버 배포 job은 주석으로 준비) |

원칙: **main에 들어간 것은 항상 테스트를 통과한 것**이어야 한다. 테스트가 없으면 CI는 "컴파일되는지"만 본다 — 그래서 study9의 시험이 먼저다.

### 운영에서 계속 지켜야 할 것 — 네 축

```
① 올리기   이미지 빌드 → 레지스트리 → 서버      (CI/CD, 롤백용 태그)
② 지키기   헬스체크, 재시작, 비root, 시크릿       (compose, Dockerfile)
③ 보기     로그, 요청 ID, 메트릭, 알림            (tracing, 모니터링)
④ 되살리기 백업, 복원 연습                        (DB 백업 + 복원 검증)
```

---

## ③ 🛠️ 실기

### 3-1. Docker · compose 운영 체크리스트

[demo/compose.yaml](demo/compose.yaml):

```yaml
services:
  api:
    build: .
    image: axum-demo:latest
    ports: ["3000:3000"]
    environment:
      JWT_SECRET: ${JWT_SECRET:?JWT_SECRET 환경변수를 설정하세요}    # 없으면 컨테이너 시작을 거부
      RUST_LOG: demo=info,tower_http=info
    volumes:
      - api-data:/app/data          # DB
      - api-uploads:/app/uploads    # 업로드 파일
    restart: unless-stopped
volumes: { api-data: , api-uploads: }
```

```
이미지
□ 멀티 스테이지 (빌더 버리고 바이너리만)                          실측 160MB
□ 비root 사용자 (USER appuser)                                    실측 uid=1000
□ 의존성 레이어 캐싱 (Cargo.toml/lock 먼저)                       소스만 바뀌면 내 코드 컴파일 ~23초
□ .dockerignore에 target/ .env *.db                              
□ 비밀을 이미지에 굽지 않았다 (JWT_SECRET은 실행 시 주입)
□ 이미지 태그를 버전(또는 커밋 해시)으로 — latest만 쓰지 않는다 (롤백용)

실행
□ restart: unless-stopped  (프로세스가 죽거나 서버가 재부팅돼도 다시 뜬다)
□ HEALTHCHECK 있음 (curl /health)  → docker ps 에서 (healthy)
□ 볼륨: DB·업로드가 컨테이너 밖에 있다 (컨테이너를 지워도 데이터가 산다)
□ BIND_ADDR=0.0.0.0:3000, 포트는 필요한 것만 노출
□ SIGTERM으로 1초 안에 정상 종료 (docker stop 0.4초 실측)  ← 안 되면 배포마다 요청이 끊긴다
□ 리소스 제한(메모리/CPU)을 둔다 (한 컨테이너가 서버 전체를 먹지 않게)
□ 로그 드라이버/크기 제한 (json-file max-size) — 안 하면 로그가 디스크를 채운다
```

compose에서 DB 컨테이너와 앱을 함께 쓸 때(PostgreSQL): DB에 `healthcheck: pg_isready`, 앱에 `depends_on: { db: { condition: service_healthy } }`. 앱 이미지는 `--build-arg CARGO_FLAGS="--no-default-features --features postgres"`로 빌드한다 (study10·11).

### 3-2. GitHub Actions — CI: [.github/workflows/ci.yml](.github/workflows/ci.yml)

```yaml
name: ci

on:
  push:
    branches: [master]
  pull_request:

concurrency:                                  # 같은 브랜치에 새 푸시가 오면 이전 실행은 취소
  group: ci-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  demo:
    name: demo (sqlite 테스트 + postgres 빌드)
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: demo } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2          # target/과 ~/.cargo 캐시 — Rust CI에서 가장 효과가 큰 한 줄
        with: { workspaces: demo }
      - name: 테스트 (in-memory SQLite)
        run: cargo test
      - name: PostgreSQL feature로도 컴파일되는가
        run: cargo check --no-default-features --features postgres

  project:
    runs-on: ubuntu-latest      # 워크스페이스 전체 테스트 (도메인 서비스 시험)
    defaults: { run: { working-directory: project } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: project }
      - run: cargo test --workspace

  lab:
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: lab } }
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
        with: { workspaces: lab }
      - run: cargo test --lib --examples
```

| 항목 | 설명 |
|---|---|
| `on: push (master) / pull_request` | 메인에 올라갈 때와 PR 때 실행 |
| 프로젝트 3개 → job 3개 | 서로 의존하지 않아 **병렬**. 하나가 깨져도 나머지 결과를 본다 |
| `working-directory` | 저장소 루트가 아니라 각 크레이트 폴더에서 실행 |
| `Swatinem/rust-cache` | 의존성 컴파일 결과를 캐시. 없으면 매번 수 분 |
| `concurrency` | 연속 푸시 시 옛 실행을 취소해 러너 시간을 아낀다 |
| 테스트는 DB 서버 불필요 | in-memory SQLite + `oneshot`(study9) 덕분. PostgreSQL은 **컴파일만** 확인 (서비스 컨테이너로 실제 시험하려면 `services: postgres:` 추가) |
| 비밀 | 이 파이프라인에는 비밀이 **없다.** 키가 필요한 시험은 가짜로 (study13) |

CI에서 `cargo test`가 통과하려면 **서버·네트워크·외부 서비스 없이** 돌아야 한다. study9의 가짜들이 그 조건을 만든다.

### 3-3. GitHub Actions — CD: [.github/workflows/docker-publish.yml](.github/workflows/docker-publish.yml)

```yaml
name: docker-publish
on:
  push:
    tags: ["v*"]                  # git tag v0.1.0 && git push origin v0.1.0
  workflow_dispatch:              # 수동 실행 버튼도
permissions:
  contents: read
  packages: write                 # GHCR에 올리려면 필요

jobs:
  publish:
    runs-on: ubuntu-latest
    strategy:
      fail-fast: false            # 한 이미지가 실패해도 다른 이미지는 계속
      matrix:
        include:
          - { name: demo,    context: demo }        # 게시판 API
          - { name: project, context: project }     # 최종 서비스 (웹사이트 + API)
    steps:
      - uses: actions/checkout@v4
      - uses: docker/setup-buildx-action@v3
      - uses: docker/login-action@v3
        with: { registry: ghcr.io, username: "${{ github.actor }}", password: "${{ secrets.GITHUB_TOKEN }}" }
      - id: meta
        uses: docker/metadata-action@v5
        with:
          images: ghcr.io/${{ github.repository }}/${{ matrix.name }}
          tags: |
            type=semver,pattern={{version}}      # v0.1.0 → 0.1.0
            type=raw,value=latest
      - uses: docker/build-push-action@v6
        with:
          context: ${{ matrix.context }}
          push: true
          tags: ${{ steps.meta.outputs.tags }}
          cache-from: type=gha,scope=${{ matrix.name }}      # 레이어 캐시를 Actions 캐시에 (이미지별 범위)
          cache-to: type=gha,mode=max,scope=${{ matrix.name }}
```

| 항목 | 설명 |
|---|---|
| 트리거가 **태그** | 매 푸시마다 이미지를 만들지 않는다. **릴리스 의도가 있을 때만**. 이미지 태그가 버전과 1:1 |
| `GITHUB_TOKEN` | 워크플로가 자동으로 받는 토큰. **별도 비밀 등록 없이** GHCR에 올린다. `packages: write` 권한만 열어 준다 (최소 권한) |
| `metadata-action` | 태그 이름에서 이미지 태그를 만든다 → 롤백할 때 "어떤 버전이었나"가 남는다 |
| `cache-from/to: gha` | Dockerfile의 의존성 레이어 캐싱 트릭과 함께 쓰면 소스만 바뀐 빌드가 크게 줄어든다 |
| 서버 배포 job | 주석으로 준비: `appleboy/ssh-action`으로 서버에서 `docker compose pull && up -d`, 마지막에 **`curl /health`**(배포 직후 헬스체크, 실패하면 job 실패). 필요한 시크릿 3개(`DEPLOY_HOST/USER/SSH_KEY`)를 등록해야 켤 수 있다 |

> **자동 배포가 안전해지는 조건**: ① CI가 통과해야만 태그를 붙인다 ② 배포 직후 헬스체크가 실패하면 **자동으로 이전 태그로 되돌린다** (또는 사람이 보게 알린다) ③ 마이그레이션이 **앞뒤 버전 모두에서 동작**하도록 (컬럼 삭제는 두 번에 나눠서).

### 3-4. 로컬에서 같은 명령을 먼저 돌려 본 결과

```
demo     $ cargo test                                                      → 단위 7 + 통합 15 통과
demo     $ cargo check --no-default-features --features postgres           → Finished
project  $ cargo test --workspace                                          → 도메인 서비스 5 + 유틸 1 통과 (SQLite·PostgreSQL·MySQL 드라이버 포함)
lab      $ cargo test --lib --examples                                     → mock 3 + 서명 3 + 프롬프트/스트림 3 통과
워크플로 YAML                                                               → 문법 검증 통과 (jobs: demo, project, lab / publish)
```

**GitHub Actions 실제 실행 결과**: (푸시 후 기록)

### 3-5. 로그 · 모니터링 체크리스트

이 저장소가 **이미 갖고 있는 것**:

```
✔ 구조적 로그          tracing — 레벨(ERROR/WARN/INFO/DEBUG), 모듈 경로, key=value 필드
✔ 요청 ID              x-request-id 응답 헤더 = 오류 본문의 requestId = (TraceLayer의) 서버 로그
✔ 응답 시간            x-response-time-ms 헤더
✔ 헬스 엔드포인트      GET /health + Docker HEALTHCHECK
✔ 패닉 → 500 + 로그    CatchPanicLayer (연결이 끊기지 않고 로그에 패닉 메시지)
✔ 로그에 비밀 없음     DB 비밀번호 마스킹 (redacted_database_url)
```

**운영에서 추가할 것** (이 저장소에서 구현하지 않았다 — 도입 체크리스트):

```
□ 로그를 JSON 한 줄로 (tracing_subscriber의 json feature) → 로그 수집기(Loki, CloudWatch 등)가 필드로 검색
□ 로그 레벨: 운영 info, 문제 추적 때만 일시적으로 debug (RUST_LOG만 바꿔 재시작)
□ 로그 보관 기간·크기 제한 (컨테이너 로그가 디스크를 채우지 않게)
□ 메트릭: 요청 수, 오류율(5xx 비율), 지연 p50/p95/p99, DB 풀 사용량 → /metrics (Prometheus 형식)
□ 외부 헬스 감시: 서비스 바깥에서 1분마다 /health 호출 (업타임 서비스) — 서버가 통째로 죽은 것은 서버 안에서는 모른다
□ 알림: 5xx 비율 급증, 헬스 실패, 디스크 80%, 인증서 만료 14일 전 → 채팅/메일
□ 장애 대응: requestId로 로그 추적하는 절차를 문서화
□ 에러 추적(Sentry 류): 패닉/500을 스택과 함께 모은다
```

로그 읽는 법은 [study9 ③-5](study9.md).

### 3-6. ⭐ 백업 — 받는 것이 아니라 **복원해 보는 것**

#### SQLite (WAL 모드)에서 파일 복사는 안전하지 않다

실측: WAL 모드의 DB(`live.db` + `live.db-wal` + `live.db-shm`)에서 **`.db` 파일만 복사**하고 열어 봤더니:

```
files while connection open: ['live.db', 'live.db-shm', 'live.db-wal'] | live rows: 1000
naive copy of .db only  -> ERROR no such table: posts          ← 복사본에는 테이블조차 없다
VACUUM INTO snapshot    -> 1000 rows
backup API              -> 1000 rows
```

최근 쓰기는 아직 `-wal` 파일에 있고 본 파일에 반영되기 전이다. **백업을 받았다고 믿었는데 비어 있는** 가장 흔한 사고다. 안전한 방법:

```sql
VACUUM INTO '/backup/board-2026-11-22.db';      -- 일관된 스냅샷을 새 파일로 (실행 중에도 안전)
```

또는 SQLite의 온라인 백업 API(`sqlite3 live.db ".backup backup.db"`), 또는 앱을 잠시 멈추고 `-wal`/`-shm`까지 함께 복사.

#### PostgreSQL — `pg_dump` → 복원

```powershell
docker exec axumstudy-db-1 pg_dump -U postgres -Fc board > board.dump          # 커스텀 포맷 (압축, 선택 복원 가능)
docker exec axumstudy-db-1 psql -U postgres -c "create database board_restore"
docker exec -i axumstudy-db-1 pg_restore -U postgres -d board_restore < board.dump
```

실측: 원본 `posts 1, users 2` → 덤프 7,168 bytes → **새 DB로 복원 후 `posts 1, users 2, _sqlx_migrations 3`** 로 일치. 마이그레이션 기록까지 복원되어 앱을 새 DB에 붙여도 다시 마이그레이션하지 않는다.

#### 업로드 파일 (`uploads/`)

DB와 **한 세트**다. DB에는 파일명만 있고 파일이 없으면 깨진 이미지가 된다. 같은 시점에 함께 백업한다(볼륨 스냅샷, `tar`).

```
백업 체크리스트
□ 대상: DB + 업로드 파일 + (설정 값의 키 이름 목록; 비밀 값은 비밀 저장소에)
□ 방법: SQLite → VACUUM INTO / 백업 API (파일 복사 금지)   PostgreSQL → pg_dump 또는 PITR(WAL 아카이브)
□ 주기: 잃어도 되는 데이터 양(RPO)으로 정한다 (하루 1회? 1시간?)
□ 보관: 서버와 **다른 곳**(다른 디스크/리전/버킷), 보관 기간·개수, 암호화
□ ⭐ 복원 연습: 새 환경에서 복원 → 앱을 붙여 가입/조회가 되는지 (분기마다)
□ 복원 시간(RTO)을 쟀다: 장애 시 몇 분 안에 되살릴 수 있나
□ 백업 실패 알림 (조용히 실패하는 백업이 가장 위험하다)
```

---

## ④ 🔗 연동 포인트

| 이어지는 곳 | 어떻게 |
|---|---|
| **study9 테스트 → CI** | CI가 `cargo test`를 돌린다. 서버·네트워크 없이 도는 시험이 CI를 가능하게 한다 |
| **study10 Docker → CD** | `docker-publish.yml`이 같은 `Dockerfile`로 이미지를 만들어 GHCR에 올린다. 배포 후 점검표(study10)를 자동 스모크로 |
| **study11 DB** | CI에서 PostgreSQL feature 컴파일 확인. 시험까지 하려면 `services: postgres` + `TEST_DATABASE_URL` |
| **study13 시크릿** | 외부 서비스 키는 GitHub Secrets → 서버 환경변수. 이미지·저장소·로그에 없다 |
| **study14 버전** | 이미지 태그 = 릴리스 버전. API `Sunset` 날짜와 배포 일정을 맞춘다 |
| **study15 측정** | 모니터링의 지연/오류율 지표가 성능 작업의 **전후 비교 기준** |
| **README** | 상태 배지(CI 통과 여부)를 붙이면 저장소 첫 화면에서 보인다 |

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| CI에서만 실패 (로컬은 통과) | CI에는 `.env`, 로컬 파일(`data.db`), 설치된 도구가 없다 / 운영체제가 Linux | 시험이 환경에 의존하지 않게. CI와 같은 명령을 **깨끗한 체크아웃**에서 돌려 본다 |
| 모든 실행이 매번 수 분 | 캐시 없음 | `Swatinem/rust-cache`, Docker는 `cache-from: gha` |
| Docker 이미지 빌드가 CI에서 `curl`/네트워크 오류 | 빌드 중 네트워크로 파일을 받는 크레이트 (예: `utoipa-swagger-ui`) | `vendored` feature (study10) |
| `denied: permission_denied: write_package` | `packages: write` 권한이 없거나 저장소 설정이 패키지 쓰기를 막음 | 워크플로 `permissions`, 저장소 Settings → Actions → Workflow permissions |
| 태그를 푸시했는데 워크플로가 안 돎 | 태그 패턴이 안 맞음(`v*`) / 태그를 만들었지만 푸시 안 함 | `git push origin v0.1.0` |
| 배포 후 새 컨테이너가 계속 재시작 | 부팅 시 설정 검증 실패(짧은 시크릿), DB 연결 실패 | `docker logs`. 시작 시 검증이 이유를 바로 보여 준다 |
| `docker compose up -d`가 옛 이미지를 씀 | 이미지 태그가 `latest` 고정이고 pull을 안 함 | `docker compose pull`, 버전 태그 사용 |
| 백업 파일을 열었더니 **테이블이 없음 / 데이터가 적음** | SQLite WAL에서 `.db`만 복사 (실측) | `VACUUM INTO` / 백업 API |
| 복원은 됐는데 앱이 마이그레이션을 또 실행/실패 | 마이그레이션 기록 테이블이 빠진 덤프 | 전체 덤프(`_sqlx_migrations` 포함)로 |
| 디스크가 갑자기 가득 참 | 컨테이너 로그·업로드·SQLite WAL이 무한 증가 | 로그 크기 제한, 업로드 정리 정책, 디스크 알림 |
| 배포 중 요청이 끊김 | `docker stop`이 SIGTERM을 못 받아 SIGKILL (10초) | `shutdown_signal`에 SIGTERM (study10, 실측 0.4초) |
| 시크릿이 Actions 로그에 찍힘 | `echo ${{ secrets.X }}` 또는 비밀을 URL에 포함 | 시크릿은 환경변수로만 넘기고 출력하지 않는다 (GitHub가 마스킹하지만 의존하지 않는다) |

---

## ⑥ 직접 해보기

1. **CI를 일부러 깨 보기**: 브랜치에서 통합 테스트 하나의 기대값을 틀리게 바꿔 PR을 올리고, Actions에서 어느 job·어느 시험이 실패하는지 읽는다. 고치면 초록으로 돌아오는지 본다.
2. **복원 리허설**: PostgreSQL `pg_dump` → 완전히 새 컨테이너/DB로 `pg_restore` → `axum-demo:pg` 이미지를 그 DB에 붙여 가입·조회가 되는지 확인한다. 걸린 시간을 적는다 (RTO).
3. **JSON 로그**: `tracing-subscriber`의 `json` feature를 켜고 `.json()` 포매터로 바꿔, 로그 한 줄을 `python -m json.tool`로 파싱해 본다. `requestId`로 필터하는 `grep`/`jq` 한 줄을 만든다.

---

## ⑦ ☐ 체크리스트

- ☐ CI(통합·테스트 자동화)와 CD(배포 가능한 산출물 자동화)의 차이를 설명할 수 있다
- ☐ `ci.yml`의 트리거·job 분리·캐시·`concurrency`가 각각 왜 있는지 설명할 수 있다
- ☐ 워크플로가 실행하는 명령을 **로컬에서 먼저** 돌려 통과를 확인했다
- ☐ 이미지 태그를 버전으로 남기는 이유(롤백)를 안다
- ☐ `GITHUB_TOKEN` + 최소 권한(`packages: write`)으로 GHCR에 올리는 방식을 안다
- ☐ 운영 Docker 체크리스트(비root, 헬스체크, 볼륨, 재시작, 정상 종료, 리소스 제한, 로그 제한)를 점검할 수 있다
- ☐ 로그·요청 ID·헬스로 장애를 추적하는 절차를 설명할 수 있다
- ☐ **SQLite WAL에서 `.db` 파일만 복사하면 안 되는 이유**와 안전한 방법 두 가지를 안다
- ☐ 백업을 받은 뒤 **새 환경에 복원해 앱을 붙여 보았다**

---

## ⑧ 다음 단계 연결

운영 기반이 갖춰졌으니 [study17](study17.md)에서는 서비스에 **실시간 기능**을 붙인다: WebSocket 채팅, SSE 알림, 대용량 파일 스트리밍 업로드 — 그리고 연결이 오래 유지되는 기능이 서버 운영(종료·타임아웃·프록시)에 미치는 영향을 다룬다.
