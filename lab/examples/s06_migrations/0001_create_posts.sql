-- 게시글 테이블. SQLite에는 배열 타입이 없어서 태그는 나중에 JSON 문자열로 저장한다(0002).
CREATE TABLE IF NOT EXISTS posts (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    title      TEXT NOT NULL,
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
