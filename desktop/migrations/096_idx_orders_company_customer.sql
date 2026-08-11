-- Desempenho (§13): `find_by_customer`, `count_customer_orders` e
-- `count_customer_coupon_uses` filtram por (company_id, customer_id) mas só
-- havia índice por (company_id, created_at/updated_at/status). Sem este, cada
-- chamada varre TODOS os pedidos da empresa filtrando customer_id linha a
-- linha — e as duas contagens estão no caminho do CHECKOUT (validação de
-- cupom / regra de primeira compra), piorando com o volume. O `created_at`
-- ao final também serve o `ORDER BY created_at DESC` do histórico do cliente.
-- (O servidor Postgres já tem o equivalente `idx_orders_customer`, migração 007.)
CREATE INDEX IF NOT EXISTS idx_orders_company_customer
    ON orders(company_id, customer_id, created_at DESC);
