-- Adiciona FK subcategory_id em products (nullable).
ALTER TABLE products ADD COLUMN subcategory_id TEXT REFERENCES categories(id);
