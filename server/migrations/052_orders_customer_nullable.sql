-- Permite `customer_id` nulo em pedidos do PDV anônimos (venda balcão
-- sem cliente cadastrado). Antes o desktop usava `Uuid::nil()` como
-- "sem cliente", o que violava a FK em Postgres no sync.

ALTER TABLE orders ALTER COLUMN customer_id DROP NOT NULL;
