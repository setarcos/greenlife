-- 鸟类调查·「重要记录」。
--
-- 对应《北京大学燕园校区鸟类记录整理.docx》里每个物种下面的观察记录行：
-- 「记录人 + 时间 + 于某地记录 + 备注」。docx 里的日期写法很杂
-- （2025年3月19日 / 2023年5月底 / 2019年5月初 / 2007年 / 2025.5.19），
-- 归一成严格日期会丢掉「月底」「冬」这类精度，所以 observed_at 用 VARCHAR
-- 存原文——与 maintenance_logs.entry_date 同一个取舍。
--
-- taxon_id 指向分类树里的物种节点（docx 按物种组织，一条记录只属于一个物种）。
-- scientific_name / chinese_name 是导入时从分类树抄下来的冗余列，与
-- species_records 的处理一致：既省一次 JOIN，也让记录在物种改名前保持原文。
CREATE TABLE bird_records (
    id UUID PRIMARY KEY NOT NULL,
    taxon_id UUID NOT NULL REFERENCES taxa (id),
    scientific_name VARCHAR(200) NOT NULL,
    chinese_name VARCHAR(200),
    -- 「记录人」：可能是一串名字（陈炜、杨延军等），也可能是观鸟者的网名（“南楼月”），
    -- 也可能是空的（只有日期和事件，例如「2023年6月3日，猫捕杀」）。
    observer VARCHAR(200),
    -- 「时间」：原文，见上面的说明。
    observed_at VARCHAR(100),
    -- 「地点」：校园内的小地名（未名湖 / 中水池 / 东北门草坪），很多记录没写。
    location VARCHAR(200),
    -- 「备注」：记录正文去掉记录人 / 时间 / 地点之后剩下的部分（数量、行为、是否首笔等）。
    note TEXT,
    -- 「内容来源」：导入时统一写 docx 文件名；手工补录时可以写别处。
    source TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT bird_records_scientific_name_not_blank CHECK (btrim(scientific_name) <> '')
);

CREATE INDEX bird_records_taxon_id_idx ON bird_records (taxon_id);
