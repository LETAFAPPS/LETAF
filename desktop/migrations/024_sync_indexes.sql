-- Índices compostos para acelerar o ciclo de sync (§13).
--
-- Regras aplicadas (AI_RULES.md §7, §13):
-- - find_unsynced filtra por (company_id, synced) — todos os domínios
-- - find_updated_since filtra por (company_id, updated_at) — todos os domínios

-- products
CREATE INDEX IF NOT EXISTS idx_products_company_synced
    ON products(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_products_company_updated
    ON products(company_id, updated_at);

-- users
CREATE INDEX IF NOT EXISTS idx_users_company_synced
    ON users(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_users_company_updated
    ON users(company_id, updated_at);

-- companies
CREATE INDEX IF NOT EXISTS idx_companies_synced
    ON companies(synced);
CREATE INDEX IF NOT EXISTS idx_companies_updated
    ON companies(updated_at);

-- customers
CREATE INDEX IF NOT EXISTS idx_customers_company_synced
    ON customers(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_customers_company_updated
    ON customers(company_id, updated_at);

-- categories
CREATE INDEX IF NOT EXISTS idx_categories_company_synced
    ON categories(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_categories_company_updated
    ON categories(company_id, updated_at);

-- subcategories
CREATE INDEX IF NOT EXISTS idx_subcategories_company_synced
    ON subcategories(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_subcategories_company_updated
    ON subcategories(company_id, updated_at);

-- orders
CREATE INDEX IF NOT EXISTS idx_orders_company_synced
    ON orders(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_orders_company_updated
    ON orders(company_id, updated_at);

-- business_hours
CREATE INDEX IF NOT EXISTS idx_business_hours_company_synced
    ON business_hours(company_id, synced);
CREATE INDEX IF NOT EXISTS idx_business_hours_company_updated
    ON business_hours(company_id, updated_at);
