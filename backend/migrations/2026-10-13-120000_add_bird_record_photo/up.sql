-- 鸟类调查「重要记录」的照片。
--
-- 照片本体存在 species_photos 里（复用物种照片的上传 / 落盘 / 公开 URL 规则），
-- 这里只挂一个外键：一条记录可以没有照片，也可以指向该物种已有照片中的一张。
-- 图片文件不重复存一份，所以同物种的 9 张上限对「记录照片」同样适用。
--
-- 「照片必须和记录是同一个物种」由应用层校验（bird_handlers 里的
-- ensure_photo_matches_taxon）：数据库里这一列表达不了跨表比对，也没法用
-- CHECK 写出来。
--
-- ON DELETE SET NULL：照片被删除时记录本身保留，只是回到「没有照片」。
ALTER TABLE bird_records
    ADD COLUMN photo_id UUID REFERENCES species_photos (id) ON DELETE SET NULL;

CREATE INDEX bird_records_photo_id_idx ON bird_records (photo_id);
