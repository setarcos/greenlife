ALTER TABLE users
    DROP COLUMN IF EXISTS real_name,
    DROP COLUMN IF EXISTS student_id,
    DROP COLUMN IF EXISTS organization,
    DROP COLUMN IF EXISTS contact,
    DROP COLUMN IF EXISTS wechat;
