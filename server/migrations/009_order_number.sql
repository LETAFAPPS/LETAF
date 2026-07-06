-- Número sequencial do pedido por empresa (AI_RULES.md §6, §11).
-- Não é auto-increment (§6): geração manual no service (MAX+1 por company_id).
ALTER TABLE orders ADD COLUMN IF NOT EXISTS number BIGINT;

-- Backfill: numera os pedidos existentes por ordem de criação dentro de cada empresa.
WITH numbered AS (
    SELECT id,
           ROW_NUMBER() OVER (PARTITION BY company_id ORDER BY created_at, id) AS n
    FROM orders
)
UPDATE orders o
SET number = numbered.n
FROM numbered
WHERE o.id = numbered.id AND o.number IS NULL;

-- A partir daqui todos os novos pedidos devem ter number preenchido pelo service.
ALTER TABLE orders ALTER COLUMN number SET NOT NULL;
ALTER TABLE orders ALTER COLUMN number SET DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_orders_company_number ON orders(company_id, number);
