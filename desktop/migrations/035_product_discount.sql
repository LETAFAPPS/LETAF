-- Campos de desconto (kind / value / min_qty). Todos NULL = sem desconto.
ALTER TABLE products ADD COLUMN discount_kind    TEXT;
ALTER TABLE products ADD COLUMN discount_value   REAL;
ALTER TABLE products ADD COLUMN discount_min_qty REAL;
