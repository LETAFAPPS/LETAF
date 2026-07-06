-- Estoque mínimo desejado por produto (Fase 9 — tela Produtos).
-- Usado para:
--   * status de estoque "baixo" (0 < stock_quantity <= min_stock);
--   * sugestão de compra (quanto comprar p/ voltar ao mínimo).
-- NOT NULL DEFAULT 0 preserva produtos existentes (sem alerta de
-- mínimo até o operador configurar). Sincroniza normalmente
-- (last-write-wins por updated_at).
ALTER TABLE products ADD COLUMN IF NOT EXISTS min_stock DOUBLE PRECISION NOT NULL DEFAULT 0;
