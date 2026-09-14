-- 회원 테이블
CREATE TABLE IF NOT EXISTS users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL UNIQUE,
    -- 평문 비밀번호는 절대 저장하지 않는다. Argon2 PHC 문자열만 저장.
    password_hash TEXT NOT NULL,
    created_at    TEXT NOT NULL
);

-- 게시글에 작성자(FK)를 추가한다.
-- 기존 행에는 값이 없으므로 NULL 허용. (이미 적용된 0001을 고치는 대신 새 파일로 추가)
ALTER TABLE posts ADD COLUMN author_id INTEGER REFERENCES users (id);

CREATE INDEX IF NOT EXISTS idx_posts_author_id ON posts (author_id);
