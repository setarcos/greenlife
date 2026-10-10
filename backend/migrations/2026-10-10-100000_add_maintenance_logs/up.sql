-- 维护日志：记录一份名录的修订记录。
--
-- 对应《北京大学燕园校区物种名录.xlsx》各名录「说明」表里的
-- 「修订笔记 / 修订人 / 修订说明 / 物种附录」四列。
--
-- list_id 可空：NULL 表示不属于任何一份名录的全局日志。
-- 名录被删除时它下面的日志一并删除（ON DELETE CASCADE）——日志脱离名录没有意义，
-- 留着只会指向一个不存在的名录。
CREATE TABLE maintenance_logs (
    id UUID PRIMARY KEY NOT NULL,
    list_id UUID REFERENCES species_lists (id) ON DELETE CASCADE,
    -- 「修订笔记」是自由文本，不是严格日期（原表里混用 20230402、202605、
    -- Excel 序列号 44824、"2022.11.25 - 至今"）。导入脚本会把前三种归一成
    -- yyyy-mm-dd / yyyy-mm，其余文字原样保留，所以用 VARCHAR。
    entry_date VARCHAR(50),
    author VARCHAR(200),
    summary TEXT NOT NULL,
    species_appendix TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT maintenance_logs_summary_not_blank CHECK (btrim(summary) <> '')
);

CREATE INDEX maintenance_logs_list_id_idx ON maintenance_logs (list_id);
