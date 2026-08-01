-- `stock_movements` entrou na reconciliação anti-entropia: o manifesto local
-- pagina por (company_id, id). O ledger cresce sem limite, então sem este
-- índice a varredura seria table scan a cada ciclo de reconcile.

CREATE INDEX IF NOT EXISTS idx_stock_movements_manifest
    ON stock_movements(company_id, id);
