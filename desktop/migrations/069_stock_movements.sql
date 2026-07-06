-- Ledger append-only de movimentos de estoque (AI_RULES §6, §7).
-- Substitui o LWW sobre o `stock_quantity` ABSOLUTO por deltas idempotentes:
-- cada mudança de estoque grava um movimento (delta) na MESMA transação que
-- atualiza o valor materializado. O sync aplica `stock_quantity += delta` uma
-- única vez por id → deltas são comutativos → sem overselling em vendas
-- offline concorrentes. Espelha cash_movements / wallet_movements.
CREATE TABLE IF NOT EXISTS stock_movements (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    product_id TEXT NOT NULL,
    delta REAL NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    order_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_stock_movements_company ON stock_movements(company_id);
CREATE INDEX IF NOT EXISTS idx_stock_movements_unsynced ON stock_movements(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_stock_movements_product ON stock_movements(company_id, product_id);
