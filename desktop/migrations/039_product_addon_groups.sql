-- Junção N:M Product ↔ AddonGroup (Fase 4) — espelha 041_*.sql do server.
CREATE TABLE IF NOT EXISTS product_addon_groups (
    company_id  TEXT NOT NULL,
    product_id  TEXT NOT NULL,
    group_id    TEXT NOT NULL,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (company_id, product_id, group_id)
);

CREATE INDEX IF NOT EXISTS idx_product_addon_groups_product
    ON product_addon_groups(company_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_addon_groups_group
    ON product_addon_groups(company_id, group_id);
