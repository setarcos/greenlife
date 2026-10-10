-- 物种照片。
--
-- 图片文件本身存在 UPLOAD_PATH 指定的目录里，数据库只存相对路径
-- （形如 2025-03/<uuid>.jpg）：绝对路径属于部署细节，写进库里换台机器就失效。
-- 公开 URL 由反向代理单独提供，`UPLOAD_PATH/<file_path>` 对应
-- `/uploads/<file_path>`。
--
-- taxon_id 指向分类树里的物种节点，与 bird_records 的处理一致。
-- uploader_username 冗余保存上传时的用户名：用户改名后照片仍然显示当时是谁传的
-- （和 species_records 抄下 scientific_name 是同一个取舍）。
CREATE TABLE species_photos (
    id UUID PRIMARY KEY NOT NULL,
    taxon_id UUID NOT NULL REFERENCES taxa (id),
    -- 「拍摄人」：可能和上传者不是同一个人，可以留空。
    photographer VARCHAR(200),
    -- 「上传用户名」：上传时从 JWT 取，不随用户改名变化。
    uploader_username VARCHAR(50) NOT NULL,
    -- 「拍摄时间」：只精确到天。按年月分目录，所以用 DATE 而不是自由文本，
    -- 否则「2023年5月底」这类写法没法可靠地算出目录名。
    taken_at DATE NOT NULL,
    -- 「地点」。
    location VARCHAR(200),
    -- 「备注」。
    note TEXT,
    -- 「评分」：1-10，留空表示还没评。CHECK 与后端校验对齐。
    rating SMALLINT,
    -- 「是否重要记录」。
    is_important BOOLEAN NOT NULL DEFAULT FALSE,
    -- UPLOAD_PATH 下的相对路径，始终用 '/' 分隔。
    file_path TEXT NOT NULL,
    -- 原始文件名，仅作展示 / 排错用。
    original_filename VARCHAR(255),
    -- 字节数，方便排查「为什么这张图挂了」。
    file_size BIGINT NOT NULL,
    content_type VARCHAR(100),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT species_photos_rating_range CHECK (rating IS NULL OR (rating BETWEEN 1 AND 10))
);

CREATE INDEX species_photos_taxon_id_idx ON species_photos (taxon_id);
CREATE INDEX species_photos_taken_at_idx ON species_photos (taken_at);
