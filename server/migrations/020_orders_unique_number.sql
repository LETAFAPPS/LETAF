-- Garante unicidade do número de pedido por empresa (AI_RULES.md §6, §11).
-- Sem este constraint, pedidos criados concorrentemente podem receber o mesmo número.
-- O índice já existe (idx_orders_company_number), apenas tornamos ele UNIQUE.
CREATE UNIQUE INDEX IF NOT EXISTS idx_orders_company_number_unique
    ON orders(company_id, number);
