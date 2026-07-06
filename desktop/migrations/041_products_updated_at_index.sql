-- Índice equivalente ao 043 do server: acelera `find_updated_since`
-- (push do desktop coleta unsynced; pull verifica updates locais).
CREATE INDEX IF NOT EXISTS idx_products_company_updated_at
    ON products(company_id, updated_at DESC);
