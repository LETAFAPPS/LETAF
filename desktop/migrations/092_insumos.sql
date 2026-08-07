-- Insumo (matéria-prima): item de estoque CONSUMÍVEL usado na receita de
-- produtos (core/src/insumo). Entidade sincronizada; o estoque evolui por um
-- ledger append-only (insumo_movements), espelhando products/stock_movements.
CREATE TABLE IF NOT EXISTS insumos (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    description TEXT,
    unit TEXT NOT NULL DEFAULT 'un',
    stock_quantity REAL NOT NULL DEFAULT 0,
    min_stock REAL NOT NULL DEFAULT 0,
    cost_price REAL,
    barcode TEXT,
    active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_insumos_company ON insumos(company_id);
CREATE INDEX IF NOT EXISTS idx_insumos_unsynced ON insumos(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_insumos_pull ON insumos(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_insumos_manifest ON insumos(company_id, id);

-- Ledger de movimentos de insumo (append-only) — deltas idempotentes.
CREATE TABLE IF NOT EXISTS insumo_movements (
    id TEXT PRIMARY KEY,
    company_id TEXT NOT NULL,
    insumo_id TEXT NOT NULL,
    delta REAL NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    order_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deleted_at TEXT,
    synced INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_company ON insumo_movements(company_id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_unsynced ON insumo_movements(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_insumo ON insumo_movements(company_id, insumo_id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_pull ON insumo_movements(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_manifest ON insumo_movements(company_id, id);
