-- Meta de reserva mensal do caixa da empresa e movimentos MANUAIS da
-- carteira do estabelecimento — espelha desktop/081.
-- Dinheiro em NUMERIC (exato, §13).
ALTER TABLE treasury_accounts ADD COLUMN reserve_goal NUMERIC(14,2) NOT NULL DEFAULT 0;

-- Aportes/retiradas manuais no caixa da empresa. Append-only: `amount`
-- é sempre positivo e a direção vem de `kind` ('deposit'/'withdraw').
CREATE TABLE treasury_movements (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    treasury_id UUID NOT NULL,
    kind TEXT NOT NULL,
    amount NUMERIC(14,2) NOT NULL,
    notes TEXT,
    created_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    updated_at TIMESTAMP WITHOUT TIME ZONE NOT NULL,
    deleted_at TIMESTAMP WITHOUT TIME ZONE,
    synced BOOLEAN NOT NULL DEFAULT FALSE
);

-- Extrato da carteira: mais recentes primeiro, sempre por empresa.
CREATE INDEX idx_treasury_movements_company_created
    ON treasury_movements(company_id, created_at);

-- Pull incremental por `updated_at` (§7).
CREATE INDEX idx_treasury_movements_company_updated
    ON treasury_movements(company_id, updated_at);
