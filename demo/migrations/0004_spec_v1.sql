-- 공통 API 스펙 v1에 맞추는 변경 (study6 ④).
--   · 글 본문 필드 이름: content → body
--   · 회원 표시 이름: username → nickname (로그인은 이메일로 한다)
--   · 댓글 테이블 추가
-- 이미 적용된 0001~0003을 고치지 않고 새 파일로 추가한다. (sqlx는 적용된 파일의 체크섬을 검사한다)
-- RENAME COLUMN은 SQLite 3.25+에서 지원한다. 데이터는 그대로 남는다.
ALTER TABLE posts RENAME COLUMN content TO body;
ALTER TABLE users RENAME COLUMN username TO nickname;

CREATE TABLE IF NOT EXISTS comments (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    -- 글이 지워지면 댓글도 함께 지운다. (SQLite는 sqlx가 foreign_keys를 켜 준다)
    post_id    INTEGER NOT NULL REFERENCES posts (id) ON DELETE CASCADE,
    author_id  INTEGER NOT NULL REFERENCES users (id),
    body       TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_comments_post_id ON comments (post_id);
