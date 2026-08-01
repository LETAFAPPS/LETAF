-- Espelha desktop/083: uma sessão de caixa aberta por empresa.
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_sessions_one_open
    ON cash_sessions(company_id)
    WHERE status = 'open' AND deleted_at IS NULL;
