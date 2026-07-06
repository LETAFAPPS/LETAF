-- Número sequencial do pedido por empresa (AI_RULES.md §6, §11).
-- Não é auto-increment (§6): geração manual no service (MAX+1 por company_id).
ALTER TABLE orders ADD COLUMN number INTEGER NOT NULL DEFAULT 0;

-- Backfill: numera os pedidos existentes por ordem de criação dentro de cada empresa.
-- ROW_NUMBER() é suportado a partir do SQLite 3.25 (2018).
UPDATE orders AS o
SET number = (
    SELECT n
    FROM (
        SELECT id,
               ROW_NUMBER() OVER (PARTITION BY company_id ORDER BY created_at, id) AS n
        FROM orders
    ) AS numbered
    WHERE numbered.id = o.id
)
WHERE o.number = 0;

CREATE INDEX IF NOT EXISTS idx_orders_company_number ON orders(company_id, number);
