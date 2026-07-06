-- Estoque ilimitado (default false). SQLite não suporta IF NOT EXISTS
-- em ADD COLUMN, mas o sqlx só roda cada migration uma vez.
ALTER TABLE products
    ADD COLUMN unlimited_stock INTEGER NOT NULL DEFAULT 0;
