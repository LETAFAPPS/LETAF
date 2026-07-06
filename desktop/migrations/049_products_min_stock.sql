-- Espelha server/050_products_min_stock.sql (Fase 9 — tela Produtos).
-- Estoque mínimo desejado por produto: alimenta o status "baixo" e a
-- sugestão de compra. NOT NULL DEFAULT 0 preserva os produtos
-- existentes. Sincroniza normalmente (last-write-wins por updated_at).
ALTER TABLE products ADD COLUMN min_stock REAL NOT NULL DEFAULT 0;
