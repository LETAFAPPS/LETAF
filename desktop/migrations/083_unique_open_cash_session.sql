-- Uma empresa só pode ter UMA sessão de caixa aberta.
--
-- A checagem existia só no service (lê-e-decide), então dois terminais
-- offline abriam o caixa sem se enxergar. Quando a sessão do outro
-- chegava no pull, o `find_active` (ORDER BY opened_at DESC) passava a
-- apontar para a sessão ALHEIA e as vendas seguintes caíam no caixa do
-- outro operador. §7.
CREATE UNIQUE INDEX IF NOT EXISTS idx_cash_sessions_one_open
    ON cash_sessions(company_id)
    WHERE status = 'open' AND deleted_at IS NULL;
