-- Ficha técnica (receita): ligação produto↔insumo com a quantidade consumida
-- por unidade vendida. Regravada inteira no update/sync (como
-- product_addon_groups); sem `synced`/`deleted_at` (viaja junto do produto).
CREATE TABLE IF NOT EXISTS product_ingredients (
    company_id  TEXT NOT NULL,
    product_id  TEXT NOT NULL,
    insumo_id   TEXT NOT NULL,
    quantity    REAL NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (company_id, product_id, insumo_id)
);
CREATE INDEX IF NOT EXISTS idx_product_ingredients_product ON product_ingredients(company_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_ingredients_insumo ON product_ingredients(company_id, insumo_id);
