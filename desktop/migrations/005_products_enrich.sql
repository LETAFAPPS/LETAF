-- Enriquecimento da tabela products no SQLite (AI_RULES.md §5, §6).
--
-- Novos campos:
--   price           REAL (preço de venda, nullable)
--   cost_price      REAL (preço de custo, nullable)
--   stock_quantity  REAL NOT NULL DEFAULT 0 (quantidade em estoque)
--   sku             TEXT (código SKU, nullable)
--   unit            TEXT NOT NULL DEFAULT 'un' (unidade: un, kg, lt, etc)

ALTER TABLE products ADD COLUMN price REAL;
ALTER TABLE products ADD COLUMN cost_price REAL;
ALTER TABLE products ADD COLUMN stock_quantity REAL NOT NULL DEFAULT 0;
ALTER TABLE products ADD COLUMN sku TEXT;
ALTER TABLE products ADD COLUMN unit TEXT NOT NULL DEFAULT 'un';
