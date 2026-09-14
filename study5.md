# sqlx + SQLite로 영속화하기 (Study 5)

지금까지의 게시글은 **서버를 끄면 사라졌습니다.** `Arc<RwLock<Vec<Post>>>`는 프로세스 메모리니까요. 이제 SQLite에 저장합니다.

**이 문서에서 만드는 것**: 마이그레이션 + 커넥션 풀 + `repo` 계층 + `PostRow` ↔ `Post` 변환

---

## 1. sqlx는 ORM이 아니다

Rust의 DB 접근 방법은 크게 세 가지입니다.

| 도구 | 성격 | 비유 |
|---|---|---|
| **sqlx** | **비동기 SQL 툴킷.** SQL을 직접 쓰고, 결과를 구조체에 매핑 | MyBatis, `pg` + 수동 매핑 |
| **SeaORM** | sqlx 위에 얹힌 **본격 ORM.** 엔티티, 관계, 쿼리 빌더 | TypeORM, Hibernate |
| **Diesel** | **동기** ORM/쿼리 빌더. 스키마를 Rust 타입으로 생성 | jOOQ |

이번엔 **sqlx**를 씁니다. 이유:

- **SQL이 그대로 보입니다.** ORM이 어떤 쿼리를 만드는지 추측할 필요가 없습니다. N+1 문제가 숨을 곳이 없습니다.
- Notion 학습 목표가 "sqlx 또는 Diesel"입니다.
- `studyFinal`에서 **SeaORM**으로 같은 것을 다시 만들어 비교합니다. (`nest.js`에서 TypeORM → Prisma로 비교했던 것과 같은 구도)

```powershell
cargo add sqlx --no-default-features --features runtime-tokio,sqlite,chrono,macros,migrate
```

| feature | 의미 |
|---|---|
| `runtime-tokio` | tokio 런타임에서 동작 (`async-std`도 선택 가능) |
| `sqlite` | SQLite 드라이버 (`postgres`, `mysql`로 바꾸면 DB만 교체됨) |
| `chrono` | `DateTime<Utc>` ↔ DB 시각 타입 매핑 |
| `macros` | `sqlx::migrate!` 등 매크로 |
| `migrate` | 마이그레이션 실행기 |

> `--no-default-features`를 준 이유: 기본값에 `runtime-async-std`, `tls-native-tls` 등이 딸려와 빌드가 무거워집니다. 필요한 것만 켭니다.

### ⚠️ `query!` 매크로 vs `query` 함수

sqlx의 간판 기능은 **컴파일 타임 SQL 검증**입니다.

```rust
// 매크로 버전: 컴파일할 때 실제 DB에 접속해 쿼리를 검증하고, 반환 타입까지 추론한다
let post = sqlx::query_as!(Post, "SELECT * FROM posts WHERE id = ?", id).fetch_one(db).await?;

// 함수 버전: 런타임에 검증한다
let post = sqlx::query_as::<_, Post>("SELECT * FROM posts WHERE id = ?").bind(id).fetch_one(db).await?;
```

매크로는 강력하지만 **빌드할 때 `DATABASE_URL`이 가리키는 DB가 살아 있어야** 합니다(또는 `cargo sqlx prepare`로 만든 `.sqlx/` 오프라인 캐시가 있어야 합니다). CI나 남의 머신에서 "DB가 없어서 빌드가 안 되는" 상황이 생기기 쉽습니다.

**이 학습 저장소는 환경 의존성을 줄이려고 함수 버전을 씁니다.** 실무에서 sqlx를 쓴다면 매크로 + `cargo sqlx prepare` 조합을 진지하게 고려하세요. 오타 난 컬럼명을 컴파일 타임에 잡아주는 건 정말 큰 이점입니다.

---

## 2. 마이그레이션

`migrations/` 폴더에 SQL 파일을 순서대로 놓습니다.

### `migrations/0001_create_posts.sql`

```sql
CREATE TABLE IF NOT EXISTS posts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT NOT NULL,
    content    TEXT NOT NULL,
    author     TEXT NOT NULL,
    -- SQLite에는 배열 타입이 없다. 태그는 JSON 문자열로 저장하고 앱에서 변환한다.
    tags       TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
```

- 파일 이름은 `<버전>_<설명>.sql` 형식입니다. sqlx가 앞의 숫자로 순서를 정합니다.
- sqlx는 `_sqlx_migrations` 테이블을 만들어 **어디까지 적용했는지 기록**합니다. 이미 적용된 파일은 건너뜁니다.
- **한 번 적용된 마이그레이션 파일은 절대 수정하지 마세요.** sqlx가 체크섬을 저장해 두기 때문에 내용이 바뀌면 실행을 거부합니다. 변경이 필요하면 **새 파일을 추가**합니다. (study6에서 실제로 `0002_...`를 추가합니다)

| | Spring Boot | NestJS | Axum + sqlx |
|---|---|---|---|
| 도구 | Flyway / Liquibase | TypeORM migration / `prisma migrate` | `sqlx::migrate!` |
| 자동 생성 | 수동 | 엔티티 diff로 자동 생성 가능 | **수동** (SQL 직접 작성) |
| 스키마 자동 동기화 | `ddl-auto: update` | `synchronize: true` | **없음** |

**"엔티티를 고치면 테이블이 알아서 바뀌는" 기능이 sqlx에는 없습니다.** 처음엔 불편하지만, 운영 DB에서 예상 못 한 DDL이 실행되는 사고도 없습니다.

---

## 3. 커넥션 풀 — `src/db.rs`

```rust
pub type DbPool = Pool<Sqlite>;

pub async fn init(database_url: &str) -> anyhow::Result<DbPool> {
    let options = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5));

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}
```

### 옵션 하나하나가 다 이유가 있습니다

| 옵션 | 없으면 생기는 일 |
|---|---|
| `create_if_missing(true)` | DB 파일이 없을 때 `unable to open database file` 에러. **기본값이 false**라서 첫 실행에서 바로 막힌다 |
| `journal_mode(Wal)` | 기본 rollback journal 모드에서는 **읽기와 쓰기가 서로를 차단**한다. WAL을 켜면 읽는 동안에도 쓸 수 있다 |
| `busy_timeout(5s)` | 기본값 0 → 쓰기 락 경합이 나면 **즉시** `database is locked` 에러. 5초 기다리게 하면 대부분 해결된다 |
| `max_connections(5)` | 무제한으로 열면 DB가 죽는다. **SQLite는 쓰기가 한 번에 하나**라 크게 잡아도 의미가 적다 |
| `acquire_timeout(5s)` | 풀이 고갈됐을 때 영원히 대기하지 않게 |

> 💡 **SQLite를 서버 DB로 쓸 때 WAL + busy_timeout은 사실상 필수**입니다. 이 둘을 안 켜고 동시 요청을 받으면 `database is locked`가 랜덤하게 터집니다. Postgres로 바꾸면 이 고민은 사라집니다(대신 서버를 따로 띄워야 합니다).

### `sqlx::migrate!("./migrations")`는 매크로다

**컴파일 시점에** 폴더의 SQL 파일 내용을 읽어 **바이너리 안에 문자열로 박아 넣습니다.** 그래서:

- 배포할 때 `migrations/` 폴더를 같이 안 보내도 됩니다. 실행 파일 하나면 충분합니다. (study10의 Docker 이미지가 작아지는 이유)
- 반대로, SQL 파일을 고쳤으면 **다시 컴파일**해야 반영됩니다.

---

## 4. `AppState` 교체

```rust
// Before (study2~4)
#[derive(Clone)]
pub struct AppState {
    posts: Arc<RwLock<Vec<Post>>>,
    next_id: Arc<AtomicI64>,
}

// After
#[derive(Clone)]
pub struct AppState {
    pub db: DbPool,
}
```

**`Arc`로 감싸지 않은 것에 주목하세요.** `sqlx::Pool`은 **내부가 이미 `Arc`** 라서 `Clone`이 포인터 복사입니다. 한 번 더 감싸면 이중 간접 참조만 늘어납니다.

> 라이브러리 타입을 `AppState`에 넣을 때는 **그 타입이 이미 `Arc`인지 먼저 확인**하는 습관을 들이세요. `sqlx::Pool`, `reqwest::Client`, `redis::Client` 등 대부분의 커넥션 타입이 그렇습니다.

---

## 5. `PostRow` ↔ `Post` — DB 모양과 API 모양 분리

SQLite에는 배열 타입이 없습니다. `tags`를 어떻게 저장할까요?

| 방법 | 장점 | 단점 |
|---|---|---|
| **JSON 문자열 컬럼** ← 선택 | 단순. 마이그레이션 1개 | 태그로 검색/집계하기 나쁨 |
| 별도 `tags` 테이블 + 조인 | 정규화, 검색 가능 | 테이블 2~3개, 조인 쿼리 |
| 쉼표 구분 문자열 | 가장 단순 | 태그에 쉼표가 있으면 깨짐 |

학습용이라 JSON 문자열을 골랐습니다. 그러면 **DB의 행 모양과 API 응답 모양이 달라집니다.** 타입을 둘로 나눕니다.

```rust
/// API 응답으로 나가는 모양
#[derive(Debug, Clone, Serialize)]
pub struct Post {
    pub id: i64,
    pub tags: Vec<String>,          // ← 배열
    // ...
}

/// DB 한 행의 모양
#[derive(Debug, Clone, FromRow)]
pub struct PostRow {
    pub id: i64,
    pub tags: String,               // ← JSON 문자열
    // ...
}

impl From<PostRow> for Post {
    fn from(row: PostRow) -> Self {
        Self {
            id: row.id,
            // 저장된 JSON이 깨져 있어도 500을 내지 않고 빈 배열로 처리한다.
            tags: serde_json::from_str(&row.tags).unwrap_or_default(),
            // ...
        }
    }
}
```

### `#[derive(FromRow)]`

sqlx가 **"DB 행 → 구조체"** 매핑 코드를 생성합니다. 규칙은 단순합니다.

- **필드 이름 = 컬럼 이름**이어야 합니다. 다르면 `#[sqlx(rename = "...")]`.
- 필드 타입이 컬럼 타입과 호환돼야 합니다. 안 맞으면 **런타임**에 `ColumnDecode` 에러가 납니다. (매크로 버전을 쓰면 이걸 컴파일 타임에 잡습니다)
- 쿼리에 없는 컬럼을 필드로 두면 `ColumnNotFound` 에러. 그래서 `SELECT *` 대신 **컬럼을 명시**하는 편이 안전합니다.

### `impl From<A> for B`의 관용성

`From`을 구현하면 세 가지가 공짜로 따라옵니다.

```rust
let post = Post::from(row);          // 1. 명시적 변환
let post: Post = row.into();         // 2. Into (자동으로 따라옴)
rows.into_iter().map(Post::from)     // 3. 이터레이터에 함수로 넘기기
```

세 번째가 특히 유용합니다. 핸들러가 이렇게 한 줄이 됩니다.

```rust
Ok(Json(rows.into_iter().map(Post::from).collect()))
```

---

## 6. `repo` 계층 — `src/posts/repo.rs`

### 조회

```rust
pub async fn list(db: &DbPool) -> Result<Vec<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>(
        r#"
        SELECT id, title, content, author, tags, created_at, updated_at
        FROM posts
        ORDER BY id DESC
        "#,
    )
    .fetch_all(db)
    .await
}

pub async fn find(db: &DbPool, id: i64) -> Result<Option<PostRow>, sqlx::Error> {
    sqlx::query_as::<_, PostRow>("SELECT ... FROM posts WHERE id = ?")
        .bind(id)
        .fetch_optional(db)
        .await
}
```

| 메서드 | 반환 | 결과가 없으면 |
|---|---|---|
| `.fetch_all(db)` | `Vec<T>` | 빈 벡터 |
| `.fetch_one(db)` | `T` | **에러** (`RowNotFound`) |
| `.fetch_optional(db)` | `Option<T>` | `None` ← **404 처리에 딱 맞음** |
| `.execute(db)` | `QueryResult` | (INSERT/UPDATE/DELETE용) |

**"없음"을 에러가 아니라 `Option`으로 받는 게 핵심**입니다. `fetch_one`을 쓰면 "없음"과 "DB 장애"가 똑같이 `sqlx::Error`가 되어 404와 500을 구분할 수 없습니다.

- `r#"..."#`는 **raw 문자열**입니다. 안에서 `\`나 `"`를 이스케이프하지 않아도 됩니다. SQL을 여러 줄로 쓸 때 편합니다.
- `?`는 **바인딩 자리표시자**입니다(Postgres는 `$1`). `.bind(id)`로 값을 넣습니다. **문자열 연결로 SQL을 만들지 않으므로 SQL 인젝션이 원천 차단**됩니다.

### 생성 — `RETURNING`

```rust
pub async fn insert(db: &DbPool, input: &CreatePost) -> Result<PostRow, sqlx::Error> {
    let now: DateTime<Utc> = Utc::now();
    let tags = serde_json::to_string(&input.tags).unwrap_or_else(|_| "[]".to_string());

    sqlx::query_as::<_, PostRow>(
        r#"
        INSERT INTO posts (title, content, author, tags, created_at, updated_at)
        VALUES (?, ?, ?, ?, ?, ?)
        RETURNING id, title, content, author, tags, created_at, updated_at
        "#,
    )
    .bind(&input.title)
    /* ... */
    .fetch_one(db)
    .await
}
```

`RETURNING` 절 덕분에 **INSERT와 "방금 넣은 행 조회"가 왕복 한 번**으로 끝납니다. `last_insert_rowid()`를 얻어 다시 SELECT할 필요가 없습니다. (SQLite 3.35+, Postgres는 오래전부터 지원)

### 부분 수정 — `COALESCE`

```rust
sqlx::query_as::<_, PostRow>(
    r#"
    UPDATE posts
    SET title      = COALESCE(?, title),
        content    = COALESCE(?, content),
        tags       = COALESCE(?, tags),
        updated_at = ?
    WHERE id = ?
    RETURNING ...
    "#,
)
.bind(input.title.as_ref())     // Option<&String> → NULL 또는 값
.bind(input.content.as_ref())
.bind(tags)
.bind(Utc::now())
.bind(id)
.fetch_optional(db)
.await
```

**`COALESCE(?, column)` = "바인딩 값이 NULL이면 기존 값을 유지"**. 이 트릭 덕분에 필드 조합(제목만 / 본문만 / 둘 다 …)마다 SQL을 따로 만들지 않아도 됩니다.

핵심은 `Option<T>`를 `.bind()`하면 **`None` → SQL `NULL`** 로 들어간다는 점입니다. Rust의 `Option`과 SQL의 `NULL`이 자연스럽게 이어집니다.

> 필드가 많아지면 `sqlx::QueryBuilder`로 동적 SQL을 만드는 방법도 있습니다. 하지만 `COALESCE`로 해결되는 수준이면 이쪽이 훨씬 읽기 쉽습니다.

### 삭제 — `rows_affected`

```rust
pub async fn delete(db: &DbPool, id: i64) -> Result<bool, sqlx::Error> {
    let result = sqlx::query("DELETE FROM posts WHERE id = ?")
        .bind(id)
        .execute(db)
        .await?;

    Ok(result.rows_affected() > 0)   // 0이면 그런 id가 없었다는 뜻
}
```

---

## 7. 핸들러가 오히려 짧아졌다

```rust
pub async fn list(State(state): State<AppState>) -> AppResult<Json<Vec<Post>>> {
    let rows = repo::list(&state.db).await?;     // sqlx::Error → AppError 자동 변환
    Ok(Json(rows.into_iter().map(Post::from).collect()))
}

pub async fn get_one(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<Json<Post>> {
    repo::find(&state.db, id)
        .await?
        .map(|row| Json(Post::from(row)))
        .ok_or_else(|| AppError::not_found("post", id))
}

pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult<StatusCode> {
    if repo::delete(&state.db, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::not_found("post", id))
    }
}
```

락을 잡고 벡터를 뒤지던 코드가 전부 사라졌습니다. **study4에서 만들어 둔 `AppError`가 여기서 값을 합니다.**

```rust
#[error(transparent)]
Database(#[from] sqlx::Error),        // ← 이 한 줄을 추가했더니
```

`repo::list(&state.db).await?` 의 `?` 하나로 DB 오류가 500 JSON 응답까지 자동으로 흘러갑니다. **에러 처리 코드를 핸들러에 한 줄도 안 썼습니다.**

### 계층 구조 정리

```
handler.rs   ← HTTP 관심사 (상태 코드, JSON, 404 판단)
    ↓ repo::find(&state.db, id)
repo.rs      ← SQL만. HTTP를 전혀 모른다
    ↓
DB
```

`repo`는 `sqlx::Error`를 그대로 반환하고 **`AppError`를 모릅니다.** 이 분리가 중요합니다. `repo`를 CLI 도구나 배치 작업에서 재사용할 수 있고, `AppError`가 HTTP에 묶여 있어도 `repo`는 영향받지 않습니다.

---

## 8. 실행 및 검증

```powershell
cargo run
```

```
database ready: sqlite://data.db?mode=rwc
listening on http://127.0.0.1:3000
```

| # | 요청 | 응답 |
|---|---|---|
| 1 | `POST /posts` `{"title":"Persisted","content":"stored in sqlite","author":"while","tags":["sqlx","db"]}` | `201` `{"id":1,...,"tags":["sqlx","db"],...}` |
| 2 | `POST /posts` `{"title":"Second",...}` | `201` `{"id":2,...,"tags":[],...}` |
| 3 | `GET /posts` | `200` `[{"id":2,...},{"id":1,...}]` |
| 4 | `PATCH /posts/1` `{"content":"updated content"}` | `200` `{"id":1,"title":"Persisted","content":"updated content","tags":["sqlx","db"],...}` |
| 5 | `DELETE /posts/2` | `204` |
| 6 | `GET /posts/2` | `404` `{"error":"not_found","message":"post 2 not found"}` |

**4번을 잘 보세요.** `content`만 보냈는데 `title`과 `tags`가 그대로 살아 있습니다. `COALESCE`가 의도대로 동작했습니다.

### ⭐ 서버를 껐다 켠 뒤

```
7 list after restart → 200 | [{"id":1,"title":"Persisted","content":"updated content",
                              "tags":["sqlx","db"],"created_at":"2026-09-14T06:24:22.885338900Z",
                              "updated_at":"2026-09-14T06:24:22.994145900Z"}]
```

**데이터가 남아 있습니다.** study2~4에서는 재시작하면 전부 사라졌습니다. 이게 이번 study의 전부입니다.

생성된 파일:

```
demo/
├─ data.db        ← SQLite 데이터베이스
├─ data.db-shm    ← WAL 공유 메모리 (journal_mode=WAL을 켜서 생김)
└─ data.db-wal    ← Write-Ahead Log
```

`.gitignore`에 `*.db*`가 있으니 커밋되지 않습니다. **DB를 초기화하고 싶으면 이 세 파일을 지우고 다시 실행**하면 마이그레이션부터 새로 돕니다.

---

## 9. 처음 만난 오류 / 주의점

### 9-1. `unable to open database file`

`SqliteConnectOptions`의 `create_if_missing`이 **기본 false**입니다. 켜지 않으면 첫 실행에서 바로 이 에러를 만납니다.

```rust
.create_if_missing(true)    // ← 이것만 있으면 해결
```

URL 쪽에 `?mode=rwc`를 붙여도 같은 효과입니다(둘 다 해두면 확실합니다).

### 9-2. `database is locked`

동시 쓰기 요청이 겹칠 때 랜덤하게 터집니다. **재현이 잘 안 돼서 운영에서 처음 만나기 쉬운 유형**입니다.

```rust
.journal_mode(SqliteJournalMode::Wal)      // 읽기/쓰기 분리
.busy_timeout(Duration::from_secs(5))      // 락 대기
```

근본적으로 **SQLite는 동시에 쓰는 트랜잭션이 하나뿐**입니다. 쓰기가 많은 서비스라면 Postgres로 가야 합니다. sqlx를 쓰고 있다면 `Pool<Sqlite>` → `Pool<Postgres>`와 `?` → `$1` 정도만 바꾸면 됩니다.

### 9-3. `ColumnDecode` / `ColumnNotFound`

`query_as::<_, PostRow>`는 **런타임에** 매핑합니다. 컬럼명 오타나 타입 불일치가 컴파일러를 그냥 통과합니다.

| 에러 | 원인 |
|---|---|
| `ColumnNotFound("tags")` | SELECT에 컬럼을 안 넣었는데 구조체에는 필드가 있음 |
| `ColumnDecode { index: "created_at", .. }` | DB에 저장된 형식을 `DateTime<Utc>`로 못 읽음 |

- `SELECT *` 대신 **컬럼을 명시적으로 나열**하면 실수가 줄어듭니다.
- 이 부류를 컴파일 타임에 막고 싶으면 `query_as!` 매크로 + `cargo sqlx prepare`를 쓰세요. **이게 sqlx의 진짜 세일즈 포인트**입니다.

### 9-4. 적용된 마이그레이션 파일을 수정하면

```
VersionMismatch(1)
```

sqlx가 체크섬을 비교해서 거부합니다. **개발 중이라 상관없다면** `data.db*`를 지우고 다시 실행하고, **운영이라면 새 마이그레이션 파일을 추가**하세요.

---

## 10. 이번 study 정리

- **sqlx는 ORM이 아니라 SQL 툴킷**이다. 쿼리를 직접 쓰는 대신 무슨 SQL이 나가는지 100% 보인다.
- 마이그레이션은 `migrations/*.sql` + `sqlx::migrate!()`. **적용된 파일은 수정하지 않고 새로 추가**한다.
- SQLite를 서버에서 쓸 땐 `create_if_missing` + **WAL** + **busy_timeout**이 사실상 필수.
- `sqlx::Pool`은 내부가 이미 `Arc`다. `AppState`에 넣을 때 또 감싸지 않는다.
- **`fetch_optional`** 로 "없음"을 `Option`으로 받아야 404와 500을 구분할 수 있다.
- `RETURNING`으로 INSERT/UPDATE와 조회를 한 번에, `COALESCE(?, col)`로 부분 수정을 한 쿼리로.
- DB 모양(`PostRow`)과 API 모양(`Post`)을 나누고 `impl From`으로 경계에서 변환한다.
- study4의 `AppError`에 `Database(#[from] sqlx::Error)` **한 줄**을 추가한 것만으로 DB 오류가 500 JSON까지 자동으로 흘러간다.

다음: [study6.md](study6.md) — 회원 가입/로그인과 JWT 인증 extractor. (고급 기술 구현 1)
