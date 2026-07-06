-- Enriquecimento da tabela products (AI_RULES.md §6).
--
-- Novos campos de domínio:
--   price           DOUBLE PRECISION (preço de venda, nullable)
--   cost_price      DOUBLE PRECISION (preço de custo, nullable)
--   stock_quantity  DOUBLE PRECISION NOT NULL DEFAULT 0 (quantidade em estoque)
--   sku             TEXT (código SKU, nullable)
--   unit            TEXT NOT NULL DEFAULT 'un' (unidade: un, kg, lt, etc)

ALTER TABLE products ADD COLUMN IF NOT EXISTS price NUMERIC(14, 2);
ALTER TABLE products ADD COLUMN IF NOT EXISTS cost_price NUMERIC(14, 2);
ALTER TABLE products ADD COLUMN IF NOT EXISTS stock_quantity DOUBLE PRECISION NOT NULL DEFAULT 0;
ALTER TABLE products ADD COLUMN IF NOT EXISTS sku TEXT;
ALTER TABLE products ADD COLUMN IF NOT EXISTS unit TEXT NOT NULL DEFAULT 'un';
