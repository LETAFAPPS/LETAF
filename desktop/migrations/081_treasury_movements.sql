-- Meta de reserva mensal do caixa da empresa (0 = sem meta; service
-- valida >= 0) e movimentos MANUAIS da carteira do estabelecimento.
-- Espelha server/089_treasury_movements.sql.
ALTER TABLE treasury_accounts ADD COLUMN reserve_goal REAL NOT NULL DEFAULT 0;

-- Aportes/retiradas manuais no caixa da empresa. Append-only: `amount`
-- é sempre positivo e a direção vem de `kind` ('deposit'/'withdraw').
-- Dinheiro no SQLite = REAL (§13).
CREATE TABLE treasury_movements (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    treasury_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    amount REAL NOT NULL,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);

-- Extrato da carteira: mais recentes primeiro, sempre por empresa.
CREATE INDEX idx_treasury_movements_company_created
    ON treasury_movements(company_id, created_at);

-- Varredura da fila de push (synced = 0) — §7.5.
CREATE INDEX idx_treasury_movements_company_synced
    ON treasury_movements(company_id, synced);
