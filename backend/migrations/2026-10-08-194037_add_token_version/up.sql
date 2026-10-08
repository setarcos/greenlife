-- 给 users 加一个 token 版本号。
--
-- 每次「使已签发 token 失效」的敏感操作（目前是改密码）都会把它 +1；
-- JWT 里带上签发时的版本号，middleware 每请求回查数据库比对，
-- 不一致就返回 401。这样改密码后旧 token 立即失效，而不是等最长 24h 过期。
--
-- 已有行的默认值：1（与登录时签发的版本号一致）。
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS token_version INTEGER NOT NULL DEFAULT 1;
