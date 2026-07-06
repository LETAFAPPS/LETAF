-- Variações do produto (Fase 5) — espelha 044_product_variations.sql.
-- Por-produto em JSON: `[{title, selection, required, options:
-- [{name, price}]}]`. `selection` ∈ {single, multi, max_value}.
ALTER TABLE products ADD COLUMN variations TEXT;
