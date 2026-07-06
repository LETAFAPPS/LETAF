-- Ledger append-only de movimentos de estoque (AI_RULES §6, §7).
-- Substitui o LWW sobre o `stock_quantity` ABSOLUTO por deltas idempotentes:
-- o servidor aplica `stock_quantity += delta` uma única vez por id (ON CONFLICT
-- DO NOTHING garante idempotência). Deltas são comutativos → sem overselling
-- em vendas offline concorrentes. Espelha cash_movements / wallet_movements.
CREATE TABLE IF NOT EXISTS stock_movements (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    product_id UUID NOT NULL,
    delta DOUBLE PRECISION NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    order_id UUID,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS idx_stock_movements_company ON stock_movements(company_id);
CREATE INDEX IF NOT EXISTS idx_stock_movements_unsynced ON stock_movements(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_stock_movements_product ON stock_movements(company_id, product_id);
