-- 对应 greenlife 现有库 (public.users)。
-- IF NOT EXISTS 使该迁移在已经建好表的库上也能安全 apply（等同于给现有库打基线）。
CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY NOT NULL,
    name VARCHAR(10) NOT NULL,
    username VARCHAR(50) UNIQUE NOT NULL,
    password_hash VARCHAR(255) NOT NULL,
    role INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);
