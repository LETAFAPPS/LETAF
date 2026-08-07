-- Insumo (matéria-prima): item de estoque CONSUMÍVEL usado na receita de
-- produtos (core/src/insumo). Entidade sincronizada; o estoque evolui por um
-- ledger append-only (insumo_movements), espelhando products/stock_movements.
CREATE TABLE IF NOT EXISTS insumos (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    name TEXT NOT NULL,
    description TEXT,
    unit TEXT NOT NULL DEFAULT 'un',
    stock_quantity DOUBLE PRECISION NOT NULL DEFAULT 0,
    min_stock DOUBLE PRECISION NOT NULL DEFAULT 0,
    cost_price DOUBLE PRECISION,
    barcode TEXT,
    active BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS idx_insumos_company ON insumos(company_id);
CREATE INDEX IF NOT EXISTS idx_insumos_unsynced ON insumos(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_insumos_pull ON insumos(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_insumos_manifest ON insumos(company_id, id);

-- Ledger de movimentos de insumo (append-only) — deltas idempotentes.
CREATE TABLE IF NOT EXISTS insumo_movements (
    id UUID PRIMARY KEY,
    company_id UUID NOT NULL REFERENCES companies(id),
    insumo_id UUID NOT NULL,
    delta DOUBLE PRECISION NOT NULL,
    reason TEXT NOT NULL DEFAULT '',
    order_id UUID,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    synced BOOLEAN NOT NULL DEFAULT false
);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_company ON insumo_movements(company_id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_unsynced ON insumo_movements(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_insumo ON insumo_movements(company_id, insumo_id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_pull ON insumo_movements(company_id, updated_at, id);
CREATE INDEX IF NOT EXISTS idx_insumo_movements_manifest ON insumo_movements(company_id, id);
