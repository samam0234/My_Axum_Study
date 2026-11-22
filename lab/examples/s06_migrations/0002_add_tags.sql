-- 이미 적용된 0001을 고치지 않고 새 파일로 쌓는다. (적용된 파일을 고치면 sqlx가 VersionMismatch로 거부한다)
ALTER TABLE posts ADD COLUMN tags TEXT NOT NULL DEFAULT '[]';
