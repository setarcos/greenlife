-- 名录的展示顺序：小的排前面，同值再按 name 排（见 taxonomy_handlers.rs::list_lists）。
--
-- 迁移只加列并给一个中性的默认值，**不预设任何名录的顺序**——顺序属于数据，
-- 由 STAFF / ADMIN 在「物种名录」页的编辑弹窗里改（PATCH /staff/taxonomy/lists/{id}）。
-- 默认 100 只保证新名录有一致的落点（排在显式设过小值的名录之后，同值按 name 排）。
ALTER TABLE species_lists ADD COLUMN position INTEGER NOT NULL DEFAULT 100;
