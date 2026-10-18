-- 每个根节点归属一份物种名录。
--
-- 一棵树的根（parent_id IS NULL）决定这棵树属于哪份名录：新增 species 节点时
-- 沿 parent_id 往上走到根，就按根的 list_id 往那份名录里自动补一条记录。
-- 非根节点不存名录归属，跟着根走——所以约束是「有名录归属」⟺「是根节点」。
ALTER TABLE taxa ADD COLUMN list_id UUID REFERENCES species_lists (id);

-- 存量根节点按子树里已有的名录记录反推归属：一棵子树（一份 xlsx 工作表）
-- 目前只会落在一份名录里，取记录最多的那份兜底。
UPDATE taxa AS root
SET list_id = picked.list_id
FROM (
    WITH RECURSIVE subtree(root_id, id) AS (
        SELECT id, id FROM taxa WHERE parent_id IS NULL
        UNION ALL
        SELECT s.root_id, t.id
        FROM taxa t JOIN subtree s ON t.parent_id = s.id
    )
    SELECT DISTINCT ON (s.root_id) s.root_id, r.list_id
    FROM subtree s
    JOIN species_records r ON r.taxon_id = s.id
    GROUP BY s.root_id, r.list_id
    ORDER BY s.root_id, count(*) DESC, r.list_id
) AS picked
WHERE root.id = picked.root_id;

-- 子树里一条记录都没有的根节点反推不出来，得人工指定名录（UPDATE taxa SET
-- list_id = ... WHERE id = ...），否则下面的约束加不上。
DO $$
DECLARE missing text;
BEGIN
    SELECT string_agg(scientific_name, ', ') INTO missing
    FROM taxa WHERE parent_id IS NULL AND list_id IS NULL;
    IF missing IS NOT NULL THEN
        RAISE EXCEPTION 'root taxa without a species list: %', missing;
    END IF;
END
$$;

ALTER TABLE taxa
    ADD CONSTRAINT taxa_list_id_on_roots_only
    CHECK ((parent_id IS NULL) = (list_id IS NOT NULL));
