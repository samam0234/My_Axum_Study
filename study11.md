# study11 — DB 확장 (PostgreSQL · MariaDB · MongoDB)

> 🎯 **목표**: 같은 게시판의 DB를 SQLite에서 **PostgreSQL · MariaDB · MongoDB**로 바꿔 연결해 보고, 코드의 **어디가 바뀌고 어디가 그대로인지**, 그리고 sqlx · SeaORM · 공식 MongoDB 드라이버의 설정 차이를 정리한다. (모두 실제 DB 컨테이너에 연결해 확인했다)

| 항목 | 내용 |
|---|---|
| 이 단계의 코드 변경 | `demo/`: `sqlite`/`postgres` **Cargo feature**, `db.rs`, `migrations_pg/`, `compose.postgres.yaml`, `$1` 플레이스홀더 · `project/`: `sqlx-postgres`·`sqlx-mysql` 드라이버, 검색 대소문자·UNIQUE 위반 판별 수정 · `lab/`: [s11_mongo.rs](lab/examples/s11_mongo.rs) |
| 검증 환경 | PostgreSQL 17, MariaDB 11, MongoDB 7 (Docker 컨테이너) |
| 선행 | [study6](study6.md) (sqlx CRUD), 최종 프로젝트의 SeaORM 구조는 [studyFinal](studyFinal.md) |

---

## ① 목표

1. `Pool<Sqlite>` → `Pool<Postgres>` 전환에서 **무엇이 바뀌는지**(타입·플레이스홀더·마이그레이션·연결 옵션)를 안다.
2. ORM(SeaORM)이 DB 교체를 어디까지 흡수하는지, 그리고 **흡수하지 못해 직접 겪은 차이**를 안다.
3. 관계형(SQL)과 문서형(MongoDB)의 모델링·쿼리·오류 처리 차이를 설명할 수 있다.
4. 컨테이너로 로컬 DB를 띄우고 환경변수 한 줄로 연결을 바꾼다.

---

## ② 📖 이론

### 왜 SQLite에서 옮기나

| SQLite의 한계 | 증상 | 해결 |
|---|---|---|
| **동시에 쓰는 트랜잭션이 하나뿐** | 쓰기가 몰리면 `database is locked` | PostgreSQL / MariaDB (행 단위 잠금, MVCC) |
| 서버가 둘 이상이면 파일을 못 나눈다 | 인스턴스마다 다른 DB | 네트워크로 접속하는 DB 서버 |
| 타입이 느슨하다 (날짜가 TEXT, 배열 없음) | `tags`를 JSON 문자열로 우회 | `TIMESTAMPTZ`, `TEXT[]`, `JSONB` |
| 백업·복제·권한 기능이 약함 | 파일 복사에 의존 | 서버 DB의 도구 |

### 바뀌는 층과 안 바뀌는 층

```
핸들러 / 서비스 / 모델(API 모양)   ← 안 바뀐다
─────────────────────────────────
repo (SQL 또는 ORM 호출)          ← sqlx: SQL 방언 차이만큼 바뀐다 / SeaORM: 거의 안 바뀐다
─────────────────────────────────
db.rs (연결 옵션, 풀 타입)         ← 바뀐다 (한 파일)
migrations (DDL)                  ← sqlx: 방언별 파일 / SeaORM: Rust 코드 하나
─────────────────────────────────
DATABASE_URL                      ← 바뀐다 (환경변수)
```

2번 층을 **한 파일(`db.rs`)로 가두는 것**이 이번 단계의 설계 목표다. 그래서 study6에서 `type DbPool`, `type Db` 별칭을 두었다.

### 관계형 vs 문서형

| | 관계형 (SQLite·PostgreSQL·MariaDB) | 문서형 (MongoDB) |
|---|---|---|
| 스키마 | 미리 정의(DDL), 마이그레이션 | 없음 (컬렉션은 첫 쓰기 때 생성), 검증은 앱이 |
| 관계 | 외래키 + JOIN | 임베딩(문서 안에 포함) 또는 id 참조 |
| 쿼리 | SQL 문자열 + 바인딩 | BSON 문서 (`doc!{...}`) |
| 배열 | 별도 테이블 또는 `TEXT[]`/`JSONB` | 문서 안에 그대로 |
| 트랜잭션·제약 | 강함 (UNIQUE, FK, CHECK) | 인덱스로 UNIQUE, 다중 문서 트랜잭션은 제한적 |
| 어울리는 곳 | 관계가 많고 정합성이 중요 | 구조가 자주 바뀌고 문서 단위로 읽고 쓰는 데이터 |

### 드라이버 · ORM 선택지

| 도구 | 성격 | 이 저장소 |
|---|---|---|
| **sqlx** | SQL을 직접 쓴다. DB별 feature(`sqlite`/`postgres`/`mysql`) | `demo/` |
| **SeaORM** (내부는 sqlx) | 엔티티·관계·마이그레이션을 Rust 코드로. DB 종류는 **URL의 스킴**으로 실행 시점에 결정 | `project/` |
| **mongodb** (공식 드라이버) | BSON 문서 CRUD | `lab/s11_mongo` |

---

## ③ 🛠️ 실기

### 3-1. PostgreSQL — `demo/` (sqlx)

#### (1) Cargo feature로 DB 종류를 고른다

```toml
# demo/Cargo.toml
sqlx = { version = "0.9.0", default-features = false, features = ["runtime-tokio", "chrono", "macros", "migrate"] }

[features]
default = ["sqlite"]
sqlite = ["sqlx/sqlite"]
postgres = ["sqlx/postgres"]        # cargo run --no-default-features --features postgres
```

feature가 꺼진 드라이버는 **컴파일 자체에서 빠진다.** 기본은 `sqlite`이고, PostgreSQL로 빌드할 때만 `--no-default-features --features postgres`.

#### (2) `db.rs` — 바뀌는 곳은 여기뿐

```rust
#[cfg(all(feature = "sqlite", feature = "postgres"))]
compile_error!("`sqlite`와 `postgres` feature는 동시에 켤 수 없습니다. 하나만 선택하세요.");

#[cfg(feature = "sqlite")]   pub type Db = sqlx::Sqlite;
#[cfg(feature = "postgres")] pub type Db = sqlx::Postgres;
pub type DbPool = sqlx::Pool<Db>;                     // ← Pool<Sqlite> → Pool<Postgres>

#[cfg(feature = "postgres")]
pub async fn init(database_url: &str) -> anyhow::Result<DbPool> {
    use sqlx::postgres::PgPoolOptions;

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(5))
        .connect(database_url)
        .await?;

    sqlx::migrate!("./migrations_pg").run(&pool).await?;   // PostgreSQL용 마이그레이션 폴더
    Ok(pool)
}
```

| 코드 | 설명 |
|---|---|
| `compile_error!` | 두 feature를 동시에 켜면 `Db`가 두 번 정의된다. **빌드 단계에서 명확한 메시지**로 막는다 |
| `pub type Db`, `DbPool` | repo 코드는 `DbPool`과 `QueryBuilder<Db>`만 쓴다 → **repo가 DB 종류를 모른다** |
| `init`의 SQLite 버전과 비교 | WAL·`busy_timeout`·`create_if_missing`·in-memory 분기가 **전부 사라진다.** 동시성은 서버가 처리하므로 **풀 크기만** 정한다 |
| `migrate!("./migrations_pg")` | 매크로가 **컴파일 시점**에 폴더를 읽는다. feature가 꺼진 쪽 폴더는 참조되지 않는다 |

#### (3) 플레이스홀더 `?` → `$1`

```rust
// 전: WHERE id = ?                     VALUES (?, ?, ?, ?, ?, ?, ?)
// 후: WHERE id = $1                    VALUES ($1, $2, $3, $4, $5, $6, $7)
sqlx::query_as::<_, PostRow>("SELECT ... FROM posts WHERE id = $1").bind(id)
```

PostgreSQL은 `$1, $2 …`만 받는다. **SQLite도 `$1`을 받는다** (실측: 같은 SQL로 SQLite 통합 테스트 15개가 그대로 통과). 그래서 두 DB가 **같은 SQL 문자열**을 쓸 수 있다. `QueryBuilder`는 DB에 맞는 플레이스홀더를 알아서 만든다.

> ⚠️ MySQL/MariaDB는 `?`만 받는다. 이 "같은 SQL" 요령은 SQLite ↔ PostgreSQL에서만 통한다.

#### (4) 방언 차이 — SQL 자체가 달라지는 곳

| 항목 | SQLite | PostgreSQL | 이 코드에서의 처리 |
|---|---|---|---|
| 자동 증가 PK | `INTEGER PRIMARY KEY AUTOINCREMENT` | `BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY` | 방언별 마이그레이션 파일 |
| 날짜 | `TEXT` (문자열로 저장) | `TIMESTAMPTZ` (시간대 포함) | Rust는 `DateTime<Utc>` 그대로 |
| `LIKE` 대소문자 | **무시** (ASCII) | **구분** | `LOWER(col) LIKE LOWER($n)` 로 통일 (아래 오류 노트) |
| 배열 | 없음 → JSON 문자열 | `TEXT[]`, `JSONB` | 일단 `TEXT`(JSON 문자열)로 **같은 코드 유지** (직접 해보기 1) |
| `RETURNING` | 지원 | 지원 | 같은 SQL |
| FK 인덱스 | 자동 아님 | **자동 아님** | `CREATE INDEX ... (author_id)` 명시 |
| 연결 | 파일 락·WAL | 풀 + 서버 | `db.rs`만 다름 |

PostgreSQL 마이그레이션 ([demo/migrations_pg/0001_create_posts.sql](demo/migrations_pg/0001_create_posts.sql)):

```sql
CREATE TABLE IF NOT EXISTS posts (
    id         BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    author     TEXT NOT NULL,
    tags       TEXT NOT NULL DEFAULT '[]',        -- SQLite와 같은 코드(PostRow.tags: String)를 쓰려고 JSON 문자열
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
```

#### (5) 실행

```powershell
# PostgreSQL 컨테이너 (호스트의 5432가 이미 쓰이면 PG_PORT로 바꾼다)
$env:PG_PORT = "55432"
docker compose -p axumstudy -f compose.postgres.yaml up -d

$env:DATABASE_URL = "postgres://postgres:postgres@127.0.0.1:55432/board"
cargo run --no-default-features --features postgres
```

[demo/compose.postgres.yaml](demo/compose.postgres.yaml)의 핵심은 `healthcheck: pg_isready` (DB가 **준비된 뒤에** 앱을 붙이기 위해)와 `"${PG_PORT:-5432}:5432"`(포트 충돌 회피)다.

실측 (PostgreSQL 17, 같은 `curl` 시나리오):

```
{"status":"ok"}
{"id":1,"username":"alice","email":"alice@example.com","createdAt":"2026-11-22T15:12:17.395524Z"}      ← 마이크로초 정밀도
{"error":{"code":"EMAIL_TAKEN",...}}                                                                  ← UNIQUE 위반 → 409 (is_unique_violation은 DB 종류와 무관)
201 Created  location: /posts/1
GET /posts?q=hello&tag=RUST → total 1                                                                  ← 대소문자 무시 검색
PATCH → 200 (updatedAt 갱신)          POST {"title":"",...} → 400 VALIDATION_ERROR + fields           DELETE → 204

public | _sqlx_migrations | table      ← 1, 2, 3 모두 success = t
public | posts | table     public | users | table
```

### 3-2. PostgreSQL · MariaDB — `project/` (SeaORM)

DB 종류를 **실행 시점에** URL로 고른다. 필요한 드라이버를 모두 컴파일해 두면 된다.

```toml
# project/Cargo.toml
sea-orm = { version = "1.1", default-features = false,
            features = ["sqlx-sqlite", "sqlx-postgres", "sqlx-mysql", "runtime-tokio-rustls", "macros", "with-chrono"] }
sea-orm-migration = { version = "1.1", default-features = false,
            features = ["sqlx-sqlite", "sqlx-postgres", "sqlx-mysql", "runtime-tokio-rustls"] }
```

```
DATABASE_URL=sqlite://board.db?mode=rwc                          # 기본
DATABASE_URL=postgres://postgres:postgres@127.0.0.1:55432/board  # PostgreSQL
DATABASE_URL=mysql://root:pw@127.0.0.1:53306/board               # MariaDB (MySQL 프로토콜)
```

엔티티·서비스·마이그레이션 코드는 **한 줄도 바뀌지 않는다.** 마이그레이션이 SQL이 아니라 Rust(`Table::create()...`)라서 SeaORM이 DB별 DDL을 만들어 준다.

실측:

| DB | 가입 | 중복 가입 | 글 작성 | 검색(`q=MARIADB`/`sea`) | 마이그레이션 테이블 |
|---|---|---|---|---|---|
| SQLite | ✅ | 409 | ✅ | ✅ | `seaql_migrations` |
| PostgreSQL 17 | ✅ | **409** (수정 후) | ✅ | ✅ (수정 후) | `seaql_migrations` |
| MariaDB 11 | ✅ | **409** | ✅ | ✅ | `seaql_migrations` |

**수정 전에는 PostgreSQL에서 두 가지가 깨졌다.** "ORM이 DB 차이를 흡수한다"는 말은 **스키마와 CRUD**에 대해서만 맞다.

1. **검색**: `Column::Title.contains(keyword)`는 그냥 `LIKE '%…%'`다. SQLite는 대소문자를 무시하지만 PostgreSQL은 구분해서 `q=sea`가 `SeaORM`을 못 찾았다.
   ```rust
   // 수정: 양쪽을 소문자로 맞춘다. (PostgreSQL 전용 ILIKE는 SQLite에서 못 쓴다)
   let pattern = format!("%{}%", keyword.to_lowercase());
   Condition::any()
       .add(Expr::expr(Func::lower(Expr::col(post::Column::Title))).like(pattern.clone()))
       .add(Expr::expr(Func::lower(Expr::col(post::Column::Body))).like(pattern))
   ```
2. **UNIQUE 위반 판별**: 예전 코드는 에러 **문자열**에 `"UNIQUE constraint failed"`가 들어 있는지 봤다. PostgreSQL의 메시지는 `duplicate key value violates unique constraint`라서 **중복 가입이 409가 아니라 500**이 됐다.
   ```rust
   // 수정: DB 종류와 무관하게 구조화된 에러를 쓴다
   if matches!(err.sql_err(), Some(sea_orm::SqlErr::UniqueConstraintViolation(_))) {
       DomainError::Conflict("이미 사용 중인 아이디 또는 이메일입니다".to_string())
   }
   ```

> 교훈: **오류를 문자열로 판별하면 DB를 바꾸는 순간 깨진다.** 그리고 "테스트가 SQLite로만 돌면" 이런 차이는 운영에서 처음 드러난다 → 최소한 핵심 시나리오를 대상 DB로 한 번은 돌린다.

MariaDB에서 추가로 보인 차이: 응답의 시각이 **초 단위**(`2026-11-22T15:56:43Z`)다. PostgreSQL은 마이크로초(`…15:12:18.036154Z`), SQLite는 나노초. 컬럼 타입(`timestamp`)의 정밀도 차이다. 시각을 **문자열로 비교하는** 시험이 있다면 깨진다.

### 3-3. MongoDB — [lab/examples/s11_mongo.rs](lab/examples/s11_mongo.rs)

```powershell
docker run -d --name axum-mongo -p 27017:27017 mongo:7
cd lab
cargo run --example s11_mongo        # MONGODB_URI 기본 mongodb://127.0.0.1:27017
```

모델 — **DB 모양**과 **API 모양**을 나누는 원칙은 그대로다.

```rust
#[derive(Debug, Serialize, Deserialize)]
struct PostDoc {
    #[serde(rename = "_id", skip_serializing_if = "Option::is_none")]
    id: Option<ObjectId>,                 // 저장 전에는 None → MongoDB가 채운다
    title: String,
    body: String,
    tags: Vec<String>,                    // ← 진짜 배열. JSON 문자열 우회가 필요 없다
    created_at: DateTime,                 // BSON 날짜
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PostOut { id: String, title: String, body: String, tags: Vec<String>, created_at: String }
```

연결과 CRUD:

```rust
let client = Client::with_uri_str(&uri).await?;
let posts: Collection<PostDoc> = client.database("board").collection("posts");

// Create — RETURNING이 없다. 저장된 _id만 돌려준다
let inserted = posts.insert_one(&post).await?;
let id = inserted.inserted_id.as_object_id().unwrap();

// Read (검색 + 정렬 + 페이지)
let filter = doc! { "$or": [
    { "title": { "$regex": &pattern, "$options": "i" } },
    { "body":  { "$regex": &pattern, "$options": "i" } },
] };
let total = posts.count_documents(filter.clone()).await?;
let cursor = posts.find(filter).sort(doc! { "created_at": -1 }).skip((page - 1) * limit as u64).limit(limit).await?;
let docs: Vec<PostDoc> = cursor.try_collect().await?;         // futures_util::TryStreamExt

// Update — 보낸 필드만 $set (COALESCE가 필요 없다)
posts.update_one(doc! { "_id": oid }, doc! { "$set": set }).await?;     // matched_count == 0 → 404

// Delete
posts.delete_one(doc! { "_id": oid }).await?;                          // deleted_count == 0 → 404
```

| 코드 | 설명 |
|---|---|
| `#[serde(rename = "_id")]` | Rust 필드 `id` ↔ MongoDB의 `_id` |
| `ObjectId::parse_str(&id)` | 경로의 id 문자열을 변환. **형식이 틀리면 404가 아니라 400** (`BAD_REQUEST`)으로 구분한다 |
| `regex::escape(s)` | 사용자 검색어를 `$regex`에 그대로 넣으면 **정규식 인젝션**(`(+` 같은 입력이 쿼리를 깨뜨리거나 느리게 만든다). escape해서 글자 그대로 찾는다 |
| `count_documents` + `find` | SQL의 `COUNT(*)` + `LIMIT/OFFSET`에 해당 |
| `.skip(...)`의 깊은 페이지 | 오프셋 방식이라 **느려진다** → 커서 방식(study14) |

실측:

```
POST /api/posts {"title":"Hello Mongo","body":"documents not rows","tags":["rust","mongo"]}
  → 201  location: /api/posts/6b03119820020fe35b595d7d
  → {"data":{"id":"6b03119820020fe35b595d7d","title":"Hello Mongo",...,"tags":["rust","mongo"],"createdAt":"2026-11-22T16:00:24.671Z"}}
GET   /api/posts?q=hello      → {"data":[...],"meta":{"page":1,"limit":10,"total":1,"totalPages":1}}   ← 스펙 v1 모양
PATCH /api/posts/{id} {"title":"Updated Mongo"} → 200 (body, tags 유지)
GET   /api/posts/not-an-id    → 400 BAD_REQUEST          GET /api/posts/000000000000000000000000 → 404 NOT_FOUND
GET   /api/posts?q=(+         → 200, 결과 0건 (정규식 특수문자가 escape되어 500이 나지 않는다)
DELETE → 204                   mongosh: db.posts.countDocuments() → 0
```

### 3-4. 설정 차이 한눈에

| | **sqlx** (`demo/`) | **SeaORM** (`project/`) | **mongodb** (`lab/`) |
|---|---|---|---|
| DB 선택 | **빌드 시** Cargo feature | **실행 시** URL 스킴 (드라이버는 미리 컴파일) | 별도 드라이버 |
| 연결 | `PgPoolOptions::new().max_connections(..).connect(url)` | `ConnectOptions::new(url).max_connections(..)` → `Database::connect` | `Client::with_uri_str(uri)` |
| 마이그레이션 | `.sql` 파일을 컴파일 시점에 박음, **방언별 폴더** | Rust 코드 (`MigrationTrait`), DB 공통 | 없음 (인덱스는 `create_index`) |
| 쿼리 | SQL 문자열 + `$n` 바인딩 | `Entity::find().filter(Condition…)` | `doc! { ... }` |
| 타입 매핑 | `FromRow` 구조체 (런타임 매핑, 컬럼 오타는 런타임 에러) | `Model` + `ActiveModel` | `Serialize/Deserialize` 구조체 |
| 보이는 SQL | **전부 보인다** | 안 보인다 (`sqlx_logging`으로 확인) | 해당 없음 |
| UNIQUE 위반 | `db_err.is_unique_violation()` | `err.sql_err()` → `UniqueConstraintViolation` (**문자열 비교 금지**) | 에러 코드 11000 (`WriteFailure`) |
| 대소문자 검색 | 직접 `LOWER`/`ILIKE` | `contains()`는 DB 따라 다름 → 직접 `LOWER` | `$regex` + `$options: "i"` |
| 어울리는 경우 | 쿼리 튜닝이 중요 | 관계가 복잡하고 CRUD 위주 | 문서 단위 데이터 |

PostgreSQL을 쓰는 이유 중 이 저장소에서 아직 안 쓴 것들 (노션 "DB → PostgreSQL" 정리 기준):

```sql
-- JSONB / 배열
SELECT * FROM posts WHERE meta @> '{"pinned": true}';           -- JSONB 포함 검색
SELECT * FROM posts WHERE 'spring' = ANY(tags);                  -- TEXT[] 안에서 찾기
CREATE INDEX ON posts USING GIN (meta);                          -- @> 검색을 빠르게

-- CTE: 글별 댓글 수를 먼저 구하고 붙이기
WITH cc AS (SELECT post_id, COUNT(*) AS cnt FROM comments GROUP BY post_id)
SELECT p.id, p.title, COALESCE(cc.cnt, 0) AS comment_count
FROM posts p LEFT JOIN cc ON cc.post_id = p.id WHERE p.deleted_at IS NULL
ORDER BY p.created_at DESC LIMIT 10 OFFSET 0;

-- 부분 인덱스: soft delete 목록에 딱 맞는다
CREATE INDEX ON posts (created_at) WHERE deleted_at IS NULL;
-- ILIKE '%검색%' 는 pg_trgm 확장 + GIN 인덱스. EXPLAIN ANALYZE로 Seq Scan / Index Scan 확인
```

---

## ④ 🔗 연동 포인트

- **환경변수가 스위치다**: `DATABASE_URL` 한 줄이 SQLite ↔ PostgreSQL ↔ MariaDB를 바꾼다(SeaORM). sqlx는 거기에 **빌드 feature**가 추가로 필요하다 (`Dockerfile`의 `ARG CARGO_FLAGS`, study10).
- **컨테이너 연결**: 앱 컨테이너에서 DB 호스트는 `localhost`가 아니라 **compose 서비스 이름**(`db`)이다. 실측: `axum-demo:pg` 이미지를 DB와 같은 네트워크에 띄워 `postgres://postgres:postgres@db:5432/board`로 연결했다.
- **API 모양은 DB와 무관해야 한다**: MongoDB 예제도 응답이 `{ data, meta }`, 오류가 `{ error: { code, message } }`로 같은 계약을 따른다. 프론트는 DB가 뭔지 모른다.
- **마이그레이션 운영**: SeaORM은 `up`/`down`을 쌍으로 요구해 롤백이 가능하다. sqlx `.sql`은 적용된 파일을 수정하면 `VersionMismatch` — 운영에서는 **항상 새 파일 추가**.
- **백업·복구**는 DB별로 다르다 → study16.
- **AI 미니 프로젝트와의 연결**: 노션에 따르면 DocuForge는 PostgreSQL(`jsonb`로 질문 근거 저장, 문서 삭제 시 질문도 `on_delete: cascade`), 나머지 AI 미니 프로젝트 3개는 MongoDB를 썼다. 이 단계에서 만든 PostgreSQL/MongoDB 연결이 그대로 study18의 오케스트레이터가 쓸 저장소가 된다.

---

## ⑤ 오류 노트

| 증상 | 원인 | 해결 |
|---|---|---|
| PostgreSQL 연결에서 `Postgres returned a non-UTF-8 string for its error message ... default lc_messages locale` | **호스트에 이미 PostgreSQL이 5432를 쓰고 있어서** 컨테이너가 아니라 그 서버로 접속했다(`localhost`가 먼저 호스트의 것으로 감). 그 서버가 비UTF-8 지역 메시지로 인증 오류를 보냄. 메시지가 오해를 부른다 | `netstat -ano \| findstr :5432`로 누가 쓰는지 확인. 컨테이너 포트를 바꾼다 (`PG_PORT=55432`) |
| PostgreSQL에서만 검색이 안 됨 | `LIKE`는 PostgreSQL에서 **대소문자 구분** | `LOWER(col) LIKE LOWER($n)` (sqlx) / `Func::lower` (SeaORM). `ILIKE`는 PostgreSQL 전용이라 SQLite와 같은 코드를 못 쓴다 |
| PostgreSQL에서만 중복 가입이 **500** | UNIQUE 위반을 에러 **메시지 문자열**로 판별 (SQLite 메시지만 알고 있음) | `err.sql_err()`의 `UniqueConstraintViolation` / sqlx의 `is_unique_violation()` |
| `error: the trait bound ... Sqlite ... Postgres` 류 타입 불일치 | `Pool<Sqlite>` 같은 구체 타입이 repo 곳곳에 박혀 있음 | `DbPool`/`Db` 별칭만 쓰게 한다 |
| 두 feature를 동시에 켜면 컴파일 에러 | `Db`가 두 번 정의됨 | 의도된 `compile_error!`. 하나만 고른다 |
| `cargo test --features postgres`에서 시험이 사라짐 | `tests/api.rs`가 `#![cfg(feature = "sqlite")]` (in-memory SQLite 전제) | 의도된 동작. PostgreSQL 시험은 `TEST_DATABASE_URL` + 별도 DB 필요 |
| `type annotations needed` / `PgRow` 디코드 실패 | PostgreSQL은 타입이 엄격 (`INT4` ↔ `i64` 불일치 등) | 컬럼 타입을 `BIGINT`로, Rust는 `i64`. SeaORM 엔티티의 `i32`는 `integer` 컬럼 |
| MySQL/MariaDB에서 `$1` 플레이스홀더 오류 | 그쪽은 `?`만 받는다 | 방언별 SQL 또는 ORM 사용 |
| `no associated function named up found for struct Migrator` | **trait 메서드는 trait을 import해야 쓸 수 있다** | `use crate::migration::MigratorTrait;` |
| `paginate().fetch_page(page)`가 한 페이지씩 밀림 | SeaORM의 `fetch_page`는 **0-based** | `page.saturating_sub(1)` |
| MariaDB에서 응답 시각이 초 단위로만 나옴 | `timestamp` 정밀도 | 정밀도가 필요하면 `datetime(6)`. 시각 문자열 비교 시험을 피한다 |
| `ObjectId` 파싱 에러가 500으로 나감 | 잘못된 id 문자열을 그대로 `unwrap` | `parse_str` 실패를 400으로 번역 |
| MongoDB 검색어에 `(`, `+`가 있으면 오류/지연 | 사용자 입력을 `$regex`에 직접 넣음 | `regex::escape` |
| 컨테이너 DB가 준비되기 전에 앱이 먼저 떠서 연결 실패 | DB 초기화에 시간이 걸림 | `healthcheck` + `depends_on: condition: service_healthy`, 또는 앱의 연결 재시도 |

---

## ⑥ 직접 해보기

1. **`tags`를 `TEXT[]`로**: PostgreSQL 마이그레이션에서 `tags TEXT[]`로 바꾸고 `PostRow.tags`를 `Vec<String>`으로 받는다. SQLite 쪽과 모델이 갈라지는 지점을 확인하고, `'rust' = ANY(tags)` 로 태그 필터를 다시 쓴다. (같은 코드를 포기하는 대가가 무엇인지 적는다)
2. **`EXPLAIN ANALYZE`**: PostgreSQL에 글 10만 건을 넣고(`generate_series`) `WHERE LOWER(title) LIKE '%abc%'`를 `EXPLAIN ANALYZE`로 본다. `Seq Scan`을 확인한 뒤 `pg_trgm` + GIN 인덱스를 만들어 `Index Scan`으로 바뀌는지 본다.
3. **MongoDB 인덱스**: `s11_mongo`에서 `created_at` 내림차순 인덱스와 `title` 텍스트 인덱스를 `create_index`로 만들고, `explain`으로 인덱스를 타는지 확인한다.

---

## ⑦ ☐ 체크리스트

- ☐ PostgreSQL 전환: `Pool`의 타입 인자를 `Sqlite`에서 `Postgres`로, 플레이스홀더 `?`를 `$1`로
- ☐ 컨테이너 PostgreSQL을 띄우고 `cargo run --no-default-features --features postgres`로 CRUD를 확인했다
- ☐ `demo/`에서 DB 종류가 나오는 파일이 `db.rs`(와 마이그레이션 폴더)뿐이라는 것을 확인했다
- ☐ SQLite와 PostgreSQL의 차이(자동 증가 PK, 날짜 타입, `LIKE` 대소문자, 배열/JSONB, FK 인덱스)를 말할 수 있다
- ☐ SeaORM이 URL 스킴으로 SQLite/PostgreSQL/MariaDB를 고르는 것을 `project/`로 확인했다
- ☐ 오류를 **문자열로 판별하면 안 되는** 이유(UNIQUE 위반 사례)를 설명할 수 있다
- ☐ MongoDB로 CRUD를 만들고 관계형과의 차이(스키마·배열·`ObjectId`·쿼리)를 설명할 수 있다
- ☐ 컨테이너끼리는 서비스 이름으로 접속한다는 것과 포트 충돌의 증상을 안다
- ☐ sqlx / SeaORM / mongodb의 설정 차이 표를 보지 않고 세 줄로 요약할 수 있다

---

## ⑧ 다음 단계 연결

백엔드가 어떤 DB 위에서도 같은 계약(`{ data, meta }` / `{ error }`)을 지키게 되었으니, [study12](study12.md)에서는 그 계약을 **프론트엔드**(UI 라이브러리·상태 관리·폼 라이브러리)가 어떻게 소비하는지, 그리고 개발 서버·CORS·쿠키 인증을 어떻게 맞물리는지 다룬다.
