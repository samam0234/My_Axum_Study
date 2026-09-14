-- 게시글 테이블
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

-- 목록은 항상 최신순으로 조회하므로 인덱스를 걸어둔다.
CREATE INDEX IF NOT EXISTS idx_posts_created_at ON posts (created_at DESC);
