-- 鸟类调查·「鸟调记录」。
--
-- 一次鸟调（bird_surveys）是某一天的样线调查，表头是日期 / 领队 / 天气 / 备注；
-- 明细（bird_survey_records）是这次调查里逐条的鸟种观察，来源是 GPX 里的 waypoint。
-- 「鸟种数」不落库，读接口按明细里的不同鸟种现算 —— 明细一改就自动跟着变，
-- 不会出现表头和明细对不上的情况。
--
-- 日期用 DATE（鸟调按天组织，没有「月底」这类模糊写法）；weather 是自由文本，
-- 从 GPX 文件名（20180522北大鸟调多云转晴.gpx）里解析或手工填。
CREATE TABLE bird_surveys (
    id UUID PRIMARY KEY NOT NULL,
    survey_date DATE NOT NULL,
    -- 「领队」：导入时由用户输入；手工新建时可选。
    leader VARCHAR(200),
    -- 「天气」：文件名里的「多云转晴」这类描述。
    weather VARCHAR(100),
    -- 「备注」。
    note TEXT,
    -- 数据来源：导入时记原始 GPX 文件名；手工新建为空。
    source VARCHAR(255),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX bird_surveys_survey_date_idx ON bird_surveys (survey_date);

-- 一条鸟种观察。GPX 的每个 <wpt> 对应一行。
--
-- taxon_id 可以为空：GPX 里的中文名可能是旧名或名录里没有的名字
-- （如「普通楼燕」「北短翅莺」），此时保留原名、不关联分类，界面上标「未匹配」。
CREATE TABLE bird_survey_records (
    id UUID PRIMARY KEY NOT NULL,
    survey_id UUID NOT NULL REFERENCES bird_surveys (id) ON DELETE CASCADE,
    -- 关联的分类节点；匹配不上时为空。
    taxon_id UUID REFERENCES taxa (id),
    -- 从分类树抄来的学名；未匹配时为空。
    scientific_name VARCHAR(200),
    -- 鸟种中文名（GPX 的 <name>）；必填，未匹配时它就是唯一的名字。
    chinese_name VARCHAR(200) NOT NULL,
    -- 「数量」：GPX 的 <desc>（观察到的个体数）。
    quantity INTEGER,
    -- 观察时间。GPX 里是 UTC，导入时换算成北京时间存入（见 bird_handlers）。
    observed_at TIMESTAMP,
    -- 观察点的经纬度（GPX 的 lat / lon）。
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    -- 「备注」：GPX 的 gpxx:StreetAddress（幼鸟 / 1雌带5幼 / 雄…）。
    note TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT bird_survey_records_chinese_name_not_blank CHECK (btrim(chinese_name) <> '')
);

CREATE INDEX bird_survey_records_survey_id_idx ON bird_survey_records (survey_id);
CREATE INDEX bird_survey_records_taxon_id_idx ON bird_survey_records (taxon_id);
