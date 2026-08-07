-- Ficha técnica (receita): ligação produto↔insumo com a quantidade consumida
-- por unidade vendida. Espelha product_addon_groups (regravada no upsert do
-- produto), com FKs para companies/products/insumos.
CREATE TABLE IF NOT EXISTS product_ingredients (
    company_id  UUID NOT NULL REFERENCES companies(id),
    product_id  UUID NOT NULL REFERENCES products(id),
    insumo_id   UUID NOT NULL REFERENCES insumos(id),
    quantity    DOUBLE PRECISION NOT NULL DEFAULT 0,
    sort_order  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (company_id, product_id, insumo_id)
);
CREATE INDEX IF NOT EXISTS idx_product_ingredients_product ON product_ingredients(company_id, product_id);
CREATE INDEX IF NOT EXISTS idx_product_ingredients_insumo ON product_ingredients(company_id, insumo_id);
