-- 一份名录（例如「高等植物名录」）。同一个物种可以同时出现在多份名录里。
CREATE TABLE species_lists (
    id UUID PRIMARY KEY NOT NULL,
    name VARCHAR(100) UNIQUE NOT NULL,
    description TEXT,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT species_lists_name_not_blank CHECK (btrim(name) <> '')
);

-- 名录里的一条物种记录。
--
-- taxon_id 指向分类树中「鉴定到的最细阶元」：通常是一个 species 节点；
-- 只鉴定到属的记录（各名录里的「属未定种」）则指向 genus 节点。
--
-- scientific_name / chinese_name 保存名录里写的名字原文，与分类树解耦：
-- 名录可能沿用异名，改名时不必连带改树。
CREATE TABLE species_records (
    id UUID PRIMARY KEY NOT NULL,
    list_id UUID NOT NULL REFERENCES species_lists (id) ON DELETE CASCADE,
    taxon_id UUID NOT NULL REFERENCES taxa (id),
    scientific_name VARCHAR(200) NOT NULL,
    chinese_name VARCHAR(200),
    distribution TEXT,
    note TEXT,
    source TEXT,
    record_no VARCHAR(50),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT species_records_scientific_name_not_blank CHECK (btrim(scientific_name) <> '')
);

CREATE INDEX species_records_list_id_idx ON species_records (list_id);
CREATE INDEX species_records_taxon_id_idx ON species_records (taxon_id);

-- 同一份名录里学名唯一。跨名录重名是允许的——这正是「几份名录」的意义。
CREATE UNIQUE INDEX species_records_list_scientific_unique
    ON species_records (list_id, scientific_name);
