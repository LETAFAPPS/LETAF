-- Adiciona quantidade de produtos por página exibida na grade (default 20).
-- SQLite não suporta IF NOT EXISTS em ADD COLUMN, mas migrations só rodam
-- uma vez por número (rastreado pela tabela _sqlx_migrations).
ALTER TABLE companies
    ADD COLUMN products_per_page INTEGER NOT NULL DEFAULT 20;
