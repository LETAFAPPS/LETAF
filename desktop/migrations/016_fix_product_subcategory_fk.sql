-- Corrige FK incorreta introduzida na migration 012.
-- subcategory_id foi adicionado com REFERENCES categories(id) por engano;
-- deve referenciar subcategories(id) — ou ficar sem FK, como category_id.
--
-- SQLite nao suporta ALTER COLUMN, entao recriamos a tabela.
-- Todos os dados sao preservados; subcategory_id era NULL para todos os
-- produtos (salvar com subcategoria falhava pela FK errada).

CREATE TABLE products_fixed (
    id             TEXT PRIMARY KEY,
    company_id     TEXT NOT NULL,
    name           TEXT NOT NULL,
    description    TEXT,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    deleted_at     TEXT,
    synced         INTEGER NOT NULL DEFAULT 0,
    price          REAL,
    cost_price     REAL,
    stock_quantity REAL NOT NULL DEFAULT 0,
    sku            TEXT,
    unit           TEXT NOT NULL DEFAULT 'un',
    category_id    TEXT,
    subcategory_id TEXT,
    active         INTEGER NOT NULL DEFAULT 1,
    image_data     TEXT
);

INSERT INTO products_fixed (
    id, company_id, name, description,
    created_at, updated_at, deleted_at, synced,
    price, cost_price, stock_quantity, sku, unit,
    category_id, subcategory_id,
    active, image_data
)
SELECT
    id, company_id, name, description,
    created_at, updated_at, deleted_at, synced,
    price, cost_price, stock_quantity, sku, unit,
    category_id, subcategory_id,
    active, image_data
FROM products;

DROP TABLE products;

ALTER TABLE products_fixed RENAME TO products;

CREATE INDEX IF NOT EXISTS idx_products_company_id ON products(company_id);
