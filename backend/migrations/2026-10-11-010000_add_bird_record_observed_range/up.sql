-- 鸟类调查「重要记录」的时间范围检索。
--
-- observed_at 存的是原文（2025年3月19日 / 2023年5月底 / 2007年），既排不了序
-- 也比不了大小。这里把它归一成 [observed_from, observed_to] 两个 DATE 列，
-- 查询时按区间求交集：
--
--     observed_from <= 查询上界 AND observed_to >= 查询下界
--
-- 不完整的日期展开成「它表示的那段时间」，而不是拍到一个点上：「2023年5月底」
-- 落在 05-22 ~ 05-31，查 5 月下旬能查到，查 5 月上旬查不到；「2007年」整年都算。
-- 这才不会因为归一化而丢掉 docx 里的「底」「初」这类精度。
--
-- 用生成列（GENERATED ALWAYS ... STORED）而不是在导入脚本 / 应用层各算一遍：
-- 归一逻辑只有函数这一处，导入、界面新建、界面修改都自动同一套规则。

-- 把「时间」原文归一成 [起始日, 结束日] 的日期数组；认不出来的写法（只有文字
-- 没有日期）返回 NULL，不参与时间筛选。支持：
--
--   2025年3月19日 / 2025-3-19 / 2025.3.19 / 2025/3/19  → 当天
--   2019年5月           → 整月
--   2019年5月初         → 该月前 10 天
--   2023年5月底         → 该月后 10 天
--   2007年              → 整年
--
-- 日期非法（2023年2月30日）也返回 NULL：不能让一条手抖的记录把整次写入变成 500，
-- 捕获异常比在 SQL 里逐月校验天数可读得多。
CREATE FUNCTION bird_observed_bounds(value TEXT) RETURNS DATE[]
LANGUAGE plpgsql IMMUTABLE AS $$
DECLARE
    parts TEXT[];
    first_day DATE;
    last_day DATE;
BEGIN
    IF value IS NULL THEN
        RETURN NULL;
    END IF;

    parts := regexp_match(btrim(value), '^(\d{4})\s*年\s*(\d{1,2})\s*月\s*(\d{1,2})\s*日$');
    IF parts IS NULL THEN
        parts := regexp_match(btrim(value), '^(\d{4})[-./](\d{1,2})[-./](\d{1,2})$');
    END IF;
    IF parts IS NOT NULL THEN
        first_day := make_date(parts[1]::INT, parts[2]::INT, parts[3]::INT);
        RETURN ARRAY[first_day, first_day];
    END IF;

    parts := regexp_match(btrim(value), '^(\d{4})\s*年\s*(\d{1,2})\s*月\s*(初|底)?$');
    IF parts IS NOT NULL THEN
        first_day := make_date(parts[1]::INT, parts[2]::INT, 1);
        last_day := (first_day + INTERVAL '1 month - 1 day')::DATE;
        IF parts[3] = '初' THEN
            last_day := least(last_day, first_day + 9);
        ELSIF parts[3] = '底' THEN
            first_day := greatest(first_day, last_day - 9);
        END IF;
        RETURN ARRAY[first_day, last_day];
    END IF;

    parts := regexp_match(btrim(value), '^(\d{4})\s*年$');
    IF parts IS NOT NULL THEN
        RETURN ARRAY[make_date(parts[1]::INT, 1, 1), make_date(parts[1]::INT, 12, 31)];
    END IF;

    RETURN NULL;
EXCEPTION WHEN others THEN
    RETURN NULL;
END;
$$;

-- 生成列：`bird_observed_bounds(observed_at)` 返回 [起始, 结束]，
-- 下标 1 / 2 各取一段。observed_at 一改，这两列跟着变，不用应用层记着同步。
ALTER TABLE bird_records
    ADD COLUMN observed_from DATE
        GENERATED ALWAYS AS ((bird_observed_bounds(observed_at))[1]) STORED,
    ADD COLUMN observed_to DATE
        GENERATED ALWAYS AS ((bird_observed_bounds(observed_at))[2]) STORED;
