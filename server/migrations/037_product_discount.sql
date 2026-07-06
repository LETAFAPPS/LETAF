-- Campos de desconto aplicado no cardápio web.
-- `discount_kind`: "fixed" | "percent" | "bulk_fixed" | "bulk_percent" | NULL.
-- `discount_value`: valor em R$ ou %, conforme kind.
-- `discount_min_qty`: quantidade mínima para aplicar bulk_* (NULL para os
--   demais kinds).
ALTER TABLE products
    ADD COLUMN IF NOT EXISTS discount_kind     TEXT,
    ADD COLUMN IF NOT EXISTS discount_value NUMERIC(14, 2),
    ADD COLUMN IF NOT EXISTS discount_min_qty  DOUBLE PRECISION;
