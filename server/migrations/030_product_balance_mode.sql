-- Modo de codificação do EAN-13 emitido por balanças (somente para unit=kg).
--
-- Regras aplicadas (AI_RULES.md §11):
-- - 'weight': 5 dígitos variáveis = peso em gramas
-- - 'price':  5 dígitos variáveis = preço total em centavos
-- - default 'weight' é o caso mais comum (balanças que conhecem o preço/kg).

ALTER TABLE products ADD COLUMN IF NOT EXISTS balance_mode TEXT NOT NULL DEFAULT 'weight';

ALTER TABLE products DROP CONSTRAINT IF EXISTS products_balance_mode_check;
ALTER TABLE products ADD CONSTRAINT products_balance_mode_check
    CHECK (balance_mode IN ('weight', 'price'));
