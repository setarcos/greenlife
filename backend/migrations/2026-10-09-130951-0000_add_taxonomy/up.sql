-- 分类阶元（界 > 门 > 纲 > 目 > 科 > 属 > 种）。
--
-- 用 PostgreSQL 枚举而不是 VARCHAR：非法阶元在数据库层就被拒绝，
-- 不需要靠应用层的字符串白名单兜底。
CREATE TYPE taxonomy_rank AS ENUM (
    'kingdom',
    'phylum',
    'class',
    'order',
    'family',
    'genus',
    'species'
);

-- 单表自引用分类树：任意阶元一行，parent_id 指向上一级。
--
-- 刻意允许「跳级」（例如苔藓名录缺目/科，属直接挂在纲下）：名录数据本身
-- 就存在这种缺口，硬补一个假的中间阶元是编造数据。数据库只要求父级
-- rank 严格高于子级，这条规则由应用层校验（见 taxonomy_models.rs）。
--
-- parent_id 用默认的 NO ACTION 而不是 RESTRICT：RESTRICT 在删父行时立即
-- 报错，连「一条语句删完整棵树」都做不到；NO ACTION 在语句末尾检查。
-- 应用层删除前会先显式检查子节点并返回 409，数据库约束只作兜底。
CREATE TABLE taxa (
    id UUID PRIMARY KEY NOT NULL,
    parent_id UUID REFERENCES taxa (id),
    rank taxonomy_rank NOT NULL,
    scientific_name VARCHAR(200) NOT NULL,
    chinese_name VARCHAR(200),
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    CONSTRAINT taxa_parent_not_self CHECK (parent_id IS NULL OR parent_id <> id),
    CONSTRAINT taxa_scientific_name_not_blank CHECK (btrim(scientific_name) <> '')
);

CREATE INDEX taxa_parent_id_idx ON taxa (parent_id);
CREATE INDEX taxa_rank_idx ON taxa (rank);

-- 同一父级下、同一阶元的学名不能重复。
--
-- 用 COALESCE 把 NULL 父级归一到哨兵 UUID：PostgreSQL 的唯一索引默认认为
-- 两个 NULL 互不相等，不加这层的话根节点可以无限重复。
CREATE UNIQUE INDEX taxa_sibling_unique ON taxa (
    COALESCE(parent_id, '00000000-0000-0000-0000-000000000000'::uuid),
    rank,
    scientific_name
);
