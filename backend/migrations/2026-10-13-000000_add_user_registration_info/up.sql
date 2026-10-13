-- 用户注册信息。
--
-- 昵称用的是 users 表已有的 name 列，这里补上自助注册时收集的其余信息：
-- 真实姓名（必填）、学号/工号、单位、联系方式、微信号（后四项选填）。
--
-- 五列都可空：迁移之前已存在的账号没有这些资料，管理员手动建号时也不强制填。
-- 长度上限与 backend/src/models.rs 里的常量保持一致，改哪边都要同步另一边的校验。
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS real_name VARCHAR(50),
    ADD COLUMN IF NOT EXISTS student_id VARCHAR(50),
    ADD COLUMN IF NOT EXISTS organization VARCHAR(200),
    ADD COLUMN IF NOT EXISTS contact VARCHAR(200),
    ADD COLUMN IF NOT EXISTS wechat VARCHAR(100);
