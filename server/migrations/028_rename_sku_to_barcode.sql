-- Renomeia o campo `sku` para `barcode` (clarifica semântica).
--
-- Regras aplicadas (AI_RULES.md §11):
-- - `barcode` será o código usado pelo leitor de barras no PDV (Fase 2).
-- - Mantém os dados existentes (RENAME COLUMN preserva valores e índices).

ALTER TABLE products RENAME COLUMN sku TO barcode;
