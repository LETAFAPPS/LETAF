-- Adiciona FK subcategory_id em products (nullable).
ALTER TABLE products ADD COLUMN IF NOT EXISTS subcategory_id UUID REFERENCES categories(id);
