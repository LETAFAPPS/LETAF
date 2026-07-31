-- Índices do pull keyset e da listagem de pedidos.
--
-- `keyset_pull_sql` filtra por (company_id, updated_at, id) em toda
-- entidade; sem o índice composto o Postgres fazia seq scan + sort a
-- cada ciclo de cada desktop conectado.
CREATE INDEX IF NOT EXISTS idx_finance_entries_pull
    ON finance_entries(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_wallet_accounts_pull
    ON wallet_accounts(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_wallet_movements_pull
    ON wallet_movements(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_cash_sessions_pull
    ON cash_sessions(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_cash_movements_pull
    ON cash_movements(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_subscription_invoices_pull
    ON subscription_invoices(company_id, updated_at, id);

-- Histórico de pedidos paginado (ORDER BY created_at DESC).
CREATE INDEX IF NOT EXISTS idx_orders_company_created
    ON orders(company_id, created_at DESC);
