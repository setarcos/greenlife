-- 生成列依赖函数，先删列再删函数。
ALTER TABLE bird_records
    DROP COLUMN observed_from,
    DROP COLUMN observed_to;

DROP FUNCTION IF EXISTS bird_observed_bounds(TEXT);
