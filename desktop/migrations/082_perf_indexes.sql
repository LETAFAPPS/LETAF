-- Índices de desempenho que faltavam no SQLite.
--
-- `order_items` nasceu sem NENHUM índice (008_orders.sql): abrir o
-- detalhe de um pedido fazia varredura completa da tabela. O servidor
-- já tinha o equivalente desde 007_orders.sql.
CREATE INDEX IF NOT EXISTS idx_order_items_order
    ON order_items(company_id, order_id);

-- Listagem de pedidos ordena por data em toda página do histórico.
CREATE INDEX IF NOT EXISTS idx_orders_company_created
    ON orders(company_id, created_at DESC);
