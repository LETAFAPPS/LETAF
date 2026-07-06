-- Renomeia o campo `sku` para `barcode` (SQLite >= 3.25).
--
-- Regras aplicadas (AI_RULES.md §11):
-- - `barcode` é o código usado pelo leitor de barras no PDV (Fase 2).

ALTER TABLE products RENAME COLUMN sku TO barcode;
