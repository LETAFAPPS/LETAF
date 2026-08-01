-- `stock_movements` passou a ter pull incremental e reconciliação.
--
-- O pull filtra por (company_id, updated_at, id) — mesmo keyset das demais
-- entidades grandes (ver 090). O manifesto pagina por (company_id, id).
-- Sem estes índices, o ledger (que cresce sem limite) faria sequential scan
-- a cada ciclo de 30 s.

CREATE INDEX IF NOT EXISTS idx_stock_movements_pull
    ON stock_movements(company_id, updated_at, id);

CREATE INDEX IF NOT EXISTS idx_stock_movements_manifest
    ON stock_movements(company_id, id);
