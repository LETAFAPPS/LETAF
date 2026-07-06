-- Campos de contato e endereço detalhado da empresa (nullable).
-- SQLite não suporta `IF NOT EXISTS` em ADD COLUMN; o sqlx só roda cada
-- migração uma vez (controlado por `_sqlx_migrations`).
ALTER TABLE companies ADD COLUMN whatsapp     TEXT;
ALTER TABLE companies ADD COLUMN email        TEXT;
ALTER TABLE companies ADD COLUMN instagram    TEXT;
ALTER TABLE companies ADD COLUMN document     TEXT;
ALTER TABLE companies ADD COLUMN neighborhood TEXT;
ALTER TABLE companies ADD COLUMN zip_code     TEXT;
ALTER TABLE companies ADD COLUMN city         TEXT;
ALTER TABLE companies ADD COLUMN uf           TEXT;
