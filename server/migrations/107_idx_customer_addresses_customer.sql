-- Desempenho (§13): `find_by_customer` filtra por (company_id, customer_id) e
-- só existiam índices de sync (company_id, synced/updated_at). Este índice
-- fecha o filtro conforme a base de endereços cresce.
CREATE INDEX IF NOT EXISTS idx_customer_addresses_company_customer
    ON customer_addresses (company_id, customer_id)
    WHERE deleted_at IS NULL;
