-- Índice para acelerar `find_updated_since` (sync pull do desktop).
--
-- Sem este índice, o `WHERE company_id = $1 AND updated_at > $2` faz
-- scan completo da tabela `products` a cada ciclo de sync — custo
-- linear no número total de produtos da empresa.
CREATE INDEX IF NOT EXISTS idx_products_company_updated_at
    ON products(company_id, updated_at DESC);
